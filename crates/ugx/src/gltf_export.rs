//! glTF export for UGX models.
//!
//! Converts UGX geometry to glTF 2.0 format with skeleton support.

use base64::{engine::general_purpose::STANDARD, Engine};
use gltf_json as json;
use json::validation::Checked::Valid;

use crate::error::Result;
use crate::types::Bone;
use crate::ugx::{GrannyBone, UgxGeom};
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
    // Prefer granny_bones (from 0x703 chunk) as they have the correct inverse world matrices
    let use_granny_bones = options.include_skeleton && !geom.granny_bones.is_empty();
    let has_skeleton = options.include_skeleton && (!geom.bones.is_empty() || use_granny_bones);
    let bone_count = if use_granny_bones {
        geom.granny_bones.len()
    } else {
        geom.bones.len()
    };

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
            bone_count,
        );

        meshes.push(json::Mesh {
            extensions: None,
            extras: json::Extras::default(),
            primitives: vec![primitive],
            weights: None,
        });
    }

    // Create skeleton nodes and skin if we have bones (must happen BEFORE buffer creation)
    let mut nodes = Vec::new();
    let mut skins = Vec::new();
    let skin_index: Option<json::Index<json::Skin>>;

    if has_skeleton {
        // Create bone nodes first (they come before mesh nodes)
        let bone_node_start = 0u32;
        let (bone_nodes, ibm_accessor_idx) = if use_granny_bones {
            // Use granny bones with correct inverse world matrices (same as Python script)
            create_skeleton_nodes_from_granny(
                &geom.granny_bones,
                &mut buffer_data,
                &mut accessors,
                &mut buffer_views,
            )
        } else {
            // Fallback to cached data bones
            create_skeleton_nodes(
                &geom.bones,
                &mut buffer_data,
                &mut accessors,
                &mut buffer_views,
            )
        };
        nodes.extend(bone_nodes);

        // Find root bones (bones with parent_index == -1)
        let root_bones: Vec<u32> = if use_granny_bones {
            geom.granny_bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.parent_index < 0)
                .map(|(i, _)| bone_node_start + i as u32)
                .collect()
        } else {
            geom.bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.parent_index < 0)
                .map(|(i, _)| bone_node_start + i as u32)
                .collect()
        };

        // Create skin
        let joint_indices: Vec<json::Index<json::Node>> = (0..bone_count as u32)
            .map(|i| json::Index::new(bone_node_start + i))
            .collect();

        // V19: With flat bone structure (no hierarchy), skeleton should be None
        // since there's no common ancestor for all joints
        skins.push(json::Skin {
            extensions: None,
            extras: json::Extras::default(),
            inverse_bind_matrices: Some(json::Index::new(ibm_accessor_idx)),
            joints: joint_indices,
            skeleton: None, // No hierarchy = no common root
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

    // V19: With flat bone structure (no children hierarchy), ALL bone nodes must be in the scene
    // This fixes the NODE_SKIN_NO_SCENE validation error from v17
    let mut scene_node_indices = Vec::new();
    if has_skeleton {
        // Add ALL bone nodes to scene (since we're using flat structure with world transforms)
        for i in 0..bone_count as u32 {
            scene_node_indices.push(json::Index::new(i));
        }
        // Add mesh nodes (they come after bone nodes)
        let mesh_node_start = bone_count as u32;
        for i in 0..meshes.len() as u32 {
            scene_node_indices.push(json::Index::new(mesh_node_start + i));
        }
    } else {
        // No skeleton - all nodes are mesh nodes and are roots
        scene_node_indices = (0..nodes.len() as u32).map(json::Index::new).collect();
    }

    let scene = json::Scene {
        extensions: None,
        extras: json::Extras::default(),
        nodes: scene_node_indices,
    };

    // Create the buffer (AFTER all data has been written, including skeleton data)
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
    bone_count: usize,
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
    // Pad to 4-byte boundary for float alignment
    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }
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
    if has_skeleton && bone_count > 0 {
        let max_bone_idx = (bone_count - 1) as u8;

        // JOINTS_0 - bone indices as unsigned bytes
        // Clamp to valid range and fix vertices with no skinning
        let joints_view_idx = buffer_views.len() as u32;
        let joints_offset = buffer_data.len();
        for v in vertices {
            // Clamp bone indices to valid range
            let mut indices = v.bone_indices;
            for idx in &mut indices {
                if *idx > max_bone_idx {
                    *idx = 0; // Bind to root bone if out of range
                }
            }
            buffer_data.extend_from_slice(&indices);
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
        // Ensure weights sum to 1.0 (normalize or set default if all zero)
        // Pad to 4-byte boundary for float alignment
        while buffer_data.len() % 4 != 0 {
            buffer_data.push(0);
        }
        let weights_view_idx = buffer_views.len() as u32;
        let weights_offset = buffer_data.len();
        for v in vertices {
            let mut weights = v.bone_weights;
            let sum: f32 = weights.iter().sum();
            if sum == 0.0 {
                // No skinning data - bind 100% to first joint
                weights[0] = 1.0;
            } else if (sum - 1.0).abs() > 0.001 {
                // Normalize weights to sum to 1.0
                for w in &mut weights {
                    *w /= sum;
                }
            }
            buffer_data.extend_from_slice(&weights[0].to_le_bytes());
            buffer_data.extend_from_slice(&weights[1].to_le_bytes());
            buffer_data.extend_from_slice(&weights[2].to_le_bytes());
            buffer_data.extend_from_slice(&weights[3].to_le_bytes());
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
    // Pad to 2-byte boundary for u16 alignment
    if buffer_data.len() % 2 != 0 {
        buffer_data.push(0);
    }
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

    // Following the Blender importer approach:
    // 1. Invert the model_to_bone matrix to get bone_to_model (world transform)
    // 2. Transpose the inverted matrix
    // 3. Use this as the bone's matrix
    //
    // From the Blender script:
    //   invWorldMat = Matrix(...)  # model_to_bone
    //   invWorldMat.invert()       # -> bone_to_model
    //   invWorldMat.transpose()    # transpose for Blender
    //   bpyBone.matrix = invWorldMat

    // Compute world transforms for all bones by inverting model_to_bone
    // model_to_bone transforms from model space to bone space
    // Inverting gives bone_to_model (bone's world transform)
    let bone_world_matrices: Vec<_> = bones
        .iter()
        .enumerate()
        .map(|(i, b)| {
            // Debug: print raw matrix for first few bones
            if i < 3 {
                eprintln!(
                    "Bone [{}] {} raw model_to_bone:\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]",
                    i, b.name,
                    b.model_to_bone.rows[0][0], b.model_to_bone.rows[0][1], b.model_to_bone.rows[0][2], b.model_to_bone.rows[0][3],
                    b.model_to_bone.rows[1][0], b.model_to_bone.rows[1][1], b.model_to_bone.rows[1][2], b.model_to_bone.rows[1][3],
                    b.model_to_bone.rows[2][0], b.model_to_bone.rows[2][1], b.model_to_bone.rows[2][2], b.model_to_bone.rows[2][3],
                    b.model_to_bone.rows[3][0], b.model_to_bone.rows[3][1], b.model_to_bone.rows[3][2], b.model_to_bone.rows[3][3],
                );
            }
            // Just invert (no transpose) - translation stays in row 3
            let world_mat = b
                .model_to_bone
                .inverse()
                .unwrap_or_else(crate::types::Matrix4x4::identity);
            // Debug: print inverted matrix for first few bones
            if i < 3 {
                eprintln!(
                    "Bone [{}] {} after invert (world pos):\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]",
                    i, b.name,
                    world_mat.rows[0][0], world_mat.rows[0][1], world_mat.rows[0][2], world_mat.rows[0][3],
                    world_mat.rows[1][0], world_mat.rows[1][1], world_mat.rows[1][2], world_mat.rows[1][3],
                    world_mat.rows[2][0], world_mat.rows[2][1], world_mat.rows[2][2], world_mat.rows[2][3],
                    world_mat.rows[3][0], world_mat.rows[3][1], world_mat.rows[3][2], world_mat.rows[3][3],
                );
            }
            world_mat
        })
        .collect();

    // Compute local transforms for each bone
    // For root bones: local = world
    // For child bones: local = parent_world^-1 * world
    for (i, bone) in bones.iter().enumerate() {
        let children = children_map.get(&(i as i32)).map(|c| {
            c.iter()
                .map(|&idx| json::Index::new(idx))
                .collect::<Vec<_>>()
        });

        let local_matrix = if bone.parent_index < 0 {
            // Root bone - use world matrix directly
            bone_world_matrices[i].clone()
        } else {
            // Child bone - compute local: parent_world^-1 * world
            let parent_idx = bone.parent_index as usize;
            let parent_inv = bone_world_matrices[parent_idx]
                .inverse()
                .unwrap_or_else(crate::types::Matrix4x4::identity);
            parent_inv.multiply(&bone_world_matrices[i])
        };

        // Extract translation and rotation from local matrix
        let local_translation = [
            local_matrix.rows[3][0],
            local_matrix.rows[3][1],
            local_matrix.rows[3][2],
        ];
        let local_rotation = local_matrix.to_quaternion();

        // Debug: print first few bones
        if i < 5 {
            eprintln!(
                "Bone [{}] {} local_translation: {:?}, rotation: {:?}",
                i, bone.name, local_translation, local_rotation
            );
        }

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
            rotation: Some(json::scene::UnitQuaternion(local_rotation)),
            scale: None,
            translation: Some(local_translation),
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices
    // Pad to 4-byte boundary for float alignment
    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        // The "transpose" version (column-by-column) looked better but had translation at wrong indices.
        // Let's keep the rotation UNCHANGED (no transpose) and put translation at correct indices.
        //
        // Source matrix (DirectX row-major, rows[row][col]):
        // | r00 r01 r02 0  |  row 0
        // | r10 r11 r12 0  |  row 1
        // | r20 r21 r22 0  |  row 2
        // | tx  ty  tz  1  |  row 3
        //
        // Target: Write rotation as-is (no transpose), translation at indices 12-14
        // glTF column-major storage:
        // Column 0: r00, r10, r20, 0   -> indices 0-3
        // Column 1: r01, r11, r21, 0   -> indices 4-7
        // Column 2: r02, r12, r22, 0   -> indices 8-11
        // Column 3: tx,  ty,  tz,  1   -> indices 12-15
        let m = &bone.model_to_bone.rows;

        // Column 0: source column 0
        buffer_data.extend_from_slice(&m[0][0].to_le_bytes()); // r00
        buffer_data.extend_from_slice(&m[1][0].to_le_bytes()); // r10
        buffer_data.extend_from_slice(&m[2][0].to_le_bytes()); // r20
        buffer_data.extend_from_slice(&0.0f32.to_le_bytes());  // 0

        // Column 1: source column 1
        buffer_data.extend_from_slice(&m[0][1].to_le_bytes()); // r01
        buffer_data.extend_from_slice(&m[1][1].to_le_bytes()); // r11
        buffer_data.extend_from_slice(&m[2][1].to_le_bytes()); // r21
        buffer_data.extend_from_slice(&0.0f32.to_le_bytes());  // 0

        // Column 2: source column 2
        buffer_data.extend_from_slice(&m[0][2].to_le_bytes()); // r02
        buffer_data.extend_from_slice(&m[1][2].to_le_bytes()); // r12
        buffer_data.extend_from_slice(&m[2][2].to_le_bytes()); // r22
        buffer_data.extend_from_slice(&0.0f32.to_le_bytes());  // 0

        // Column 3: translation (from source row 3)
        buffer_data.extend_from_slice(&m[3][0].to_le_bytes()); // tx
        buffer_data.extend_from_slice(&m[3][1].to_le_bytes()); // ty
        buffer_data.extend_from_slice(&m[3][2].to_le_bytes()); // tz
        buffer_data.extend_from_slice(&1.0f32.to_le_bytes());  // 1
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

/// Create skeleton nodes from granny bones (0x703 chunk) with correct inverse world matrices.
/// This follows the exact same approach as the Python Blender importer.
/// Returns (bone_nodes, inverse_bind_matrices_accessor_index).
fn create_skeleton_nodes_from_granny(
    bones: &[GrannyBone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> (Vec<json::Node>, u32) {
    let mut nodes = Vec::with_capacity(bones.len());

    // V19: Use flat structure (no children hierarchy) with world transforms
    // This is closer to v17 which worked visually but had validation errors.
    // The Python script sets bpyBone.matrix = worldMatrix directly in Blender edit mode.
    // Blender handles hierarchy internally, but glTF compounds transforms.
    // To match, we use world transforms WITHOUT parent-child relationships.

    // Following the Python Blender importer EXACTLY:
    //   matUnpack = struct.unpack("<ffffffffffffffff", granny[cur + 80 : cur + 80 + 64])
    //   invWorldMat = mathutils.Matrix((matUnpack[0:4], matUnpack[4:8], matUnpack[8:12], matUnpack[12:16]))
    //   invWorldMat.invert()
    //   invWorldMat.transpose()
    //   bpyBone.matrix = invWorldMat
    //
    // The Python script sets bpyBone.matrix which is the WORLD matrix in edit mode.
    // But glTF node transforms are LOCAL (relative to parent).
    // So we need to compute: local = parent_world^-1 * world
    //
    // After invert+transpose, translation is in column 3 (rows[0][3], rows[1][3], rows[2][3])

    let bone_world_matrices: Vec<_> = bones
        .iter()
        .enumerate()
        .map(|(i, b)| {
            // Debug: print raw matrix for first few bones
            if i < 3 {
                eprintln!(
                    "GrannyBone [{}] {} raw inverse_world_matrix:\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]",
                    i, b.name,
                    b.inverse_world_matrix.rows[0][0], b.inverse_world_matrix.rows[0][1], b.inverse_world_matrix.rows[0][2], b.inverse_world_matrix.rows[0][3],
                    b.inverse_world_matrix.rows[1][0], b.inverse_world_matrix.rows[1][1], b.inverse_world_matrix.rows[1][2], b.inverse_world_matrix.rows[1][3],
                    b.inverse_world_matrix.rows[2][0], b.inverse_world_matrix.rows[2][1], b.inverse_world_matrix.rows[2][2], b.inverse_world_matrix.rows[2][3],
                    b.inverse_world_matrix.rows[3][0], b.inverse_world_matrix.rows[3][1], b.inverse_world_matrix.rows[3][2], b.inverse_world_matrix.rows[3][3],
                );
            }

            // Python script does: invWorldMat.invert() then invWorldMat.transpose()
            let world_mat = b
                .inverse_world_matrix
                .inverse()
                .unwrap_or_else(crate::types::Matrix4x4::identity)
                .transpose(); // Match Python: invert then transpose

            if i < 3 {
                eprintln!(
                    "GrannyBone [{}] {} after invert+transpose (world matrix):\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]\n  [{:.4}, {:.4}, {:.4}, {:.4}]",
                    i, b.name,
                    world_mat.rows[0][0], world_mat.rows[0][1], world_mat.rows[0][2], world_mat.rows[0][3],
                    world_mat.rows[1][0], world_mat.rows[1][1], world_mat.rows[1][2], world_mat.rows[1][3],
                    world_mat.rows[2][0], world_mat.rows[2][1], world_mat.rows[2][2], world_mat.rows[2][3],
                    world_mat.rows[3][0], world_mat.rows[3][1], world_mat.rows[3][2], world_mat.rows[3][3],
                );
            }

            world_mat
        })
        .collect();

    // V19: Use flat structure - each bone node has NO children
    // We use world transforms directly (like the Python script does with bpyBone.matrix)
    // glTF skeleton property will point to bone 0 (root)
    for (i, bone) in bones.iter().enumerate() {
        let world_matrix = &bone_world_matrices[i];

        // Extract translation from column 3 (after transpose, translation is at [row][3])
        let translation = [
            world_matrix.rows[0][3],
            world_matrix.rows[1][3],
            world_matrix.rows[2][3],
        ];

        // Extract rotation from the 3x3 part
        let rotation = world_matrix.to_quaternion();

        // Debug: print first few bones
        if i < 5 {
            eprintln!(
                "GrannyBone [{}] {} world_translation: {:?}, rotation: {:?}",
                i, bone.name, translation, rotation
            );
        }

        nodes.push(json::Node {
            camera: None,
            children: None, // V19: Flat structure, no hierarchy
            extensions: None,
            extras: json::Extras::default(),
            matrix: None,
            mesh: None,
            rotation: Some(json::scene::UnitQuaternion(rotation)),
            scale: None,
            translation: Some(translation),
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices
    // The inverse bind matrix in glTF is the transform that takes a vertex from model space
    // to bone space. This is exactly what inverse_world_matrix already is.
    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        // Write the inverse_world_matrix as glTF inverse bind matrix
        // Our source matrix has translation in row 3: [tx, ty, tz, 1]
        // glTF expects affine matrices with translation in column 3 (indices 12-14)
        // and the last row (indices 3, 7, 11) must be [0, 0, 0, 1]
        // So we need to transpose before writing column-major
        let m = bone.inverse_world_matrix.transpose();
        let m = &m.rows;

        // Column 0: m[0][0], m[1][0], m[2][0], m[3][0] -> should be [r00, r10, r20, 0]
        buffer_data.extend_from_slice(&m[0][0].to_le_bytes());
        buffer_data.extend_from_slice(&m[1][0].to_le_bytes());
        buffer_data.extend_from_slice(&m[2][0].to_le_bytes());
        buffer_data.extend_from_slice(&m[3][0].to_le_bytes());

        // Column 1: m[0][1], m[1][1], m[2][1], m[3][1] -> should be [r01, r11, r21, 0]
        buffer_data.extend_from_slice(&m[0][1].to_le_bytes());
        buffer_data.extend_from_slice(&m[1][1].to_le_bytes());
        buffer_data.extend_from_slice(&m[2][1].to_le_bytes());
        buffer_data.extend_from_slice(&m[3][1].to_le_bytes());

        // Column 2: m[0][2], m[1][2], m[2][2], m[3][2] -> should be [r02, r12, r22, 0]
        buffer_data.extend_from_slice(&m[0][2].to_le_bytes());
        buffer_data.extend_from_slice(&m[1][2].to_le_bytes());
        buffer_data.extend_from_slice(&m[2][2].to_le_bytes());
        buffer_data.extend_from_slice(&m[3][2].to_le_bytes());

        // Column 3: m[0][3], m[1][3], m[2][3], m[3][3] -> should be [tx, ty, tz, 1]
        buffer_data.extend_from_slice(&m[0][3].to_le_bytes());
        buffer_data.extend_from_slice(&m[1][3].to_le_bytes());
        buffer_data.extend_from_slice(&m[2][3].to_le_bytes());
        buffer_data.extend_from_slice(&m[3][3].to_le_bytes());
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
