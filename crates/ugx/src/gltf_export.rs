//! glTF export for UGX models.
//!
//! Converts UGX geometry to glTF 2.0 format with skeleton support.

use base64::{engine::general_purpose::STANDARD, Engine};
use gltf_json as json;
use json::validation::Checked::Valid;

use crate::error::Result;
use crate::types::Bone;
use crate::ugx::UgxGeom;
use crate::univert_packer::UnpackedVertex;

/// glTF export options.
#[derive(Debug, Clone, Default)]
pub struct GltfExportOptions {
    /// Embed buffer data as base64 in the .gltf file (default: true).
    pub embed_buffers: bool,
    /// Include materials (default: true).
    pub include_materials: bool,
    /// Include skeleton/bones (default: true).
    pub include_skeleton: bool,
}

impl GltfExportOptions {
    pub fn new() -> Self {
        Self {
            embed_buffers: true,
            include_materials: true,
            include_skeleton: true,
        }
    }
}

/// Export result containing the glTF JSON and optional binary buffer.
#[derive(Debug)]
pub struct GltfExport {
    /// The glTF JSON document.
    pub json: String,
    /// Binary buffer data (if not embedded).
    pub buffer: Option<Vec<u8>>,
}

/// Export UGX geometry to glTF format.
pub fn export_to_gltf(geom: &UgxGeom, options: &GltfExportOptions) -> Result<GltfExport> {
    let mut root = json::Root::default();

    // Build the binary buffer containing all vertex and index data
    let mut buffer_data = Vec::new();
    let mut accessors = Vec::new();
    let mut buffer_views = Vec::new();
    let mut meshes = Vec::new();
    let mut materials_json = Vec::new();

    // Create materials if requested
    if options.include_materials {
        for mat in &geom.materials {
            let pbr = json::material::PbrMetallicRoughness {
                base_color_factor: json::material::PbrBaseColorFactor([
                    mat.diff_color[0],
                    mat.diff_color[1],
                    mat.diff_color[2],
                    1.0,
                ]),
                base_color_texture: None,
                metallic_factor: json::material::StrengthFactor(0.0),
                roughness_factor: json::material::StrengthFactor(
                    1.0 - (mat.spec_power / 100.0).clamp(0.0, 1.0),
                ),
                metallic_roughness_texture: None,
                extensions: None,
                extras: json::Extras::default(),
            };

            materials_json.push(json::Material {
                alpha_cutoff: None,
                alpha_mode: Valid(json::material::AlphaMode::Opaque),
                double_sided: false,
                pbr_metallic_roughness: pbr,
                normal_texture: None,
                occlusion_texture: None,
                emissive_texture: None,
                emissive_factor: json::material::EmissiveFactor([0.0, 0.0, 0.0]),
                extensions: None,
                extras: json::Extras::default(),
            });
        }
    }

    // Check if we have bones and should include skeleton
    let has_skeleton = options.include_skeleton && !geom.bones.is_empty();

    // Process each section as a mesh primitive
    for (section_idx, section) in geom.sections.iter().enumerate() {
        let vertices = geom.unpack_section_vertices(section_idx)?;
        let indices = geom.get_section_indices(section_idx);

        if vertices.is_empty() || indices.is_empty() {
            continue;
        }

        let primitive = create_primitive(
            &vertices,
            &indices,
            section.material_index,
            &mut buffer_data,
            &mut accessors,
            &mut buffer_views,
            options.include_materials && !materials_json.is_empty(),
            has_skeleton,
        );

        meshes.push(json::Mesh {
            extensions: None,
            extras: json::Extras::default(),
            primitives: vec![primitive],
            weights: None,
        });
    }

    // Create the buffer
    let buffer_length = buffer_data.len() as u64;
    let buffer = if options.embed_buffers {
        let encoded = STANDARD.encode(&buffer_data);
        json::Buffer {
            byte_length: json::validation::USize64(buffer_length),
            uri: Some(format!("data:application/octet-stream;base64,{}", encoded)),
            extensions: None,
            extras: json::Extras::default(),
        }
    } else {
        json::Buffer {
            byte_length: json::validation::USize64(buffer_length),
            uri: Some("model.bin".to_string()),
            extensions: None,
            extras: json::Extras::default(),
        }
    };

    // Create skeleton nodes and skin if we have bones
    let mut nodes = Vec::new();
    let mut skins = Vec::new();
    let skin_index: Option<json::Index<json::Skin>>;

    if has_skeleton {
        // Create bone nodes first (they come before mesh nodes)
        let bone_node_start = 0u32;
        let (bone_nodes, ibm_accessor_idx) = create_skeleton_nodes(
            &geom.bones,
            &mut buffer_data,
            &mut accessors,
            &mut buffer_views,
        );
        nodes.extend(bone_nodes);

        // Find root bones (bones with parent_index == -1)
        let root_bones: Vec<u32> = geom
            .bones
            .iter()
            .enumerate()
            .filter(|(_, b)| b.parent_index < 0)
            .map(|(i, _)| bone_node_start + i as u32)
            .collect();

        // Create skin
        let joint_indices: Vec<json::Index<json::Node>> = (0..geom.bones.len() as u32)
            .map(|i| json::Index::new(bone_node_start + i))
            .collect();

        skins.push(json::Skin {
            extensions: None,
            extras: json::Extras::default(),
            inverse_bind_matrices: Some(json::Index::new(ibm_accessor_idx)),
            joints: joint_indices,
            skeleton: root_bones.first().map(|&i| json::Index::new(i)),
        });
        skin_index = Some(json::Index::new(0));

        // Create mesh nodes (after bone nodes)
        for (i, _mesh) in meshes.iter().enumerate() {
            nodes.push(json::Node {
                camera: None,
                children: None,
                extensions: None,
                extras: json::Extras::default(),
                matrix: None,
                mesh: Some(json::Index::new(i as u32)),
                rotation: None,
                scale: None,
                translation: None,
                skin: skin_index.clone(),
                weights: None,
            });
        }
    } else {
        skin_index = None;
        // No skeleton - just create mesh nodes
        for (i, _mesh) in meshes.iter().enumerate() {
            nodes.push(json::Node {
                camera: None,
                children: None,
                extensions: None,
                extras: json::Extras::default(),
                matrix: None,
                mesh: Some(json::Index::new(i as u32)),
                rotation: None,
                scale: None,
                translation: None,
                skin: None,
                weights: None,
            });
        }
    }

    // Scene includes all nodes
    let node_indices: Vec<_> = (0..nodes.len() as u32).map(json::Index::new).collect();

    let scene = json::Scene {
        extensions: None,
        extras: json::Extras::default(),
        nodes: node_indices,
    };

    // Assemble the root
    root.accessors = accessors;
    root.buffers = vec![buffer];
    root.buffer_views = buffer_views;
    root.meshes = meshes;
    root.nodes = nodes;
    root.scenes = vec![scene];
    root.scene = Some(json::Index::new(0));

    if !skins.is_empty() {
        root.skins = skins;
    }

    if options.include_materials && !materials_json.is_empty() {
        root.materials = materials_json;
    }

    // Set asset info
    root.asset = json::Asset {
        copyright: None,
        extensions: None,
        extras: json::Extras::default(),
        generator: Some("ugx-rs".to_string()),
        min_version: None,
        version: "2.0".to_string(),
    };

    let json_string = serde_json::to_string_pretty(&root)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(GltfExport {
        json: json_string,
        buffer: if options.embed_buffers {
            None
        } else {
            Some(buffer_data)
        },
    })
}

/// Create a mesh primitive from vertices and indices.
fn create_primitive(
    vertices: &[UnpackedVertex],
    indices: &[u16],
    material_index: i32,
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
    has_materials: bool,
    has_skeleton: bool,
) -> json::mesh::Primitive {
    let mut attributes = std::collections::BTreeMap::new();

    // Calculate bounds for position accessor
    let mut min_pos = [f32::MAX; 3];
    let mut max_pos = [f32::MIN; 3];
    for v in vertices {
        for i in 0..3 {
            min_pos[i] = min_pos[i].min(v.position[i]);
            max_pos[i] = max_pos[i].max(v.position[i]);
        }
    }

    // Write positions
    let pos_view_idx = buffer_views.len() as u32;
    let pos_offset = buffer_data.len();
    for v in vertices {
        buffer_data.extend_from_slice(&v.position[0].to_le_bytes());
        buffer_data.extend_from_slice(&v.position[1].to_le_bytes());
        buffer_data.extend_from_slice(&v.position[2].to_le_bytes());
    }
    let pos_byte_length = buffer_data.len() - pos_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(pos_byte_length as u64),
        byte_offset: Some(json::validation::USize64(pos_offset as u64)),
        byte_stride: Some(json::buffer::Stride(12)),
        extensions: None,
        extras: json::Extras::default(),
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
    });

    let pos_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(pos_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(vertices.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Vec3),
        min: Some(json::Value::from(min_pos.to_vec())),
        max: Some(json::Value::from(max_pos.to_vec())),
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Valid(json::mesh::Semantic::Positions),
        json::Index::new(pos_accessor_idx),
    );

    // Write normals
    let norm_view_idx = buffer_views.len() as u32;
    let norm_offset = buffer_data.len();
    for v in vertices {
        buffer_data.extend_from_slice(&v.normal[0].to_le_bytes());
        buffer_data.extend_from_slice(&v.normal[1].to_le_bytes());
        buffer_data.extend_from_slice(&v.normal[2].to_le_bytes());
    }
    let norm_byte_length = buffer_data.len() - norm_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(norm_byte_length as u64),
        byte_offset: Some(json::validation::USize64(norm_offset as u64)),
        byte_stride: Some(json::buffer::Stride(12)),
        extensions: None,
        extras: json::Extras::default(),
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
    });

    let norm_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(norm_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(vertices.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Vec3),
        min: None,
        max: None,
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Valid(json::mesh::Semantic::Normals),
        json::Index::new(norm_accessor_idx),
    );

    // Write UVs (first set only)
    if vertices.iter().any(|v| v.num_texcoords > 0) {
        let uv_view_idx = buffer_views.len() as u32;
        let uv_offset = buffer_data.len();
        for v in vertices {
            // Flip V coordinate (glTF uses top-left origin)
            buffer_data.extend_from_slice(&v.texcoords[0][0].to_le_bytes());
            buffer_data.extend_from_slice(&(1.0 - v.texcoords[0][1]).to_le_bytes());
        }
        let uv_byte_length = buffer_data.len() - uv_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(uv_byte_length as u64),
            byte_offset: Some(json::validation::USize64(uv_offset as u64)),
            byte_stride: Some(json::buffer::Stride(8)),
            extensions: None,
            extras: json::Extras::default(),
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let uv_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(uv_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec2),
            min: None,
            max: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::TexCoords(0)),
            json::Index::new(uv_accessor_idx),
        );
    }

    // Write bone indices and weights if we have a skeleton
    if has_skeleton {
        // JOINTS_0 - bone indices as unsigned bytes
        let joints_view_idx = buffer_views.len() as u32;
        let joints_offset = buffer_data.len();
        for v in vertices {
            buffer_data.extend_from_slice(&v.bone_indices);
        }
        let joints_byte_length = buffer_data.len() - joints_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(joints_byte_length as u64),
            byte_offset: Some(json::validation::USize64(joints_offset as u64)),
            byte_stride: Some(json::buffer::Stride(4)),
            extensions: None,
            extras: json::Extras::default(),
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let joints_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(joints_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::U8,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Joints(0)),
            json::Index::new(joints_accessor_idx),
        );

        // WEIGHTS_0 - bone weights as floats
        let weights_view_idx = buffer_views.len() as u32;
        let weights_offset = buffer_data.len();
        for v in vertices {
            buffer_data.extend_from_slice(&v.bone_weights[0].to_le_bytes());
            buffer_data.extend_from_slice(&v.bone_weights[1].to_le_bytes());
            buffer_data.extend_from_slice(&v.bone_weights[2].to_le_bytes());
            buffer_data.extend_from_slice(&v.bone_weights[3].to_le_bytes());
        }
        let weights_byte_length = buffer_data.len() - weights_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(weights_byte_length as u64),
            byte_offset: Some(json::validation::USize64(weights_offset as u64)),
            byte_stride: Some(json::buffer::Stride(16)),
            extensions: None,
            extras: json::Extras::default(),
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let weights_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(weights_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Weights(0)),
            json::Index::new(weights_accessor_idx),
        );
    }

    // Write indices
    let idx_view_idx = buffer_views.len() as u32;
    let idx_offset = buffer_data.len();
    for idx in indices {
        buffer_data.extend_from_slice(&idx.to_le_bytes());
    }
    let idx_byte_length = buffer_data.len() - idx_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(idx_byte_length as u64),
        byte_offset: Some(json::validation::USize64(idx_offset as u64)),
        byte_stride: None,
        extensions: None,
        extras: json::Extras::default(),
        target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
    });

    let idx_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(idx_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(indices.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::U16,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Scalar),
        min: None,
        max: None,
        normalized: false,
        sparse: None,
    });

    let material = if has_materials && material_index >= 0 {
        Some(json::Index::new(material_index as u32))
    } else {
        None
    };

    json::mesh::Primitive {
        attributes,
        extensions: None,
        extras: json::Extras::default(),
        indices: Some(json::Index::new(idx_accessor_idx)),
        material,
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
    }
}

/// Create skeleton nodes and inverse bind matrices accessor.
/// Returns (bone_nodes, inverse_bind_matrices_accessor_index).
fn create_skeleton_nodes(
    bones: &[Bone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> (Vec<json::Node>, u32) {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for each bone
    let mut children_map: std::collections::HashMap<i32, Vec<u32>> =
        std::collections::HashMap::new();
    for (i, bone) in bones.iter().enumerate() {
        if bone.parent_index >= 0 {
            children_map
                .entry(bone.parent_index)
                .or_default()
                .push(i as u32);
        }
    }

    // Create nodes for each bone
    for (i, bone) in bones.iter().enumerate() {
        let children = children_map.get(&(i as i32)).map(|c| {
            c.iter()
                .map(|&idx| json::Index::new(idx))
                .collect::<Vec<_>>()
        });

        // Extract translation from the model-to-bone matrix
        // The matrix is model-to-bone, but for glTF we need bone-to-model (local transform)
        // For now, just use translation from the matrix
        let translation = bone.model_to_bone.translation();

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().map_or(true, |c| c.is_empty()) {
                None
            } else {
                children
            },
            extensions: None,
            extras: json::Extras::default(),
            matrix: None,
            mesh: None,
            rotation: None,
            scale: None,
            translation: Some(translation),
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        // The model_to_bone matrix IS the inverse bind matrix
        // glTF expects column-major, but we store row-major, so transpose
        let m = &bone.model_to_bone.rows;
        // Write as column-major (transpose)
        for col in 0..4 {
            for row in 0..4 {
                buffer_data.extend_from_slice(&m[row][col].to_le_bytes());
            }
        }
    }

    let ibm_byte_length = buffer_data.len() - ibm_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(ibm_byte_length as u64),
        byte_offset: Some(json::validation::USize64(ibm_offset as u64)),
        byte_stride: None,
        extensions: None,
        extras: json::Extras::default(),
        target: None,
    });

    let ibm_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(ibm_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(bones.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Mat4),
        min: None,
        max: None,
        normalized: false,
        sparse: None,
    });

    (nodes, ibm_accessor_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_options_default() {
        let opts = GltfExportOptions::new();
        assert!(opts.embed_buffers);
        assert!(opts.include_materials);
        assert!(opts.include_skeleton);
    }
}
