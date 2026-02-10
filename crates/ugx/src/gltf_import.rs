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
use crate::ugx::{GrannyBone, GrannyMesh, UgxGeom};
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

    // Track mesh names, vertex ranges, and section ranges for granny_meshes generation
    // (name, start_vertex, end_vertex, start_section, end_section)
    let mut mesh_infos: Vec<(String, usize, usize, usize, usize)> = Vec::new();

    let has_skeleton = !bones.is_empty();

    for (mesh_idx, mesh) in root.meshes.iter().enumerate() {
        let mesh_name = mesh
            .name
            .clone()
            .unwrap_or_else(|| format!("mesh_{}", mesh_idx));
        let mesh_start_vertex = all_vertices.len();
        let mesh_start_section = sections.len();

        for primitive in &mesh.primitives {
            let (vertices, indices, material_index) =
                import_primitive(primitive, &root, &buffer_bytes, has_skeleton, bones.len())?;

            if vertices.is_empty() || indices.is_empty() {
                continue;
            }

            // Choose vertex format based on what data is present
            let has_tangents = vertices
                .iter()
                .any(|v| v.tangent[0] != 0.0 || v.tangent[1] != 0.0 || v.tangent[2] != 0.0);
            let has_skin = has_skeleton
                && vertices
                    .iter()
                    .any(|v| v.bone_weights.iter().sum::<f32>() > 0.0);
            let has_colors = vertices.iter().any(|v| {
                v.diffuse[0] != 0.0
                    || v.diffuse[1] != 0.0
                    || v.diffuse[2] != 0.0
                    || v.diffuse[3] != 0.0
            });
            let max_texcoords = vertices.iter().map(|v| v.num_texcoords).max().unwrap_or(0);

            // Build pack order - game format: PNA0ST0 (skin before texcoords for skinned meshes)
            // For rigid meshes: PNA0T0 (no skin data)
            let mut pack_order = String::from("PN");
            if has_tangents {
                pack_order.push_str("A0");
            }
            // Skin data comes before texcoords in game format
            if has_skin {
                pack_order.push('S');
            }
            for i in 0..max_texcoords {
                pack_order.push('T');
                pack_order.push(char::from_digit(i as u32, 10).unwrap_or('0'));
            }
            if has_colors {
                pack_order.push('D');
            }

            // Use compact vertex types matching game format
            let mut uv_types = [VertexElementType::Ignore; MAX_UV];
            for i in 0..max_texcoords.min(MAX_UV) {
                uv_types[i] = VertexElementType::HalfFloat2; // Game uses HalfFloat2 for UVs
            }

            // UnivertPacker with game-standard defaults for ALL fields.
            // The game sets these defaults regardless of whether they're used in pack_order:
            //   setPos(eHALFFLOAT4), setNorm(eDEC3N), setBasis(eDEC3N), setBasisScales(eHALFFLOAT2)
            //   setTangent(eDEC3N), setIndices(eUBYTE4), setWeights(eUBYTE4N), setDiffuse(eD3DCOLOR)
            //   setUV(eHALFFLOAT2)
            // The actual marine_01.ugx uses Float3 for tangent/basis/normal instead of DEC3N.
            // We match the original file values exactly.
            let packer = UnivertPacker {
                pack_order: pack_order.clone(),
                decl_order: String::new(), // Original has empty decl_order
                pos_type: VertexElementType::HalfFloat4,
                basis_type: VertexElementType::Float3, // Game default, even if unused
                basis_scale_type: VertexElementType::Float2,
                tangent_type: VertexElementType::Float3, // Game default, even if unused
                normal_type: VertexElementType::Float3,
                uv_types,
                indices_type: VertexElementType::UByte4, // Game default, even if unused
                weights_type: VertexElementType::UByte4N, // Game default, even if unused
                diffuse_type: VertexElementType::D3DColor, // Game default, even if unused
                index_type: VertexElementType::Ignore,
            };

            // Analyze bone usage for this section
            // Detect global_bones sections:
            // If ALL vertices have weight[0]=1.0 and weights[1..3]=0.0 on the SAME bone,
            // this was originally a global_bones=true section with zero weights.
            // glTF export transforms zero-weight vertices to weight=1.0 on rigid bone.
            //
            // We detect this pattern and restore the original behavior:
            // - global_bones=true
            // - Zero weights on all vertices
            // - Pack order without skin data (PNT0 instead of PNST0)
            //
            // NOTE: max_bones is the maximum number of bone influences on ANY SINGLE VERTEX,
            // NOT the total unique bones in the section. This controls shader selection:
            // - max_bones=1 → ONE_BONE_REG=true (single bone optimization)
            // - max_bones=2 → neither flag set (2 bones)
            // - max_bones>2 → FOUR_BONES_REG=true (4 bones)
            let (is_global_bones, global_bone_idx, actual_max_bones) = if has_skin {
                let mut all_single_bone = true;
                let mut common_bone: Option<u16> = None;
                let mut max_influences_per_vertex = 0i32;

                for v in &vertices {
                    // Count how many non-zero weights this vertex has
                    let mut num_influences = 0;
                    for k in 0..4 {
                        if v.bone_weights[k] > 0.0 {
                            num_influences += 1;
                        }
                    }
                    max_influences_per_vertex = max_influences_per_vertex.max(num_influences);

                    // Check if this vertex has exactly weight[0]=1.0 and rest=0.0
                    let is_single_bone_vertex = v.bone_weights[0] > 0.99
                        && v.bone_weights[1] < 0.01
                        && v.bone_weights[2] < 0.01
                        && v.bone_weights[3] < 0.01;

                    if is_single_bone_vertex {
                        let bone = v.bone_indices[0];
                        match common_bone {
                            None => common_bone = Some(bone),
                            Some(cb) if cb != bone => all_single_bone = false,
                            _ => {}
                        }
                    } else {
                        all_single_bone = false;
                    }
                }

                let max_bones = max_influences_per_vertex.max(1);

                // If all vertices use the same single bone, this is a global_bones section
                if all_single_bone && common_bone.is_some() {
                    // Convert 1-based bone index to 0-based for rigid_bone_index
                    let bone_idx = (common_bone.unwrap() as i32) - 1;
                    (true, bone_idx, 1)
                } else {
                    (false, i32::MAX, max_bones)
                }
            } else {
                (false, i32::MAX, 1)
            };

            // For global_bones sections, restore zero weights and use simpler pack order
            let (final_packer, final_vertices) = if is_global_bones {
                // Rebuild packer without skin data for global_bones sections
                let mut global_pack_order = String::from("PN");
                if has_tangents {
                    global_pack_order.push_str("A0");
                }
                for i in 0..max_texcoords {
                    global_pack_order.push('T');
                    global_pack_order.push(char::from_digit(i as u32, 10).unwrap_or('0'));
                }
                if has_colors {
                    global_pack_order.push('D');
                }

                // Global bones packer - pack_order is PNT0 (no skin data), but we still
                // set game-standard defaults for ALL fields to match original file
                let global_packer = UnivertPacker {
                    pack_order: global_pack_order,
                    decl_order: String::new(), // Original has empty decl_order
                    pos_type: VertexElementType::HalfFloat4,
                    basis_type: VertexElementType::Float3, // Game default
                    basis_scale_type: VertexElementType::Float2,
                    tangent_type: VertexElementType::Float3, // Game default
                    normal_type: VertexElementType::Float3,
                    uv_types: uv_types.clone(),
                    indices_type: VertexElementType::UByte4, // Game default, even for PNT0
                    weights_type: VertexElementType::UByte4N, // Game default, even for PNT0
                    diffuse_type: VertexElementType::D3DColor, // Game default
                    index_type: VertexElementType::Ignore,
                };

                // Restore zero weights for global_bones vertices
                let restored_vertices: Vec<UnpackedVertex> = vertices
                    .iter()
                    .map(|v| {
                        let mut rv = v.clone();
                        rv.bone_weights = [0.0, 0.0, 0.0, 0.0];
                        rv.bone_indices = [0, 0, 0, 0];
                        rv
                    })
                    .collect();

                (global_packer, restored_vertices)
            } else {
                (packer, vertices)
            };

            // Pack vertices into binary buffer
            let vb_offset = all_vertex_buffer.len() as i32;
            for v in &final_vertices {
                final_packer.pack_vertex(&mut all_vertex_buffer, v)?;
            }
            let vb_bytes = (all_vertex_buffer.len() as i32) - vb_offset;
            let vert_size = final_packer.vertex_size() as i32;

            // Add indices
            let ib_offset = all_index_buffer.len() as i32;
            all_index_buffer.extend_from_slice(&indices);
            let num_tris = (indices.len() / 3) as i32;

            sections.push(Section {
                material_index,
                accessory_index: 0, // Default accessory index (0 = none)
                max_bones: actual_max_bones,
                rigid_bone_index: global_bone_idx,
                ib_offset,
                num_tris,
                vb_offset,
                vb_bytes,
                vert_size,
                num_verts: final_vertices.len() as i32,
                base_vert_packer: final_packer,
                bone_remap: Vec::new(),
                // rigid_only is always false from glTF import
                // global_bones is true for sections where all vertices use same bone
                rigid_only: false,
                global_bones: is_global_bones,
            });

            all_vertices.extend(final_vertices);
        }

        // Record mesh info for granny_meshes generation
        // Include section indices to handle global_bones sections
        let mesh_end_section = sections.len();
        let mesh_end_vertex = all_vertices.len();
        if mesh_end_vertex > mesh_start_vertex {
            mesh_infos.push((
                mesh_name,
                mesh_start_vertex,
                mesh_end_vertex,
                mesh_start_section,
                mesh_end_section,
            ));
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

    let all_rigid = sections.iter().all(|s| s.rigid_only);
    let all_skinned = sections.iter().all(|s| !s.rigid_only);
    // Header globalBones should be true if any section uses global bones
    let any_global_bones = sections.iter().any(|s| s.global_bones);

    // allSectionsSkinned is true only if:
    // - globalBones is true
    // - Not all sections are rigid
    // - The model is not rigidOnly
    // - ALL sections are skinned (none are rigidOnly)
    // - No sections use global_bones (section-level flag)
    //
    // Note: The original game code sets this flag, but the original marine_01.ugx
    // has it as false even though it meets the criteria. This suggests either:
    // 1. The original file was generated before this flag was implemented
    // 2. Or sections with global_bones=true don't count as "skinned"
    //
    // We match the original behavior: if any section has global_bones=true,
    // don't set all_sections_skinned=true (these sections are transformed differently)
    let all_sections_skinned = !any_global_bones && !all_rigid && all_skinned;

    // Generate granny_meshes from vertex skin data and section info, preserving mesh names from glTF
    let granny_meshes = generate_granny_meshes_from_vertices(
        &all_vertices,
        &granny_bones,
        &mesh_infos,
        &sections,
    );

    Ok(UgxGeom {
        bounding_sphere,
        bounds,
        materials,
        bones,
        granny_bones,
        granny_meshes,
        bone_bounds,
        sections,
        vertex_buffer: all_vertex_buffer,
        index_buffer: all_index_buffer,
        rigid_only: all_rigid,
        rigid_bone_index: 0,
        all_sections_rigid: all_rigid,
        all_sections_skinned,
        global_bones: any_global_bones,
    })
}

/// Generate `GrannyMesh` entries from vertex skin data and section info.
///
/// For glTF imports, we analyze which bones each vertex uses (via bone_weights > 0)
/// and create one mesh per glTF mesh, preserving the original mesh names.
/// For global_bones sections (zero weights), we use the rigid_bone_index instead.
/// This allows the game to properly skin the vertices.
fn generate_granny_meshes_from_vertices(
    vertices: &[UnpackedVertex],
    granny_bones: &[GrannyBone],
    mesh_infos: &[(String, usize, usize, usize, usize)], // (name, start_vertex, end_vertex, start_section, end_section)
    sections: &[Section],
) -> Vec<GrannyMesh> {
    let mut granny_meshes = Vec::new();

    for (mesh_name, start_vertex, end_vertex, start_section, end_section) in mesh_infos {
        // Collect all unique bone indices used by this mesh's vertices
        let mut used_bones: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();

        for v in &vertices[*start_vertex..*end_vertex] {
            for k in 0..4 {
                // bone_indices are 1-based, bone_weights[k] > 0 means the bone is used
                if v.bone_weights[k] > 0.0 && v.bone_indices[k] > 0 {
                    used_bones.insert(v.bone_indices[k]);
                }
            }
        }

        // For global_bones sections, vertices have zero weights but use rigid_bone_index
        // We need to include that bone in the mesh bindings
        for section in &sections[*start_section..*end_section] {
            if section.global_bones && section.rigid_bone_index >= 0 {
                // rigid_bone_index is 0-based, convert to 1-based for the set
                let bone_idx_1based = (section.rigid_bone_index as u16) + 1;
                used_bones.insert(bone_idx_1based);
            }
        }

        if used_bones.is_empty() {
            // No bones used in this mesh - skip it (fully rigid mesh with no bone reference)
            continue;
        }

        // Convert bone indices to bone names
        // bone_indices are 1-based, so subtract 1 to get the granny_bones index
        let bone_bindings: Vec<String> = used_bones
            .iter()
            .filter_map(|&idx| {
                let idx_0based = (idx as usize).saturating_sub(1);
                granny_bones.get(idx_0based).map(|b| b.name.clone())
            })
            .collect();

        if !bone_bindings.is_empty() {
            granny_meshes.push(GrannyMesh {
                name: mesh_name.clone(),
                bone_bindings,
            });
        }
    }

    granny_meshes
}

/// Resolve the binary buffer data from either the provided bytes or embedded base64.
fn resolve_buffer(root: &gltf_json::Root, external_data: Option<&[u8]>) -> Result<Vec<u8>> {
    if let Some(data) = external_data {
        return Ok(data.to_vec());
    }

    // Try to decode from the first buffer's URI (base64 embedded)
    if let Some(buffer) = root.buffers.first() {
        if let Some(ref uri) = buffer.uri {
            if let Some(base64_data) = uri.strip_prefix("data:application/octet-stream;base64,") {
                let decoded = STANDARD.decode(base64_data).map_err(|e| {
                    Error::UnsupportedFormat(format!("Invalid base64 buffer: {}", e))
                })?;
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
    let view_idx = accessor
        .buffer_view
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
                gltf_json::validation::Checked::Valid(
                    gltf_json::accessor::GenericComponentType(ct),
                ) => match ct {
                    gltf_json::accessor::ComponentType::F32 => {
                        f32::from_le_bytes(buffer_bytes[offset..offset + 4].try_into().unwrap())
                    }
                    gltf_json::accessor::ComponentType::U8 => buffer_bytes[offset] as f32,
                    gltf_json::accessor::ComponentType::U16 => {
                        u16::from_le_bytes(buffer_bytes[offset..offset + 2].try_into().unwrap())
                            as f32
                    }
                    gltf_json::accessor::ComponentType::I8 => buffer_bytes[offset] as i8 as f32,
                    gltf_json::accessor::ComponentType::I16 => {
                        i16::from_le_bytes(buffer_bytes[offset..offset + 2].try_into().unwrap())
                            as f32
                    }
                    gltf_json::accessor::ComponentType::U32 => {
                        u32::from_le_bytes(buffer_bytes[offset..offset + 4].try_into().unwrap())
                            as f32
                    }
                },
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
    let (positions, vertex_count) =
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Positions)) {
            let acc = &root.accessors[acc_idx.value()];
            let count = acc.count.0 as usize;
            (read_accessor_f32(acc, root, buffer_bytes)?, count)
        } else {
            return Err(Error::UnsupportedFormat(
                "Mesh primitive missing POSITION".into(),
            ));
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
        if let Some(acc_idx) = primitive
            .attributes
            .get(&Valid(Semantic::TexCoords(i as u32)))
        {
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

    // Read vertex colors (COLOR_0)
    let colors = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Colors(0))) {
        let acc = &root.accessors[acc_idx.value()];
        Some(read_accessor_f32(acc, root, buffer_bytes)?)
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
    let max_bone_idx = if bone_count > 0 {
        (bone_count - 1) as u16
    } else {
        0
    };

    for i in 0..vertex_count {
        let mut vertex = UnpackedVertex::default();

        // Position
        vertex.position = [positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]];

        // Normal
        vertex.normal = [normals[i * 3], normals[i * 3 + 1], normals[i * 3 + 2]];

        // Tangent
        if let Some(ref t) = tangents {
            vertex.tangent = [t[i * 4], t[i * 4 + 1], t[i * 4 + 2], t[i * 4 + 3]];
        }

        // UVs
        vertex.num_texcoords = uv_sets.len();
        for (uv_idx, uv_data) in uv_sets.iter().enumerate() {
            if uv_idx < MAX_UV {
                vertex.texcoords[uv_idx] = [uv_data[i * 2], uv_data[i * 2 + 1]];
            }
        }

        // Joints and weights
        if let (Some(ref j), Some(ref w)) = (&joints, &weights) {
            let mut bone_indices = [0u16; 4];
            let mut bone_weights = [0.0f32; 4];

            // First pass: find the first valid bone index for padding
            let mut first_valid_bone: u16 = 1; // Default to bone 1 if no valid bones
            for k in 0..4 {
                if w[i * 4 + k] > 0.0 {
                    let joint_0based = j[i * 4 + k] as u16;
                    // Clamp to valid range and convert to 1-based
                    first_valid_bone = (joint_0based.min(max_bone_idx) + 1).max(1);
                    break;
                }
            }

            // Second pass: set bone indices and weights
            for k in 0..4 {
                let joint_0based = j[i * 4 + k] as u16;
                bone_weights[k] = w[i * 4 + k];

                if bone_weights[k] > 0.0 {
                    // Convert 0-based glTF joint to 1-based UGX bone index
                    // Clamp to valid range to prevent out-of-bounds
                    bone_indices[k] = (joint_0based.min(max_bone_idx) + 1).max(1);
                } else {
                    // Use first valid bone for padding (game expects valid indices)
                    bone_indices[k] = first_valid_bone;
                }
            }
            vertex.bone_indices = bone_indices;
            vertex.bone_weights = bone_weights;
        }

        // Vertex colors
        if let Some(ref c) = colors {
            // COLOR_0 can be Vec3 or Vec4; handle both
            let stride = if c.len() == vertex_count * 4 { 4 } else { 3 };
            vertex.diffuse[0] = c[i * stride];
            vertex.diffuse[1] = c[i * stride + 1];
            vertex.diffuse[2] = c[i * stride + 2];
            vertex.diffuse[3] = if stride == 4 { c[i * stride + 3] } else { 1.0 };
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
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]);
        }
        data
    };

    // Build a map from node index → joint index
    let mut node_to_joint: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (joint_idx, joint_node) in skin.joints.iter().enumerate() {
        node_to_joint.insert(joint_node.value(), joint_idx);
    }

    // Build bones from joint nodes
    let mut bones = Vec::with_capacity(joint_count);
    let mut granny_bones = Vec::with_capacity(joint_count);

    for (joint_idx, joint_node_idx) in skin.joints.iter().enumerate() {
        let node = &root.nodes[joint_node_idx.value()];

        let name = node
            .name
            .clone()
            .unwrap_or_else(|| format!("bone_{}", joint_idx));

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

/// Resolve a glTF texture index to the image URI (or name as fallback).
fn resolve_texture_uri(root: &gltf_json::Root, texture_idx: usize) -> String {
    let texture = &root.textures[texture_idx];
    let image = &root.images[texture.source.value()];
    // Prefer name (full path preserved by our exporter), fall back to URI
    image
        .name
        .as_deref()
        .or(image.uri.as_deref())
        .unwrap_or_default()
        .to_string()
}

/// Import materials from glTF, including texture map references.
fn import_materials(root: &gltf_json::Root) -> Vec<Material> {
    root.materials
        .iter()
        .map(|mat| {
            let base_color = mat.pbr_metallic_roughness.base_color_factor.0;
            let roughness = mat.pbr_metallic_roughness.roughness_factor.0;

            let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();

            // baseColorTexture → Diffuse
            if let Some(ref info) = mat.pbr_metallic_roughness.base_color_texture {
                let name = resolve_texture_uri(root, info.index.value());
                if !name.is_empty() {
                    maps[MapType::Diffuse as usize].push(Map {
                        name,
                        channel: info.tex_coord as i16,
                        flags: 0,
                    });
                }
            }

            // normalTexture → Normal
            if let Some(ref info) = mat.normal_texture {
                let name = resolve_texture_uri(root, info.index.value());
                if !name.is_empty() {
                    maps[MapType::Normal as usize].push(Map {
                        name,
                        channel: info.tex_coord as i16,
                        flags: 0,
                    });
                }
            }

            // occlusionTexture → AO
            if let Some(ref info) = mat.occlusion_texture {
                let name = resolve_texture_uri(root, info.index.value());
                if !name.is_empty() {
                    maps[MapType::AO as usize].push(Map {
                        name,
                        channel: info.tex_coord as i16,
                        flags: 0,
                    });
                }
            }

            // emissiveTexture → Emissive
            if let Some(ref info) = mat.emissive_texture {
                let name = resolve_texture_uri(root, info.index.value());
                if !name.is_empty() {
                    maps[MapType::Emissive as usize].push(Map {
                        name,
                        channel: info.tex_coord as i16,
                        flags: 0,
                    });
                }
            }

            // Alpha mode → blend_type
            let blend_type = if let gltf_json::validation::Checked::Valid(
                gltf_json::material::AlphaMode::Blend,
            ) = mat.alpha_mode
            {
                1
            } else {
                0
            };

            // Read UGX extras (flags, uvw_velocity, non-PBR maps)
            let (flags, uvw_velocity, extra_maps) = read_material_extras(&mat.extras, &maps);

            // Merge extra maps into the maps array
            let mut final_maps = maps;
            for (idx, extra) in extra_maps {
                final_maps[idx] = extra;
            }

            Material {
                name: mat.name.clone().unwrap_or_default(),
                maps: final_maps,
                spec_power: (1.0 - roughness) * 100.0,
                opacity: base_color[3],
                blend_type,
                flags,
                uvw_velocity,
            }
        })
        .collect()
}

/// Read UGX material extras from glTF extras JSON.
///
/// Returns (flags, uvw_velocity, extra_maps) where extra_maps is a vec of
/// (map_type_index, Vec<Map>) for non-PBR map types.
fn read_material_extras(
    extras: &gltf_json::Extras,
    _existing_maps: &[Vec<Map>; MapType::NUM_TYPES],
) -> (u32, [[f32; 3]; MapType::NUM_TYPES], Vec<(usize, Vec<Map>)>) {
    let mut flags = 0u32;
    let mut uvw_velocity = [[0.0f32; 3]; MapType::NUM_TYPES];
    let mut extra_maps: Vec<(usize, Vec<Map>)> = Vec::new();

    let raw = match extras {
        Some(raw_value) => raw_value,
        None => return (flags, uvw_velocity, extra_maps),
    };

    let parsed: serde_json::Value = match serde_json::from_str(raw.get()) {
        Ok(v) => v,
        Err(_) => return (flags, uvw_velocity, extra_maps),
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return (flags, uvw_velocity, extra_maps),
    };

    // Read flags
    if let Some(v) = obj.get("ugx_flags") {
        flags = v.as_u64().unwrap_or(0) as u32;
    }

    // Read UVW velocity
    if let Some(serde_json::Value::Array(arr)) = obj.get("ugx_uvw_velocity") {
        for (i, val) in arr.iter().enumerate() {
            if i >= MapType::NUM_TYPES {
                break;
            }
            if let serde_json::Value::Array(v) = val {
                if v.len() >= 3 {
                    uvw_velocity[i][0] = v[0].as_f64().unwrap_or(0.0) as f32;
                    uvw_velocity[i][1] = v[1].as_f64().unwrap_or(0.0) as f32;
                    uvw_velocity[i][2] = v[2].as_f64().unwrap_or(0.0) as f32;
                }
            }
        }
    }

    // Read non-PBR maps
    if let Some(serde_json::Value::Object(maps_obj)) = obj.get("ugx_maps") {
        for map_type in MapType::ALL {
            let type_name = map_type.name();
            if let Some(serde_json::Value::Array(arr)) = maps_obj.get(type_name) {
                let mut map_vec = Vec::new();
                for entry in arr {
                    if let serde_json::Value::Object(m) = entry {
                        let name = m
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let channel = m.get("channel").and_then(|v| v.as_i64()).unwrap_or(0) as i16;
                        let map_flags = m.get("flags").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                        map_vec.push(Map {
                            name,
                            channel,
                            flags: map_flags,
                        });
                    }
                }
                if !map_vec.is_empty() {
                    extra_maps.push((map_type as usize, map_vec));
                }
            }
        }
    }

    (flags, uvw_velocity, extra_maps)
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
            pack_order: "PNA0T0S".to_string(),
            decl_order: "PNA0T0S".to_string(),
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Float4,
            basis_scale_type: VertexElementType::Float2,
            tangent_type: VertexElementType::Float4,
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
                tangent: [1.0, 0.0, 0.0, 1.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [0.0, 0.0];
                    tc
                },
                num_texcoords: 1,
                bone_indices: [1, 0, 0, 0],
                bone_weights: [1.0, 0.0, 0.0, 0.0],
                ..Default::default()
            },
            UnpackedVertex {
                position: [1.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0, 1.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [1.0, 0.0];
                    tc
                },
                num_texcoords: 1,
                bone_indices: [1, 2, 0, 0],
                bone_weights: [0.7, 0.3, 0.0, 0.0],
                ..Default::default()
            },
            UnpackedVertex {
                position: [0.0, 1.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0, -1.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [0.0, 1.0];
                    tc
                },
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
            bounding_sphere: Sphere {
                center: [0.5, 0.5, 0.0],
                radius: 1.0,
            },
            bounds: AABB {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
            materials: Vec::new(),
            bones,
            granny_bones,
            granny_meshes: Vec::new(),
            bone_bounds: vec![
                AABB {
                    min: [0.0, 0.0, 0.0],
                    max: [1.0, 1.0, 0.0],
                },
                AABB {
                    min: [0.0, 0.0, 0.0],
                    max: [1.0, 1.0, 0.0],
                },
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
                bone_remap: Vec::new(),
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
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // Compare vertex positions
        assert_eq!(imported.sections.len(), 1);
        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();
        assert_eq!(imported_verts.len(), orig_verts.len());

        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            for i in 0..3 {
                assert!(
                    (orig.position[i] - imp.position[i]).abs() < 1e-5,
                    "position[{}] mismatch: {} vs {}",
                    i,
                    orig.position[i],
                    imp.position[i]
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
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();

        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            // Normals
            for i in 0..3 {
                assert!(
                    (orig.normal[i] - imp.normal[i]).abs() < 1e-4,
                    "normal[{}] mismatch: {} vs {}",
                    i,
                    orig.normal[i],
                    imp.normal[i]
                );
            }
            // UVs
            for i in 0..2 {
                assert!(
                    (orig.texcoords[0][i] - imp.texcoords[0][i]).abs() < 1e-5,
                    "uv[0][{}] mismatch: {} vs {}",
                    i,
                    orig.texcoords[0][i],
                    imp.texcoords[0][i]
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
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

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
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let imported_verts = imported.unpack_section_vertices(0).unwrap();

        for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
            // Bone indices with non-zero weights should match
            // Indices with zero weights may be padded with valid bone indices (game requirement)
            for k in 0..4 {
                if orig.bone_weights[k] > 0.0 {
                    assert_eq!(
                        imp.bone_indices[k], orig.bone_indices[k],
                        "bone_indices[{}] mismatch for weighted bone: {:?} vs {:?}",
                        k, imp.bone_indices, orig.bone_indices
                    );
                } else {
                    // For zero-weight slots, just verify the index is valid (non-zero for 1-based)
                    // Game expects valid bone indices even for unused slots
                    assert!(
                        imp.bone_indices[k] > 0,
                        "bone_indices[{}] should be valid (>0) for game compatibility, got {}",
                        k, imp.bone_indices[k]
                    );
                }
            }

            // Bone weights should be close (UByte4N has ~1/255 precision)
            for k in 0..4 {
                assert!(
                    (orig.bone_weights[k] - imp.bone_weights[k]).abs() < 0.01,
                    "bone_weight[{}] mismatch: {} vs {}",
                    k,
                    orig.bone_weights[k],
                    imp.bone_weights[k]
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
        assert!(
            export.buffer.is_none(),
            "embedded export should not have separate buffer"
        );

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
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

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
                    "vertex {} position[{}] mismatch: {} vs {}",
                    vi,
                    i,
                    orig.position[i],
                    fin.position[i]
                );
            }

            // Normals
            for i in 0..3 {
                assert!(
                    (orig.normal[i] - fin.normal[i]).abs() < 1e-4,
                    "vertex {} normal[{}] mismatch: {} vs {}",
                    vi,
                    i,
                    orig.normal[i],
                    fin.normal[i]
                );
            }

            // Tangents (only check xyz - game uses Float3 which doesn't store w/handedness)
            for i in 0..3 {
                assert!(
                    (orig.tangent[i] - fin.tangent[i]).abs() < 1e-4,
                    "vertex {} tangent[{}] mismatch: {} vs {}",
                    vi,
                    i,
                    orig.tangent[i],
                    fin.tangent[i]
                );
            }

            // UVs
            for i in 0..orig.num_texcoords {
                for j in 0..2 {
                    assert!(
                        (orig.texcoords[i][j] - fin.texcoords[i][j]).abs() < 1e-4,
                        "vertex {} texcoord[{}][{}] mismatch: {} vs {}",
                        vi,
                        i,
                        j,
                        orig.texcoords[i][j],
                        fin.texcoords[i][j]
                    );
                }
            }

            // Bone indices - only check indices with non-zero weights
            // Indices with zero weights may be padded with valid bone indices (game requirement)
            for i in 0..4 {
                if orig.bone_weights[i] > 0.0 {
                    assert_eq!(
                        orig.bone_indices[i], fin.bone_indices[i],
                        "vertex {} bone_indices[{}] mismatch for weighted bone: {:?} vs {:?}",
                        vi, i, orig.bone_indices, fin.bone_indices
                    );
                } else {
                    // For zero-weight slots, just verify the index is valid (non-zero for 1-based)
                    assert!(
                        fin.bone_indices[i] > 0,
                        "vertex {} bone_indices[{}] should be valid (>0) for game compatibility, got {}",
                        vi, i, fin.bone_indices[i]
                    );
                }
            }

            // Bone weights (UByte4N has ~1/255 precision)
            for i in 0..4 {
                assert!(
                    (orig.bone_weights[i] - fin.bone_weights[i]).abs() < 0.01,
                    "vertex {} bone_weights[{}] mismatch: {} vs {}",
                    vi,
                    i,
                    orig.bone_weights[i],
                    fin.bone_weights[i]
                );
            }
        }

        // Verify indices survived
        let orig_indices = original.get_section_indices(0);
        let final_indices = re_read.get_section_indices(0);
        assert_eq!(final_indices, orig_indices);

        // Verify bone names and hierarchy survived
        assert_eq!(re_read.bones.len(), original.bones.len());
        for (bi, (orig_bone, fin_bone)) in
            original.bones.iter().zip(re_read.bones.iter()).enumerate()
        {
            assert_eq!(
                orig_bone.name, fin_bone.name,
                "bone {} name mismatch: {:?} vs {:?}",
                bi, orig_bone.name, fin_bone.name
            );
            assert_eq!(
                orig_bone.parent_index, fin_bone.parent_index,
                "bone {} parent_index mismatch: {} vs {}",
                bi, orig_bone.parent_index, fin_bone.parent_index
            );
        }

        // Verify granny bones survived the round trip
        assert_eq!(
            re_read.granny_bones.len(),
            original.granny_bones.len(),
            "granny_bones count mismatch: {} vs {}",
            re_read.granny_bones.len(),
            original.granny_bones.len()
        );
        for (bi, (orig_gb, fin_gb)) in original
            .granny_bones
            .iter()
            .zip(re_read.granny_bones.iter())
            .enumerate()
        {
            assert_eq!(
                orig_gb.name, fin_gb.name,
                "granny_bone {} name mismatch: {:?} vs {:?}",
                bi, orig_gb.name, fin_gb.name
            );
            assert_eq!(
                orig_gb.parent_index, fin_gb.parent_index,
                "granny_bone {} parent_index mismatch: {} vs {}",
                bi, orig_gb.parent_index, fin_gb.parent_index
            );
            // Compare inverse world matrices
            for row in 0..4 {
                for col in 0..4 {
                    assert!(
                        (orig_gb.inverse_world_matrix.rows[row][col]
                            - fin_gb.inverse_world_matrix.rows[row][col])
                            .abs()
                            < 1e-4,
                        "granny_bone {} matrix[{}][{}] mismatch: {} vs {}",
                        bi,
                        row,
                        col,
                        orig_gb.inverse_world_matrix.rows[row][col],
                        fin_gb.inverse_world_matrix.rows[row][col]
                    );
                }
            }
        }

        // Verify bounding volumes are reasonable
        assert!(
            re_read.bounding_sphere.radius > 0.0,
            "bounding sphere radius should be positive"
        );
        for i in 0..3 {
            assert!(
                re_read.bounds.max[i] >= re_read.bounds.min[i],
                "AABB max[{}] < min[{}]",
                i,
                i
            );
        }
    }

    /// Test that glTF import generates granny_meshes from vertex skin data.
    /// This is critical for the game to properly skin the model.
    #[test]
    fn test_gltf_import_generates_granny_meshes() {
        let original = make_test_geom();

        // Export to glTF with skeleton
        let export_opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &export_opts).unwrap();

        // Import from glTF
        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // Verify granny_meshes was generated
        assert!(
            !imported.granny_meshes.is_empty(),
            "granny_meshes should be generated from vertex skin data"
        );

        // Should have exactly one mesh for glTF imports
        assert_eq!(
            imported.granny_meshes.len(),
            1,
            "glTF import should create one mesh with all used bones"
        );

        let mesh = &imported.granny_meshes[0];
        // Mesh name is preserved from glTF export (section_0 since test geom has no granny_meshes)
        assert_eq!(mesh.name, "section_0");

        // The test geom uses bones 1 and 2 (1-based), which are "root" and "child"
        // Verify bone bindings contain the bones actually used by vertices
        assert!(
            !mesh.bone_bindings.is_empty(),
            "mesh should have bone bindings"
        );

        // Check that the bone names are from our test skeleton ("root" and "child")
        let valid_bones = ["root", "child"];
        for bone_name in &mesh.bone_bindings {
            assert!(
                valid_bones.contains(&bone_name.as_str()),
                "bone binding '{}' should be from test skeleton (root or child)",
                bone_name
            );
        }

        // Write to UGX and read back to verify it survives serialization
        let ugx_bytes = crate::write_ugx(&imported).unwrap();
        let re_read = UgxGeom::read(&ugx_bytes).unwrap();

        // Verify granny_meshes survived the round trip
        assert_eq!(
            re_read.granny_meshes.len(),
            1,
            "granny_meshes should survive UGX round trip"
        );
        assert_eq!(re_read.granny_meshes[0].name, "section_0");
        assert_eq!(
            re_read.granny_meshes[0].bone_bindings.len(),
            mesh.bone_bindings.len(),
            "bone binding count should survive UGX round trip"
        );
    }

    /// Round-trip a real Halo Wars UGX file: read → export to glTF → import → write UGX → read back.
    /// Compares vertex positions, normals, indices, bone hierarchy, and granny bone data.
    #[test]
    fn test_real_file_roundtrip() {
        let paths = [
            "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx",
            "../../test_ugx/art/covenant/air/spirit_01/spirit_damaged_01.ugx",
            "../../test_ugx/art/covenant/building/barracks_01/barracks_damaged_01.ugx",
        ];

        let mut tested = false;
        for path in &paths {
            let data = match std::fs::read(path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let original = match UgxGeom::read(&data) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Failed to read {}: {}", path, e);
                    continue;
                }
            };

            eprintln!("\n=== Testing: {} ===", path);
            eprintln!(
                "  Sections: {}, Bones: {}, Granny bones: {}",
                original.sections.len(),
                original.bones.len(),
                original.granny_bones.len()
            );
            eprintln!(
                "  Total vertices: {}, Total triangles: {}",
                original.total_vertices(),
                original.total_triangles()
            );

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
            let imported =
                import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

            // 3. Verify imported matches original in structure
            assert_eq!(
                imported.sections.len(),
                original.sections.len(),
                "section count mismatch"
            );

            // Check vertex counts match per section
            for (si, (orig_sec, imp_sec)) in original
                .sections
                .iter()
                .zip(imported.sections.iter())
                .enumerate()
            {
                assert_eq!(
                    imp_sec.num_verts, orig_sec.num_verts,
                    "section {} vertex count mismatch: {} vs {}",
                    si, imp_sec.num_verts, orig_sec.num_verts
                );
                assert_eq!(
                    imp_sec.num_tris, orig_sec.num_tris,
                    "section {} triangle count mismatch: {} vs {}",
                    si, imp_sec.num_tris, orig_sec.num_tris
                );
            }

            // Check vertex data for each section
            for si in 0..original.sections.len() {
                let orig_verts = original.unpack_section_vertices(si).unwrap();
                let imp_verts = imported.unpack_section_vertices(si).unwrap();
                assert_eq!(
                    imp_verts.len(),
                    orig_verts.len(),
                    "section {} unpacked vertex count mismatch",
                    si
                );

                for (vi, (ov, iv)) in orig_verts.iter().zip(imp_verts.iter()).enumerate() {
                    for c in 0..3 {
                        assert!(
                            (ov.position[c] - iv.position[c]).abs() < 0.01,
                            "section {} vertex {} position[{}] mismatch: {} vs {}",
                            si,
                            vi,
                            c,
                            ov.position[c],
                            iv.position[c]
                        );
                    }
                    // Normals (may lose some precision through half-float or dec3n packing in original)
                    let ndot = ov.normal[0] * iv.normal[0]
                        + ov.normal[1] * iv.normal[1]
                        + ov.normal[2] * iv.normal[2];
                    assert!(
                        ndot > 0.9 || (ov.normal == [0.0, 0.0, 0.0]),
                        "section {} vertex {} normal diverged too much: {:?} vs {:?} (dot={})",
                        si,
                        vi,
                        ov.normal,
                        iv.normal,
                        ndot
                    );
                }

                // Check indices
                let orig_idx = original.get_section_indices(si);
                let imp_idx = imported.get_section_indices(si);
                assert_eq!(imp_idx, orig_idx, "section {} index mismatch", si);
            }

            // Check bone hierarchy
            if !original.bones.is_empty() {
                assert_eq!(
                    imported.bones.len(),
                    original.bones.len(),
                    "bone count mismatch"
                );
                for (bi, (ob, ib)) in original.bones.iter().zip(imported.bones.iter()).enumerate() {
                    assert_eq!(ob.name, ib.name, "bone {} name mismatch", bi);
                    assert_eq!(
                        ob.parent_index, ib.parent_index,
                        "bone {} parent mismatch",
                        bi
                    );
                }
            }

            // Check granny bones
            if !original.granny_bones.is_empty() {
                assert_eq!(
                    imported.granny_bones.len(),
                    original.granny_bones.len(),
                    "granny bone count mismatch"
                );
                for (bi, (og, ig)) in original
                    .granny_bones
                    .iter()
                    .zip(imported.granny_bones.iter())
                    .enumerate()
                {
                    assert_eq!(og.name, ig.name, "granny bone {} name mismatch", bi);
                    assert_eq!(
                        og.parent_index, ig.parent_index,
                        "granny bone {} parent mismatch",
                        bi
                    );
                    // Compare inverse world matrices (may have some float precision loss)
                    for r in 0..4 {
                        for c in 0..4 {
                            assert!(
                                (og.inverse_world_matrix.rows[r][c]
                                    - ig.inverse_world_matrix.rows[r][c])
                                    .abs()
                                    < 1e-3,
                                "granny bone {} matrix[{}][{}] mismatch: {} vs {}",
                                bi,
                                r,
                                c,
                                og.inverse_world_matrix.rows[r][c],
                                ig.inverse_world_matrix.rows[r][c]
                            );
                        }
                    }
                }
            }

            // 4. Write to UGX bytes and read back
            let ugx_bytes = crate::write_ugx(&imported).unwrap();
            let re_read = UgxGeom::read(&ugx_bytes).unwrap();

            // 5. Verify write→read preserved the data
            assert_eq!(re_read.sections.len(), imported.sections.len());
            for si in 0..imported.sections.len() {
                let imp_verts = imported.unpack_section_vertices(si).unwrap();
                let rr_verts = re_read.unpack_section_vertices(si).unwrap();
                assert_eq!(
                    rr_verts.len(),
                    imp_verts.len(),
                    "write roundtrip section {} vertex count mismatch",
                    si
                );

                for (vi, (iv, rv)) in imp_verts.iter().zip(rr_verts.iter()).enumerate() {
                    for c in 0..3 {
                        assert!(
                            (iv.position[c] - rv.position[c]).abs() < 1e-4,
                            "write roundtrip section {} vertex {} position[{}] mismatch: {} vs {}",
                            si,
                            vi,
                            c,
                            iv.position[c],
                            rv.position[c]
                        );
                    }
                }

                let imp_idx = imported.get_section_indices(si);
                let rr_idx = re_read.get_section_indices(si);
                assert_eq!(
                    rr_idx, imp_idx,
                    "write roundtrip section {} index mismatch",
                    si
                );
            }

            // Verify granny bones survived write→read
            if !imported.granny_bones.is_empty() {
                assert_eq!(re_read.granny_bones.len(), imported.granny_bones.len());
                for (bi, (ig, rg)) in imported
                    .granny_bones
                    .iter()
                    .zip(re_read.granny_bones.iter())
                    .enumerate()
                {
                    assert_eq!(
                        ig.name, rg.name,
                        "write roundtrip granny bone {} name mismatch",
                        bi
                    );
                    assert_eq!(
                        ig.parent_index, rg.parent_index,
                        "write roundtrip granny bone {} parent mismatch",
                        bi
                    );
                    for r in 0..4 {
                        for c in 0..4 {
                            assert!(
                                (ig.inverse_world_matrix.rows[r][c]
                                    - rg.inverse_world_matrix.rows[r][c])
                                    .abs()
                                    < 1e-4,
                                "write roundtrip granny bone {} matrix[{}][{}] mismatch: {} vs {}",
                                bi,
                                r,
                                c,
                                ig.inverse_world_matrix.rows[r][c],
                                rg.inverse_world_matrix.rows[r][c]
                            );
                        }
                    }
                }
            }

            eprintln!(
                "  PASSED: {} sections, {} bones, {} granny bones round-tripped",
                re_read.sections.len(),
                re_read.bones.len(),
                re_read.granny_bones.len()
            );
            tested = true;
        }

        if !tested {
            eprintln!("No real UGX files found in test_ugx/ - test skipped");
            eprintln!("Place UGX files in test_ugx/ directory to enable real file testing");
        }
    }

    /// Round-trip foxcannon UGX files and export glTF (with and without skeleton) to disk.
    #[test]
    fn test_foxcannon_roundtrip_and_export() {
        let dir = "../../foxcannon01";
        let files = [
            "mesh_barrel_0.ugx",
            "mesh_chassis_front_0.ugx",
            "mesh_foxcannon01.ugx",
            "mesh_turret_0.ugx",
        ];

        let mut tested = false;
        for filename in &files {
            let path = format!("{}/{}", dir, filename);
            let data = match std::fs::read(&path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let original = match UgxGeom::read(&data) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Failed to read {}: {}", path, e);
                    continue;
                }
            };

            let stem = filename.trim_end_matches(".ugx");
            eprintln!("\n=== Foxcannon: {} ===", filename);
            eprintln!(
                "  Sections: {}, Bones: {}, Granny bones: {}",
                original.sections.len(),
                original.bones.len(),
                original.granny_bones.len()
            );
            eprintln!(
                "  Total vertices: {}, Total triangles: {}",
                original.total_vertices(),
                original.total_triangles()
            );

            // Export glTF WITHOUT skeleton
            {
                let opts = GltfExportOptions {
                    embed_buffers: false,
                    include_materials: false,
                    include_skeleton: false,
                };
                let buffer_name = format!("{}_no_bones.bin", stem);
                let export =
                    crate::export_to_gltf_with_buffer_name(&original, &opts, &buffer_name).unwrap();
                let json_path = format!("{}/{}_no_bones.gltf", dir, stem);
                let bin_path = format!("{}/{}", dir, buffer_name);
                std::fs::write(&json_path, &export.json).unwrap();
                if let Some(ref buf) = export.buffer {
                    std::fs::write(&bin_path, buf).unwrap();
                }
                eprintln!("  Exported (no bones): {}", json_path);
            }

            // Export glTF WITH skeleton
            {
                let opts = GltfExportOptions {
                    embed_buffers: false,
                    include_materials: false,
                    include_skeleton: true,
                };
                let buffer_name = format!("{}_with_bones.bin", stem);
                let export =
                    crate::export_to_gltf_with_buffer_name(&original, &opts, &buffer_name).unwrap();
                let json_path = format!("{}/{}_with_bones.gltf", dir, stem);
                let bin_path = format!("{}/{}", dir, buffer_name);
                std::fs::write(&json_path, &export.json).unwrap();
                if let Some(ref buf) = export.buffer {
                    std::fs::write(&bin_path, buf).unwrap();
                }
                eprintln!("  Exported (with bones): {}", json_path);
            }

            // Round-trip: export → import → write UGX → read back
            let export_opts = GltfExportOptions {
                embed_buffers: false,
                include_materials: false,
                include_skeleton: true,
            };
            let export = crate::export_to_gltf(&original, &export_opts).unwrap();

            let import_opts = GltfImportOptions {
                include_skeleton: true,
                include_materials: false,
            };
            let imported =
                import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

            // Verify section structure
            assert_eq!(
                imported.sections.len(),
                original.sections.len(),
                "{}: section count mismatch",
                filename
            );
            for (si, (orig_sec, imp_sec)) in original
                .sections
                .iter()
                .zip(imported.sections.iter())
                .enumerate()
            {
                assert_eq!(
                    imp_sec.num_verts, orig_sec.num_verts,
                    "{}: section {} vertex count mismatch",
                    filename, si
                );
                assert_eq!(
                    imp_sec.num_tris, orig_sec.num_tris,
                    "{}: section {} triangle count mismatch",
                    filename, si
                );
            }

            // Verify vertex data per section
            for si in 0..original.sections.len() {
                let orig_verts = original.unpack_section_vertices(si).unwrap();
                let imp_verts = imported.unpack_section_vertices(si).unwrap();
                for (vi, (ov, iv)) in orig_verts.iter().zip(imp_verts.iter()).enumerate() {
                    for c in 0..3 {
                        assert!(
                            (ov.position[c] - iv.position[c]).abs() < 0.01,
                            "{}: section {} vertex {} position[{}] mismatch: {} vs {}",
                            filename,
                            si,
                            vi,
                            c,
                            ov.position[c],
                            iv.position[c]
                        );
                    }
                }

                let orig_idx = original.get_section_indices(si);
                let imp_idx = imported.get_section_indices(si);
                assert_eq!(
                    imp_idx, orig_idx,
                    "{}: section {} index mismatch",
                    filename, si
                );
            }

            // Verify bone hierarchy
            if !original.bones.is_empty() {
                assert_eq!(
                    imported.bones.len(),
                    original.bones.len(),
                    "{}: bone count mismatch",
                    filename
                );
                for (bi, (ob, ib)) in original.bones.iter().zip(imported.bones.iter()).enumerate() {
                    assert_eq!(ob.name, ib.name, "{}: bone {} name mismatch", filename, bi);
                    assert_eq!(
                        ob.parent_index, ib.parent_index,
                        "{}: bone {} parent mismatch",
                        filename, bi
                    );
                }
            }

            // Write to UGX and read back
            let ugx_bytes = crate::write_ugx(&imported).unwrap();
            let re_read = UgxGeom::read(&ugx_bytes).unwrap();

            assert_eq!(
                re_read.sections.len(),
                imported.sections.len(),
                "{}: write roundtrip section count mismatch",
                filename
            );
            for si in 0..imported.sections.len() {
                let imp_verts = imported.unpack_section_vertices(si).unwrap();
                let rr_verts = re_read.unpack_section_vertices(si).unwrap();
                assert_eq!(
                    rr_verts.len(),
                    imp_verts.len(),
                    "{}: write roundtrip section {} vertex count mismatch",
                    filename,
                    si
                );
                for (vi, (iv, rv)) in imp_verts.iter().zip(rr_verts.iter()).enumerate() {
                    for c in 0..3 {
                        assert!(
                            (iv.position[c] - rv.position[c]).abs() < 1e-4,
                            "{}: write roundtrip section {} vertex {} position[{}]: {} vs {}",
                            filename,
                            si,
                            vi,
                            c,
                            iv.position[c],
                            rv.position[c]
                        );
                    }
                }
            }

            eprintln!("  PASSED round-trip");
            tested = true;
        }

        if !tested {
            eprintln!("No foxcannon UGX files found in foxcannon01/ - test skipped");
        }
    }

    #[test]
    fn test_material_texture_roundtrip() {
        use crate::gltf_export::{export_to_gltf, GltfExportOptions};

        // Build a UGX with materials that have texture maps
        let mut geom = make_test_geom();
        geom.materials = vec![
            Material {
                name: "mat_diffuse_normal".into(),
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize].push(Map {
                        name: r"\textures\diffuse_01".into(),
                        channel: 0,
                        flags: 0,
                    });
                    maps[MapType::Normal as usize].push(Map {
                        name: r"\textures\normal_01".into(),
                        channel: 0,
                        flags: 0,
                    });
                    maps
                },
                spec_power: 50.0,
                opacity: 1.0,
                blend_type: 0,
                ..Default::default()
            },
            Material {
                name: "mat_emissive_ao".into(),
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Emissive as usize].push(Map {
                        name: r"\textures\emissive_01".into(),
                        channel: 1,
                        flags: 0,
                    });
                    maps[MapType::AO as usize].push(Map {
                        name: r"\textures\ao_01".into(),
                        channel: 0,
                        flags: 0,
                    });
                    maps
                },
                spec_power: 10.0,
                opacity: 0.8,
                blend_type: 1,
                ..Default::default()
            },
        ];

        // Export to glTF with materials
        let export_opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: true,
            include_skeleton: false,
        };
        let exported = export_to_gltf(&geom, &export_opts).unwrap();

        // Import back
        let import_opts = GltfImportOptions {
            include_skeleton: false,
            include_materials: true,
        };
        let imported =
            import_from_gltf(&exported.json, exported.buffer.as_deref(), &import_opts).unwrap();

        assert_eq!(imported.materials.len(), 2);

        // First material: diffuse + normal, opaque
        let m0 = &imported.materials[0];
        assert_eq!(m0.name, "mat_diffuse_normal");
        assert_eq!(m0.maps[MapType::Diffuse as usize].len(), 1);
        assert_eq!(
            m0.maps[MapType::Diffuse as usize][0].name,
            r"\textures\diffuse_01"
        );
        assert_eq!(m0.maps[MapType::Diffuse as usize][0].channel, 0);
        assert_eq!(m0.maps[MapType::Normal as usize].len(), 1);
        assert_eq!(
            m0.maps[MapType::Normal as usize][0].name,
            r"\textures\normal_01"
        );
        assert_eq!(m0.blend_type, 0);

        // Second material: emissive + AO, blend
        let m1 = &imported.materials[1];
        assert_eq!(m1.name, "mat_emissive_ao");
        assert_eq!(m1.maps[MapType::Emissive as usize].len(), 1);
        assert_eq!(
            m1.maps[MapType::Emissive as usize][0].name,
            r"\textures\emissive_01"
        );
        assert_eq!(m1.maps[MapType::Emissive as usize][0].channel, 1);
        assert_eq!(m1.maps[MapType::AO as usize].len(), 1);
        assert_eq!(m1.maps[MapType::AO as usize][0].name, r"\textures\ao_01");
        assert_eq!(m1.blend_type, 1);
        assert!((m1.opacity - 0.8).abs() < 0.01);
    }

    /// Test that round-tripped vertex formats match game expectations.
    /// The game expects compact vertex types: HalfFloat4 for position, UByte4N for weights,
    /// HalfFloat2 for UVs, and pack_order with skin before texcoords (PNA0ST0).
    #[test]
    fn test_gltf_import_uses_game_vertex_formats() {
        let original = make_test_geom();

        // Export to glTF with skeleton (to test skinned mesh format)
        let opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &opts).unwrap();

        // Import back
        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        assert_eq!(imported.sections.len(), 1);
        let section = &imported.sections[0];
        let packer = &section.base_vert_packer;

        // Verify game-compatible vertex element types
        assert_eq!(
            packer.pos_type,
            VertexElementType::HalfFloat4,
            "Position should use HalfFloat4 (8 bytes) for game compatibility"
        );
        assert_eq!(
            packer.weights_type,
            VertexElementType::UByte4N,
            "Weights should use UByte4N (4 bytes) for game compatibility"
        );
        assert_eq!(
            packer.uv_types[0],
            VertexElementType::HalfFloat2,
            "UVs should use HalfFloat2 (4 bytes) for game compatibility"
        );
        assert_eq!(
            packer.tangent_type,
            VertexElementType::Float3,
            "Tangent should use Float3 (12 bytes) for game compatibility"
        );

        // Verify pack_order has skin before texcoords (game format: PNA0ST0)
        let pack_order = &packer.pack_order;
        let s_pos = pack_order.find('S');
        let t_pos = pack_order.find('T');
        assert!(
            s_pos.is_some() && t_pos.is_some(),
            "Pack order should contain both S and T: {}",
            pack_order
        );
        assert!(
            s_pos.unwrap() < t_pos.unwrap(),
            "Skin (S) should come before texcoords (T) in pack_order: {}",
            pack_order
        );

        // Verify vertex size is compact (should be ~44 bytes for skinned mesh with tangent)
        // HalfFloat4(8) + Float3(12) + Float3(12) + UByte4(4) + UByte4N(4) + HalfFloat2(4) = 44
        let expected_size = 8 + 12 + 12 + 4 + 4 + 4; // 44 bytes
        assert_eq!(
            packer.vertex_size(),
            expected_size,
            "Vertex size should be {} bytes for game-compatible format, got {}",
            expected_size,
            packer.vertex_size()
        );
    }

    /// Test that rigid (non-skinned) meshes also use game-compatible formats.
    #[test]
    fn test_gltf_import_rigid_mesh_uses_game_formats() {
        // Create a rigid mesh (no skeleton)
        let mut uv_types = [VertexElementType::Ignore; MAX_UV];
        uv_types[0] = VertexElementType::Float2;

        let packer = UnivertPacker {
            pack_order: "PNA0T0".to_string(),
            decl_order: "PNA0T0".to_string(),
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Float4,
            basis_scale_type: VertexElementType::Float2,
            tangent_type: VertexElementType::Float4,
            normal_type: VertexElementType::Float3,
            uv_types,
            indices_type: VertexElementType::Ignore,
            weights_type: VertexElementType::Ignore,
            diffuse_type: VertexElementType::Ignore,
            index_type: VertexElementType::Ignore,
        };

        let vertices = vec![
            UnpackedVertex {
                position: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0, 1.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [0.0, 0.0];
                    tc
                },
                num_texcoords: 1,
                ..Default::default()
            },
            UnpackedVertex {
                position: [1.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0, 1.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [1.0, 0.0];
                    tc
                },
                num_texcoords: 1,
                ..Default::default()
            },
            UnpackedVertex {
                position: [0.0, 1.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0, -1.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [0.0, 1.0];
                    tc
                },
                num_texcoords: 1,
                ..Default::default()
            },
        ];

        let mut vertex_buffer = Vec::new();
        for v in &vertices {
            packer.pack_vertex(&mut vertex_buffer, v).unwrap();
        }

        let vert_size = packer.vertex_size() as i32;
        let vb_bytes = vertex_buffer.len() as i32;

        let geom = UgxGeom {
            bounding_sphere: Sphere {
                center: [0.5, 0.5, 0.0],
                radius: 1.0,
            },
            bounds: AABB {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
            materials: Vec::new(),
            bones: Vec::new(),
            granny_bones: Vec::new(),
            granny_meshes: Vec::new(),
            bone_bounds: Vec::new(),
            sections: vec![Section {
                material_index: -1,
                accessory_index: -1,
                max_bones: 0,
                rigid_bone_index: -1,
                ib_offset: 0,
                num_tris: 1,
                vb_offset: 0,
                vb_bytes,
                vert_size,
                num_verts: 3,
                base_vert_packer: packer,
                bone_remap: Vec::new(),
                rigid_only: true,
                global_bones: false,
            }],
            vertex_buffer,
            index_buffer: vec![0, 1, 2],
            rigid_only: true,
            rigid_bone_index: -1,
            all_sections_rigid: true,
            all_sections_skinned: false,
            global_bones: false,
        };

        // Export to glTF
        let opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: false,
        };
        let export = export_to_gltf(&geom, &opts).unwrap();

        // Import back
        let import_opts = GltfImportOptions {
            include_skeleton: false,
            include_materials: false,
        };
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        assert_eq!(imported.sections.len(), 1);
        let section = &imported.sections[0];
        let imported_packer = &section.base_vert_packer;

        // Verify game-compatible vertex element types for rigid mesh
        assert_eq!(
            imported_packer.pos_type,
            VertexElementType::HalfFloat4,
            "Position should use HalfFloat4 for game compatibility"
        );
        assert_eq!(
            imported_packer.uv_types[0],
            VertexElementType::HalfFloat2,
            "UVs should use HalfFloat2 for game compatibility"
        );

        // Rigid mesh should not have skin data
        assert!(
            !imported_packer.pack_order.contains('S'),
            "Rigid mesh pack_order should not contain S: {}",
            imported_packer.pack_order
        );

        // Verify vertex size is compact for rigid mesh
        // HalfFloat4(8) + Float3(12) + Float3(12) + HalfFloat2(4) = 36 bytes
        let expected_size = 8 + 12 + 12 + 4; // 36 bytes
        assert_eq!(
            imported_packer.vertex_size(),
            expected_size,
            "Rigid vertex size should be {} bytes, got {}",
            expected_size,
            imported_packer.vertex_size()
        );
    }
}
