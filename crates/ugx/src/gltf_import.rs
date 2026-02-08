//! glTF import for UGX models.
//!
//! Converts glTF 2.0 format back to UGX geometry.
//!
//! # Matrix convention notes
//!
//! Our glTF export writes DX row-major matrices as flat rows into glTF's column-major
//! storage. On import we reverse this: read 16 floats from glTF as DX row-major directly.

use base64::{engine::general_purpose::STANDARD, Engine};

use crate::error::{Error, Result};
use crate::types::*;
use crate::ugx::{GrannyBone, UgxGeom};
use crate::univert_packer::{UnivertPacker, UnpackedVertex, MAX_UV};
use crate::vertex_element::VertexElementType;

/// Import options for glTF → UGX conversion.
#[derive(Debug, Clone)]
pub struct GltfImportOptions {
    /// Import skeleton/bones if present (default: true).
    pub include_skeleton: bool,
    /// Import materials if present (default: true).
    pub include_materials: bool,
}

impl Default for GltfImportOptions {
    fn default() -> Self {
        Self {
            include_skeleton: true,
            include_materials: true,
        }
    }
}

/// Import a glTF document (JSON string + optional binary buffer) into a UgxGeom.
///
/// When `buffer_data` is None, embedded base64 buffers in the JSON are decoded automatically.
pub fn import_from_gltf(
    json_str: &str,
    buffer_data: Option<&[u8]>,
    options: &GltfImportOptions,
) -> Result<UgxGeom> {
    let root: gltf_json::Root = serde_json::from_str(json_str)
        .map_err(|e| Error::UnsupportedFormat(format!("Invalid glTF JSON: {}", e)))?;

    // Resolve buffer data
    let buffer_bytes = resolve_buffer(&root, buffer_data)?;

    // Import skeleton if present
    let (bones, granny_bones) = if options.include_skeleton {
        import_skeleton(&root, &buffer_bytes)?
    } else {
        (Vec::new(), Vec::new())
    };

    // Import materials
    let materials = if options.include_materials {
        import_materials(&root)
    } else {
        Vec::new()
    };

    // Import mesh data
    let mut all_vertices: Vec<UnpackedVertex> = Vec::new();
    let mut all_vertex_buffer: Vec<u8> = Vec::new();
    let mut all_index_buffer: Vec<u16> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();

    let has_skeleton = !bones.is_empty();

    for mesh in &root.meshes {
        for primitive in &mesh.primitives {
            let (vertices, indices, material_index) =
                import_primitive(primitive, &root, &buffer_bytes, has_skeleton, bones.len())?;

            if vertices.is_empty() || indices.is_empty() {
                continue;
            }

            // Choose vertex format based on what data is present
            let has_tangents = vertices.iter().any(|v| {
                v.tangent[0] != 0.0 || v.tangent[1] != 0.0 || v.tangent[2] != 0.0
            });
            let has_skin = has_skeleton && vertices.iter().any(|v| {
                v.bone_weights.iter().sum::<f32>() > 0.0
            });
            let max_texcoords = vertices.iter().map(|v| v.num_texcoords).max().unwrap_or(0);

            // Build pack order
            let mut pack_order = String::from("PN");
            if has_tangents {
                pack_order.push_str("A0");
            }
            for i in 0..max_texcoords {
                pack_order.push('T');
                pack_order.push(char::from_digit(i as u32, 10).unwrap_or('0'));
            }
            if has_skin {
                pack_order.push('S');
            }

            let mut uv_types = [VertexElementType::Ignore; MAX_UV];
            for i in 0..max_texcoords.min(MAX_UV) {
                uv_types[i] = VertexElementType::Float2;
            }

            let packer = UnivertPacker {
                pack_order: pack_order.clone(),
                decl_order: pack_order,
                pos_type: VertexElementType::Float3,
                basis_type: VertexElementType::Float4,
                basis_scale_type: VertexElementType::Float2,
                tangent_type: if has_tangents { VertexElementType::Float4 } else { VertexElementType::Ignore },
                normal_type: VertexElementType::Float3,
                uv_types,
                indices_type: if has_skin { VertexElementType::UByte4 } else { VertexElementType::Ignore },
                weights_type: if has_skin { VertexElementType::Float4 } else { VertexElementType::Ignore },
                diffuse_type: VertexElementType::Ignore,
                index_type: VertexElementType::Ignore,
            };

            // Pack vertices into binary buffer
            let vb_offset = all_vertex_buffer.len() as i32;
            for v in &vertices {
                packer.pack_vertex(&mut all_vertex_buffer, v)?;
            }
            let vb_bytes = (all_vertex_buffer.len() as i32) - vb_offset;
            let vert_size = packer.vertex_size() as i32;

            // Add indices
            let ib_offset = all_index_buffer.len() as i32;
            all_index_buffer.extend_from_slice(&indices);
            let num_tris = (indices.len() / 3) as i32;

            sections.push(Section {
                material_index,
                accessory_index: -1,
                max_bones: if has_skin { bones.len() as i32 } else { 0 },
                rigid_bone_index: if !has_skin && !bones.is_empty() { 0 } else { -1 },
                ib_offset,
                num_tris,
                vb_offset,
                vb_bytes,
                vert_size,
                num_verts: vertices.len() as i32,
                base_vert_packer: packer,
                rigid_only: !has_skin,
                global_bones: false,
            });

            all_vertices.extend(vertices);
        }
    }

    // Compute bounding volumes from all vertices
    let (bounds, bounding_sphere) = compute_bounds(&all_vertices);

    // Compute bone bounds
    let bone_bounds = if !bones.is_empty() {
        bones.iter().map(|_| bounds.clone()).collect()
    } else {
        Vec::new()
    };

    let has_any_skin = sections.iter().any(|s| !s.rigid_only);
    let all_rigid = sections.iter().all(|s| s.rigid_only);

    Ok(UgxGeom {
        bounding_sphere,
        bounds,
        materials,
        bones,
        granny_bones,
        bone_bounds,
        sections,
        vertex_buffer: all_vertex_buffer,
        index_buffer: all_index_buffer,
        rigid_only: all_rigid && !has_any_skin,
        rigid_bone_index: 0,
        all_sections_rigid: all_rigid,
        all_sections_skinned: has_any_skin && !all_rigid,
        global_bones: false,
    })
}

/// Resolve the binary buffer data from either the provided bytes or embedded base64.
fn resolve_buffer(
    root: &gltf_json::Root,
    external_data: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if let Some(data) = external_data {
        return Ok(data.to_vec());
    }

    // Try to decode from the first buffer's URI (base64 embedded)
    if let Some(buffer) = root.buffers.first() {
        if let Some(ref uri) = buffer.uri {
            if let Some(base64_data) = uri.strip_prefix("data:application/octet-stream;base64,") {
                let decoded = STANDARD
                    .decode(base64_data)
                    .map_err(|e| Error::UnsupportedFormat(format!("Invalid base64 buffer: {}", e)))?;
                return Ok(decoded);
            }
        }
    }

    // No buffer data available — might be a mesh with no buffer
    Ok(Vec::new())
}

/// Read accessor data as f32 values from the buffer.
fn read_accessor_f32(
    accessor: &gltf_json::Accessor,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<Vec<f32>> {
    let view_idx = accessor.buffer_view
        .ok_or_else(|| Error::UnsupportedFormat("Accessor missing buffer_view".into()))?;
    let view = &root.buffer_views[view_idx.value()];

    let byte_offset = accessor.byte_offset.map(|o| o.0 as usize).unwrap_or(0)
        + view.byte_offset.map(|o| o.0 as usize).unwrap_or(0);
    let stride = view.byte_stride.map(|s| s.0 as usize);
    let count = accessor.count.0 as usize;

    let components = match accessor.type_ {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Scalar) => 1,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec2) => 2,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec3) => 3,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec4) => 4,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Mat4) => 16,
        _ => return Err(Error::UnsupportedFormat("Unsupported accessor type".into())),
    };

    let component_size = match accessor.component_type {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::GenericComponentType(ct)) => {
            match ct {
                gltf_json::accessor::ComponentType::F32 => 4,
                gltf_json::accessor::ComponentType::U8 => 1,
                gltf_json::accessor::ComponentType::U16 => 2,
                gltf_json::accessor::ComponentType::I8 => 1,
                gltf_json::accessor::ComponentType::I16 => 2,
                gltf_json::accessor::ComponentType::U32 => 4,
            }
        }
        _ => return Err(Error::UnsupportedFormat("Invalid component type".into())),
    };

    let element_size = components * component_size;
    let actual_stride = stride.unwrap_or(element_size);

    let mut result = Vec::with_capacity(count * components);
    for i in 0..count {
        let elem_offset = byte_offset + i * actual_stride;
        for c in 0..components {
            let offset = elem_offset + c * component_size;
            let value = match accessor.component_type {
                gltf_json::validation::Checked::Valid(gltf_json::accessor::GenericComponentType(ct)) => {
                    match ct {
                        gltf_json::accessor::ComponentType::F32 => {
                            f32::from_le_bytes(buffer_bytes[offset..offset + 4].try_into().unwrap())
                        }
                        gltf_json::accessor::ComponentType::U8 => buffer_bytes[offset] as f32,
                        gltf_json::accessor::ComponentType::U16 => {
                            u16::from_le_bytes(buffer_bytes[offset..offset + 2].try_into().unwrap()) as f32
                        }
                        gltf_json::accessor::ComponentType::I8 => buffer_bytes[offset] as i8 as f32,
                        gltf_json::accessor::ComponentType::I16 => {
                            i16::from_le_bytes(buffer_bytes[offset..offset + 2].try_into().unwrap()) as f32
                        }
                        gltf_json::accessor::ComponentType::U32 => {
                            u32::from_le_bytes(buffer_bytes[offset..offset + 4].try_into().unwrap()) as f32
                        }
                    }
                }
                _ => 0.0,
            };
            result.push(value);
        }
    }

    Ok(result)
}

/// Import a single mesh primitive.
fn import_primitive(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    has_skeleton: bool,
    bone_count: usize,
) -> Result<(Vec<UnpackedVertex>, Vec<u16>, i32)> {
    use gltf_json::mesh::Semantic;
    use gltf_json::validation::Checked::Valid;

    // Read positions
    let (positions, vertex_count) = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Positions)) {
        let acc = &root.accessors[acc_idx.value()];
        let count = acc.count.0 as usize;
        (read_accessor_f32(acc, root, buffer_bytes)?, count)
    } else {
        return Err(Error::UnsupportedFormat("Mesh primitive missing POSITION".into()));
    };

    // Read normals
    let normals = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Normals)) {
        let acc = &root.accessors[acc_idx.value()];
        read_accessor_f32(acc, root, buffer_bytes)?
    } else {
        vec![0.0, 1.0, 0.0].repeat(vertex_count)
    };

    // Read tangents
    let tangents = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Tangents)) {
        let acc = &root.accessors[acc_idx.value()];
        Some(read_accessor_f32(acc, root, buffer_bytes)?)
    } else {
        None
    };

    // Read UV sets
    let mut uv_sets: Vec<Vec<f32>> = Vec::new();
    for i in 0..MAX_UV {
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::TexCoords(i as u32))) {
            let acc = &root.accessors[acc_idx.value()];
            uv_sets.push(read_accessor_f32(acc, root, buffer_bytes)?);
        } else {
            break;
        }
    }

    // Read joints
    let joints = if has_skeleton {
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Joints(0))) {
            let acc = &root.accessors[acc_idx.value()];
            Some(read_accessor_f32(acc, root, buffer_bytes)?)
        } else {
            None
        }
    } else {
        None
    };

    // Read weights
    let weights = if has_skeleton {
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Weights(0))) {
            let acc = &root.accessors[acc_idx.value()];
            Some(read_accessor_f32(acc, root, buffer_bytes)?)
        } else {
            None
        }
    } else {
        None
    };

    // Read indices
    let indices = if let Some(ref idx_accessor) = primitive.indices {
        let acc = &root.accessors[idx_accessor.value()];
        let raw = read_accessor_f32(acc, root, buffer_bytes)?;
        raw.iter().map(|&v| v as u16).collect::<Vec<_>>()
    } else {
        // No index buffer — generate sequential indices
        (0..vertex_count as u16).collect()
    };

    // Build vertices
    let mut vertices = Vec::with_capacity(vertex_count);
    let max_bone_idx = if bone_count > 0 { (bone_count - 1) as u16 } else { 0 };

    for i in 0..vertex_count {
        let mut vertex = UnpackedVertex::default();

        // Position
        vertex.position = [
            positions[i * 3],
            positions[i * 3 + 1],
            positions[i * 3 + 2],
        ];

        // Normal
        vertex.normal = [
            normals[i * 3],
            normals[i * 3 + 1],
            normals[i * 3 + 2],
        ];

        // Tangent
        if let Some(ref t) = tangents {
            vertex.tangent = [t[i * 4], t[i * 4 + 1], t[i * 4 + 2], t[i * 4 + 3]];
        }

        // UVs
        vertex.num_texcoords = uv_sets.len();
        for (uv_idx, uv_data) in uv_sets.iter().enumerate() {
            if uv_idx < 4 {
                vertex.texcoords[uv_idx] = [uv_data[i * 2], uv_data[i * 2 + 1]];
            }
        }

        // Joints and weights
        if let (Some(ref j), Some(ref w)) = (&joints, &weights) {
            let mut bone_indices = [0u16; 4];
            let mut bone_weights = [0.0f32; 4];
            for k in 0..4 {
                let joint_0based = j[i * 4 + k] as u16;
                // Convert 0-based glTF joint to 1-based UGX bone index
                bone_indices[k] = if w[i * 4 + k] > 0.0 {
                    (joint_0based + 1).min(max_bone_idx + 1)
                } else {
                    0
                };
                bone_weights[k] = w[i * 4 + k];
            }
            vertex.bone_indices = bone_indices;
            vertex.bone_weights = bone_weights;
        }

        vertices.push(vertex);
    }

    // Material index from primitive
    let material_index = primitive.material.map(|m| m.value() as i32).unwrap_or(-1);

    Ok((vertices, indices, material_index))
}

/// Import skeleton from glTF skin.
fn import_skeleton(
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<(Vec<Bone>, Vec<GrannyBone>)> {
    if root.skins.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let skin = &root.skins[0];
    let joint_count = skin.joints.len();
    if joint_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Read inverse bind matrices
    let ibm_data = if let Some(ref ibm_accessor) = skin.inverse_bind_matrices {
        let acc = &root.accessors[ibm_accessor.value()];
        read_accessor_f32(acc, root, buffer_bytes)?
    } else {
        // Default to identity matrices
        let mut data = Vec::with_capacity(joint_count * 16);
        for _ in 0..joint_count {
            data.extend_from_slice(&[
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ]);
        }
        data
    };

    // Build a map from node index → joint index
    let mut node_to_joint: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (joint_idx, joint_node) in skin.joints.iter().enumerate() {
        node_to_joint.insert(joint_node.value(), joint_idx);
    }

    // Build bones from joint nodes
    let mut bones = Vec::with_capacity(joint_count);
    let mut granny_bones = Vec::with_capacity(joint_count);

    for (joint_idx, joint_node_idx) in skin.joints.iter().enumerate() {
        let node = &root.nodes[joint_node_idx.value()];

        let name = node.name.clone().unwrap_or_else(|| format!("bone_{}", joint_idx));

        // Find parent: look through all joint nodes to find one that has this node as a child
        let mut parent_index: i32 = -1;
        for (other_idx, other_node_idx) in skin.joints.iter().enumerate() {
            if other_idx == joint_idx {
                continue;
            }
            let other_node = &root.nodes[other_node_idx.value()];
            if let Some(ref children) = other_node.children {
                if children.iter().any(|c| c.value() == joint_node_idx.value()) {
                    parent_index = other_idx as i32;
                    break;
                }
            }
        }

        // Read inverse bind matrix (16 floats)
        // Our export wrote DX row-major matrix rows flat into glTF column-major storage.
        // So on import we just read the 16 floats back as DX row-major.
        let ibm_offset = joint_idx * 16;
        let mut rows = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                rows[r][c] = ibm_data[ibm_offset + r * 4 + c];
            }
        }
        let model_to_bone = Matrix4x4 { rows };

        bones.push(Bone {
            name: name.clone(),
            parent_index,
            model_to_bone: model_to_bone.clone(),
        });

        granny_bones.push(GrannyBone {
            name,
            parent_index,
            inverse_world_matrix: model_to_bone,
        });
    }

    Ok((bones, granny_bones))
}

/// Import materials from glTF.
fn import_materials(root: &gltf_json::Root) -> Vec<Material> {
    root.materials
        .iter()
        .map(|mat| {
            let base_color = mat.pbr_metallic_roughness.base_color_factor.0;
            let roughness = mat.pbr_metallic_roughness.roughness_factor.0;

            Material {
                name: mat.name.clone().unwrap_or_default(),
                diff_color: [base_color[0], base_color[1], base_color[2]],
                spec_power: (1.0 - roughness) * 100.0,
                spec_level: 0.0,
                ..Default::default()
            }
        })
        .collect()
}

/// Compute AABB and bounding sphere from vertices.
fn compute_bounds(vertices: &[UnpackedVertex]) -> (AABB, Sphere) {
    if vertices.is_empty() {
        return (AABB::default(), Sphere::default());
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];

    for v in vertices {
        for i in 0..3 {
            min[i] = min[i].min(v.position[i]);
            max[i] = max[i].max(v.position[i]);
        }
    }

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];

    let mut max_dist_sq = 0.0f32;
    for v in vertices {
        let dx = v.position[0] - center[0];
        let dy = v.position[1] - center[1];
        let dz = v.position[2] - center[2];
        max_dist_sq = max_dist_sq.max(dx * dx + dy * dy + dz * dz);
    }

    (
        AABB { min, max },
        Sphere {
            center,
            radius: max_dist_sq.sqrt(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf_export::{export_to_gltf, GltfExportOptions};

    /// Create a test UgxGeom with vertices, bones, and skin data.
    fn make_test_geom() -> UgxGeom {
        let mut uv_types = [VertexElementType::Ignore; MAX_UV];
        uv_types[0] = VertexElementType::Float2;

        let packer = UnivertPacker {
            pack_order: "PNT0S".to_string(),
            decl_order: "PNT0S".to_string(),
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Float4,
            basis_scale_type: VertexElementType::Float2,
            tangent_type: VertexElementType::Ignore,
            normal_type: VertexElementType::Float3,
            uv_types,
            indices_type: VertexElementType::UByte4,
            weights_type: VertexElementType::Float4,
            diffuse_type: VertexElementType::Ignore,
            index_type: VertexElementType::Ignore,
        };

        let vertices = vec![
            UnpackedVertex {
                position: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: [[0.0, 0.0], [0.0; 2], [0.0; 2], [0.0; 2]],
                num_texcoords: 1,
                bone_indices: [1, 0, 0, 0],
                bone_weights: [1.0, 0.0, 0.0, 0.0],
                ..Default::default()
            },
            UnpackedVertex {
                position: [1.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: [[1.0, 0.0], [0.0; 2], [0.0; 2], [0.0; 2]],
                num_texcoords: 1,
                bone_indices: [1, 2, 0, 0],
                bone_weights: [0.7, 0.3, 0.0, 0.0],
                ..Default::default()
            },
            UnpackedVertex {
                position: [0.0, 1.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: [[0.0, 1.0], [0.0; 2], [0.0; 2], [0.0; 2]],
                num_texcoords: 1,
                bone_indices: [2, 0, 0, 0],
                bone_weights: [1.0, 0.0, 0.0, 0.0],
                ..Default::default()
            },
        ];

        let mut vertex_buffer = Vec::new();
        for v in &vertices {
            packer.pack_vertex(&mut vertex_buffer, v).unwrap();
        }

        let vert_size = packer.vertex_size() as i32;
        let vb_bytes = vertex_buffer.len() as i32;

        let bones = vec![
            Bone {
                name: "root".to_string(),
                parent_index: -1,
                model_to_bone: Matrix4x4::identity(),
            },
            Bone {
                name: "child".to_string(),
                parent_index: 0,
                model_to_bone: Matrix4x4::identity(),
            },
        ];

        let granny_bones = vec![
            GrannyBone {
                name: "root".to_string(),
                parent_index: -1,
                inverse_world_matrix: Matrix4x4::identity(),
            },
            GrannyBone {
                name: "child".to_string(),
                parent_index: 0,
                inverse_world_matrix: Matrix4x4::identity(),
            },
        ];

        UgxGeom {
            bounding_sphere: Sphere { center: [0.5, 0.5, 0.0], radius: 1.0 },
            bounds: AABB { min: [0.0, 0.0, 0.0], max: [1.0, 1.0, 0.0] },
            materials: Vec::new(),
            bones,
            granny_bones,
            bone_bounds: vec![
                AABB { min: [0.0, 0.0, 0.0], max: [1.0, 1.0, 0.0] },
                AABB { min: [0.0, 0.0, 0.0], max: [1.0, 1.0, 0.0] },
            ],
            sections: vec![Section {
                material_index: -1,
                accessory_index: -1,
                max_bones: 2,
                rigid_bone_index: -1,
                ib_offset: 0,
                num_tris: 1,
                vb_offset: 0,
                vb_bytes,
                vert_size,
                num_verts: 3,
                base_vert_packer: packer,
                rigid_only: false,
                global_bones: false,
            }],
            vertex_buffer,
            index_buffer: vec![0, 1, 2],
            rigid_only: false,
            rigid_bone_index: -1,
            all_sections_rigid: false,
            all_sections_skinned: true,
            global_bones: false,
        }
    }

    #[test]
    fn test_gltf_import_roundtrip_positions() {
        let original = make_test_geom();

        // Export to glTF with external buffer
        let opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: false,
        };
        let export = export_to_gltf(&original, &opts).unwrap();

        // Import back
        let import_opts = GltfImportOptions {
            include_skeleton: false,
            include_materials: false,
        };
        let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // Compare vertex positions
        assert_eq!(imported.sections.len(), 1);
        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();
        assert_eq!(imported_verts.len(), orig_verts.len());

        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            for i in 0..3 {
                assert!(
                    (orig.position[i] - imp.position[i]).abs() < 1e-5,
                    "position[{}] mismatch: {} vs {}", i, orig.position[i], imp.position[i]
                );
            }
        }
    }

    #[test]
    fn test_gltf_import_roundtrip_normals_uvs() {
        let original = make_test_geom();

        let opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: false,
        };
        let export = export_to_gltf(&original, &opts).unwrap();

        let import_opts = GltfImportOptions {
            include_skeleton: false,
            include_materials: false,
        };
        let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();

        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            // Normals
            for i in 0..3 {
                assert!(
                    (orig.normal[i] - imp.normal[i]).abs() < 1e-4,
                    "normal[{}] mismatch: {} vs {}", i, orig.normal[i], imp.normal[i]
                );
            }
            // UVs
            for i in 0..2 {
                assert!(
                    (orig.texcoords[0][i] - imp.texcoords[0][i]).abs() < 1e-5,
                    "uv[0][{}] mismatch: {} vs {}", i, orig.texcoords[0][i], imp.texcoords[0][i]
                );
            }
        }
    }

    #[test]
    fn test_gltf_import_roundtrip_skeleton() {
        let original = make_test_geom();

        let opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &opts).unwrap();

        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // Check bone count and names
        assert_eq!(imported.bones.len(), original.bones.len());
        for (orig, imp) in original.bones.iter().zip(imported.bones.iter()) {
            assert_eq!(imp.name, orig.name);
            assert_eq!(imp.parent_index, orig.parent_index);
        }
    }

    #[test]
    fn test_gltf_import_roundtrip_skin_data() {
        let original = make_test_geom();

        let opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &opts).unwrap();

        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();

        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            // Bone indices should match
            assert_eq!(imp.bone_indices, orig.bone_indices,
                "bone_indices mismatch: {:?} vs {:?}", imp.bone_indices, orig.bone_indices);

            // Bone weights should be close
            for k in 0..4 {
                assert!(
                    (orig.bone_weights[k] - imp.bone_weights[k]).abs() < 1e-4,
                    "bone_weight[{}] mismatch: {} vs {}", k, orig.bone_weights[k], imp.bone_weights[k]
                );
            }
        }
    }

    #[test]
    fn test_gltf_import_embedded_base64() {
        let original = make_test_geom();

        // Export with embedded base64
        let opts = GltfExportOptions {
            embed_buffers: true,
            include_materials: false,
            include_skeleton: false,
        };
        let export = export_to_gltf(&original, &opts).unwrap();
        assert!(export.buffer.is_none(), "embedded export should not have separate buffer");

        // Import with no external buffer (should decode from base64)
        let import_opts = GltfImportOptions {
            include_skeleton: false,
            include_materials: false,
        };
        let imported = import_from_gltf(&export.json, None, &import_opts).unwrap();

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();
        assert_eq!(imported_verts.len(), orig_verts.len());
        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            assert_eq!(orig.position, imp.position);
        }
    }

    #[test]
    fn test_full_roundtrip_ugx_gltf_ugx() {
        let original = make_test_geom();

        // 1. Export to glTF
        let export_opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &export_opts).unwrap();

        // 2. Import from glTF
        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // 3. Write to UGX bytes
        let ugx_bytes = crate::write_ugx(&imported).unwrap();

        // 4. Read back
        let re_read = UgxGeom::read(&ugx_bytes).unwrap();

        // 5. Verify
        assert_eq!(re_read.sections.len(), 1);
        assert_eq!(re_read.bones.len(), original.bones.len());

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let final_verts = re_read.unpack_section_vertices(0).unwrap();
        assert_eq!(final_verts.len(), orig_verts.len());

        for (vi, (orig, fin)) in orig_verts.iter().zip(final_verts.iter()).enumerate() {
            // Positions
            for i in 0..3 {
                assert!(
                    (orig.position[i] - fin.position[i]).abs() < 1e-4,
                    "vertex {} position[{}] mismatch: {} vs {}", vi, i, orig.position[i], fin.position[i]
                );
            }

            // Normals
            for i in 0..3 {
                assert!(
                    (orig.normal[i] - fin.normal[i]).abs() < 1e-4,
                    "vertex {} normal[{}] mismatch: {} vs {}", vi, i, orig.normal[i], fin.normal[i]
                );
            }

            // UVs
            for i in 0..orig.num_texcoords {
                for j in 0..2 {
                    assert!(
                        (orig.texcoords[i][j] - fin.texcoords[i][j]).abs() < 1e-4,
                        "vertex {} texcoord[{}][{}] mismatch: {} vs {}", vi, i, j, orig.texcoords[i][j], fin.texcoords[i][j]
                    );
                }
            }

            // Bone indices
            assert_eq!(
                orig.bone_indices, fin.bone_indices,
                "vertex {} bone_indices mismatch: {:?} vs {:?}", vi, orig.bone_indices, fin.bone_indices
            );

            // Bone weights
            for i in 0..4 {
                assert!(
                    (orig.bone_weights[i] - fin.bone_weights[i]).abs() < 1e-4,
                    "vertex {} bone_weights[{}] mismatch: {} vs {}", vi, i, orig.bone_weights[i], fin.bone_weights[i]
                );
            }
        }

        // Verify indices survived
        let orig_indices = original.get_section_indices(0);
        let final_indices = re_read.get_section_indices(0);
        assert_eq!(final_indices, orig_indices);

        // Verify bone names and hierarchy survived
        assert_eq!(re_read.bones.len(), original.bones.len());
        for (bi, (orig_bone, fin_bone)) in original.bones.iter().zip(re_read.bones.iter()).enumerate() {
            assert_eq!(
                orig_bone.name, fin_bone.name,
                "bone {} name mismatch: {:?} vs {:?}", bi, orig_bone.name, fin_bone.name
            );
            assert_eq!(
                orig_bone.parent_index, fin_bone.parent_index,
                "bone {} parent_index mismatch: {} vs {}", bi, orig_bone.parent_index, fin_bone.parent_index
            );
        }

        // Verify granny bones survived the round trip
        assert_eq!(
            re_read.granny_bones.len(), original.granny_bones.len(),
            "granny_bones count mismatch: {} vs {}", re_read.granny_bones.len(), original.granny_bones.len()
        );
        for (bi, (orig_gb, fin_gb)) in original.granny_bones.iter().zip(re_read.granny_bones.iter()).enumerate() {
            assert_eq!(
                orig_gb.name, fin_gb.name,
                "granny_bone {} name mismatch: {:?} vs {:?}", bi, orig_gb.name, fin_gb.name
            );
            assert_eq!(
                orig_gb.parent_index, fin_gb.parent_index,
                "granny_bone {} parent_index mismatch: {} vs {}", bi, orig_gb.parent_index, fin_gb.parent_index
            );
            // Compare inverse world matrices
            for row in 0..4 {
                for col in 0..4 {
                    assert!(
                        (orig_gb.inverse_world_matrix.rows[row][col] - fin_gb.inverse_world_matrix.rows[row][col]).abs() < 1e-4,
                        "granny_bone {} matrix[{}][{}] mismatch: {} vs {}",
                        bi, row, col,
                        orig_gb.inverse_world_matrix.rows[row][col],
                        fin_gb.inverse_world_matrix.rows[row][col]
                    );
                }
            }
        }

        // Verify bounding volumes are reasonable
        assert!(re_read.bounding_sphere.radius > 0.0, "bounding sphere radius should be positive");
        for i in 0..3 {
            assert!(
                re_read.bounds.max[i] >= re_read.bounds.min[i],
                "AABB max[{}] < min[{}]", i, i
            );
        }
    }
}
