//! glTF export for UGX models.
//!
//! Converts UGX geometry to glTF 2.0 format with skeleton support.
//!
//! # Matrix convention notes
//!
//! UGX/Granny stores matrices in DirectX row-major, row-vector convention:
//!   `v_transformed = v * M` with translation in row 3.
//!
//! glTF uses OpenGL column-major, column-vector convention:
//!   `v_transformed = M * v` with translation in column 3.
//!
//! Key insight: column-major storage of `M_gl` = row-major storage of `M_dx`,
//! because `M_gl = M_dx^T`. So we just write DX matrix rows flat for glTF.

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
///
/// Uses "buffer.bin" as the external buffer filename when `embed_buffers` is false.
pub fn export_to_gltf(geom: &UgxGeom, options: &GltfExportOptions) -> Result<GltfExport> {
    export_to_gltf_with_buffer_name(geom, options, "buffer.bin")
}

/// Export UGX geometry to glTF format with a specific external buffer filename.
pub fn export_to_gltf_with_buffer_name(
    geom: &UgxGeom,
    options: &GltfExportOptions,
    buffer_name: &str,
) -> Result<GltfExport> {
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
                base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, mat.opacity]),
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
                name: None,
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
            section.rigid_bone_index,
        );

        meshes.push(json::Mesh {
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            primitives: vec![primitive],
            weights: None,
        });
    }

    // Create skeleton nodes and skin if we have bones (must happen BEFORE buffer creation)
    let mut nodes = Vec::new();
    let mut skins = Vec::new();
    let skin_index: Option<json::Index<json::Skin>>;
    let mut root_bone_indices: Vec<u32> = Vec::new();

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
        root_bone_indices = if use_granny_bones {
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

        // Set skeleton root to first root bone (if there is one)
        let skeleton_root = root_bone_indices.first().copied().map(json::Index::new);

        skins.push(json::Skin {
            extensions: None,
            extras: json::Extras::default(),
            inverse_bind_matrices: Some(json::Index::new(ibm_accessor_idx)),
            joints: joint_indices,
            name: Some("Armature".to_string()),
            skeleton: skeleton_root,
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
                name: Some(format!("mesh_{}", i)),
                rotation: None,
                scale: None,
                translation: None,
                skin: skin_index.clone(),
                weights: None,
            });
        }
    } else {
        // No skeleton - just create mesh nodes
        for (i, _mesh) in meshes.iter().enumerate() {
            nodes.push(json::Node {
                camera: None,
                children: None,
                extensions: None,
                extras: json::Extras::default(),
                matrix: None,
                mesh: Some(json::Index::new(i as u32)),
                name: Some(format!("mesh_{}", i)),
                rotation: None,
                scale: None,
                translation: None,
                skin: None,
                weights: None,
            });
        }
    }

    // Build scene: only root bones (children reached through hierarchy) + mesh nodes
    let mut scene_node_indices = Vec::new();
    if has_skeleton {
        // Only root bones go in the scene (child bones are in parent.children)
        for &root_idx in &root_bone_indices {
            scene_node_indices.push(json::Index::new(root_idx));
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
        name: None,
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
            name: None,
        }
    } else {
        json::Buffer {
            byte_length: json::validation::USize64(buffer_length),
            uri: Some(buffer_name.to_string()),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
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
    rigid_bone_index: i32,
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
        name: None,
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
        name: None,
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Valid(json::mesh::Semantic::Positions),
        json::Index::new(pos_accessor_idx),
    );

    // Write normals (normalized to unit length for glTF compliance)
    let norm_view_idx = buffer_views.len() as u32;
    let norm_offset = buffer_data.len();
    for v in vertices {
        let len = (v.normal[0] * v.normal[0]
            + v.normal[1] * v.normal[1]
            + v.normal[2] * v.normal[2])
            .sqrt();
        let (nx, ny, nz) = if len > 1e-6 {
            (v.normal[0] / len, v.normal[1] / len, v.normal[2] / len)
        } else {
            (0.0, 1.0, 0.0)
        };
        buffer_data.extend_from_slice(&nx.to_le_bytes());
        buffer_data.extend_from_slice(&ny.to_le_bytes());
        buffer_data.extend_from_slice(&nz.to_le_bytes());
    }
    let norm_byte_length = buffer_data.len() - norm_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(norm_byte_length as u64),
        byte_offset: Some(json::validation::USize64(norm_offset as u64)),
        byte_stride: Some(json::buffer::Stride(12)),
        extensions: None,
        extras: json::Extras::default(),
        name: None,
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
        name: None,
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Valid(json::mesh::Semantic::Normals),
        json::Index::new(norm_accessor_idx),
    );

    // Write UV sets (TEXCOORD_0, TEXCOORD_1, ...)
    let max_texcoords = vertices.iter().map(|v| v.num_texcoords).max().unwrap_or(0);
    for uv_set in 0..max_texcoords {
        let uv_view_idx = buffer_views.len() as u32;
        let uv_offset = buffer_data.len();
        for v in vertices {
            // glTF and DirectX both use V=0 at top (no flip needed).
            // Note: the Python Blender script flips V for Blender's convention,
            // but that's Blender-specific — glTF matches DX convention already.
            buffer_data.extend_from_slice(&v.texcoords[uv_set][0].to_le_bytes());
            buffer_data.extend_from_slice(&v.texcoords[uv_set][1].to_le_bytes());
        }
        let uv_byte_length = buffer_data.len() - uv_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(uv_byte_length as u64),
            byte_offset: Some(json::validation::USize64(uv_offset as u64)),
            byte_stride: Some(json::buffer::Stride(8)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
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
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::TexCoords(uv_set as u32)),
            json::Index::new(uv_accessor_idx),
        );
    }

    // Write tangents (vec4: xyz + handedness in w)
    let has_tangents = vertices.iter().any(|v| {
        v.tangent[0] != 0.0 || v.tangent[1] != 0.0 || v.tangent[2] != 0.0
    });
    if has_tangents {
        let tangent_view_idx = buffer_views.len() as u32;
        let tangent_offset = buffer_data.len();
        for v in vertices {
            // glTF requires unit-length tangent xyz. UGX tangents may not be
            // normalized (e.g. HWDE stores them at length 0.5).
            let len = (v.tangent[0] * v.tangent[0]
                + v.tangent[1] * v.tangent[1]
                + v.tangent[2] * v.tangent[2])
                .sqrt();
            let (tx, ty, tz) = if len > 1e-6 {
                (v.tangent[0] / len, v.tangent[1] / len, v.tangent[2] / len)
            } else {
                (1.0, 0.0, 0.0)
            };
            buffer_data.extend_from_slice(&tx.to_le_bytes());
            buffer_data.extend_from_slice(&ty.to_le_bytes());
            buffer_data.extend_from_slice(&tz.to_le_bytes());
            // glTF tangent.w is handedness: +1 or -1.
            // UGX stores this in tangent[3]; default to 1.0 if unset.
            let w = if v.tangent[3] == 0.0 { 1.0f32 } else { v.tangent[3] };
            buffer_data.extend_from_slice(&w.to_le_bytes());
        }
        let tangent_byte_length = buffer_data.len() - tangent_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(tangent_byte_length as u64),
            byte_offset: Some(json::validation::USize64(tangent_offset as u64)),
            byte_stride: Some(json::buffer::Stride(16)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let tangent_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(tangent_view_idx)),
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
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Tangents),
            json::Index::new(tangent_accessor_idx),
        );
    }

    // Write bone indices and weights if we have a skeleton
    if has_skeleton && bone_count > 0 {
        let max_bone_idx = (bone_count - 1) as u16;
        let use_u16_joints = bone_count > 256;
        // rigid_bone_index can be INT_MAX (0x7FFFFFFF) meaning "no rigid bone".
        // Default to bone 0 when invalid.
        let rigid_idx: u16 =
            if rigid_bone_index >= 0 && (rigid_bone_index as usize) < bone_count {
                rigid_bone_index as u16
            } else {
                0
            };

        // JOINTS_0 - bone indices as u8 (<=256 bones) or u16 (>256 bones)
        // Pad to 2-byte boundary if using u16
        if use_u16_joints && buffer_data.len() % 2 != 0 {
            buffer_data.push(0);
        }
        let joints_view_idx = buffer_views.len() as u32;
        let joints_offset = buffer_data.len();
        for v in vertices {
            let weight_sum: f32 = v.bone_weights.iter().sum();
            let joint_indices = if weight_sum == 0.0 {
                // Rigid vertex (no skin data) - bind to section's rigid bone
                [rigid_idx, 0, 0, 0]
            } else {
                let mut indices = v.bone_indices;
                for idx in &mut indices {
                    // Vertex bone indices are 1-based in UGX data (0 = no bone);
                    // convert to 0-based for glTF joint indices.
                    if *idx > 0 {
                        *idx -= 1;
                    }
                    if *idx > max_bone_idx {
                        *idx = 0;
                    }
                }
                indices
            };
            if use_u16_joints {
                for &idx in &joint_indices {
                    buffer_data.extend_from_slice(&idx.to_le_bytes());
                }
            } else {
                for &idx in &joint_indices {
                    buffer_data.push(idx as u8);
                }
            }
        }
        let joints_byte_length = buffer_data.len() - joints_offset;
        let joints_stride = if use_u16_joints { 8 } else { 4 };

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(joints_byte_length as u64),
            byte_offset: Some(json::validation::USize64(joints_offset as u64)),
            byte_stride: Some(json::buffer::Stride(joints_stride)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let joints_component_type = if use_u16_joints {
            json::accessor::ComponentType::U16
        } else {
            json::accessor::ComponentType::U8
        };
        let joints_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(joints_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                joints_component_type,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Joints(0)),
            json::Index::new(joints_accessor_idx),
        );

        // WEIGHTS_0 - bone weights as floats
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
                // Rigid vertex - 100% weight on rigid bone
                weights[0] = 1.0;
            } else if (sum - 1.0).abs() > 0.001 {
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
            name: None,
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
            name: None,
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
        name: None,
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
        name: None,
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

/// Create skeleton nodes from cached data bones (0x700 chunk).
/// Fallback when granny bones aren't available.
/// Returns (bone_nodes, inverse_bind_matrices_accessor_index).
fn create_skeleton_nodes(
    bones: &[Bone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> (Vec<json::Node>, u32) {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for hierarchy
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

    // Compute world transforms in DX row-major convention.
    // model_to_bone: model->bone (DX: v_bone = v_model * M)
    // Invert to get world transform (DX: v_model = v_bone * W_dx)
    let bone_world_dx: Vec<_> = bones
        .iter()
        .map(|b| {
            b.model_to_bone
                .inverse()
                .unwrap_or_else(crate::types::Matrix4x4::identity)
        })
        .collect();

    // Create nodes with local transforms and parent-child hierarchy.
    for (i, bone) in bones.iter().enumerate() {
        let children = children_map.get(&(i as i32)).map(|c| {
            c.iter()
                .map(|&idx| json::Index::new(idx))
                .collect::<Vec<_>>()
        });

        // DX: local = world * parent_world^{-1}
        // Use model_to_bone[parent] directly (= parent_world^{-1}) to avoid
        // double-inversion precision loss.
        let local_dx = if bone.parent_index < 0 {
            bone_world_dx[i].clone()
        } else {
            let parent_idx = bone.parent_index as usize;
            bone_world_dx[i].multiply(&bones[parent_idx].model_to_bone)
        };

        // Write DX rows flat = column-major of GL matrix
        let m = &local_dx.rows;
        let gltf_matrix = [
            m[0][0], m[0][1], m[0][2], m[0][3],
            m[1][0], m[1][1], m[1][2], m[1][3],
            m[2][0], m[2][1], m[2][2], m[2][3],
            m[3][0], m[3][1], m[3][2], m[3][3],
        ];

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().map_or(true, |c| c.is_empty()) {
                None
            } else {
                children
            },
            extensions: None,
            extras: json::Extras::default(),
            matrix: Some(gltf_matrix),
            mesh: None,
            name: Some(bone.name.clone()),
            rotation: None,
            scale: None,
            translation: None,
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices.
    // model_to_bone rows flat = column-major of GL IBM (same derivation as granny path).
    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        let m = &bone.model_to_bone.rows;
        for row in m {
            for &val in row {
                buffer_data.extend_from_slice(&val.to_le_bytes());
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
        name: None,
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
        name: None,
        normalized: false,
        sparse: None,
    });

    (nodes, ibm_accessor_idx)
}

/// Create skeleton nodes from granny bones (0x703 chunk).
/// Uses hierarchical structure with local transforms.
/// Returns (bone_nodes, inverse_bind_matrices_accessor_index).
fn create_skeleton_nodes_from_granny(
    bones: &[GrannyBone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> (Vec<json::Node>, u32) {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for hierarchy
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

    // Compute world transforms in DirectX row-major convention.
    // inverse_world_matrix: model->bone (DX: v_bone = v_model * IWM)
    // Invert to get: bone->model / world transform (DX: v_model = v_bone * W_dx)
    let bone_world_dx: Vec<_> = bones
        .iter()
        .map(|b| {
            b.inverse_world_matrix
                .inverse()
                .unwrap_or_else(crate::types::Matrix4x4::identity)
        })
        .collect();

    // Create nodes with local transforms and parent-child hierarchy.
    for (i, bone) in bones.iter().enumerate() {
        let children = children_map.get(&(i as i32)).map(|c| {
            c.iter()
                .map(|&idx| json::Index::new(idx))
                .collect::<Vec<_>>()
        });

        // In DX row-vector convention: v_world = v_local * local_dx * parent_world_dx
        // So: world_dx = local_dx * parent_world_dx
        // Therefore: local_dx = world_dx * parent_world_dx^{-1}
        //
        // Optimization: parent_world_dx^{-1} = IWM[parent] (which we already have),
        // avoiding double-inversion precision loss from (IWM.inverse()).inverse().
        let local_dx = if bone.parent_index < 0 {
            bone_world_dx[i].clone()
        } else {
            let parent_idx = bone.parent_index as usize;
            bone_world_dx[i].multiply(&bones[parent_idx].inverse_world_matrix)
        };

        // glTF column-major storage of M_gl = row-major storage of M_dx
        // (because M_gl = M_dx^T, and column-major(M^T) = row-major(M))
        // So just write DX matrix rows flat.
        let m = &local_dx.rows;
        let gltf_matrix = [
            m[0][0], m[0][1], m[0][2], m[0][3],
            m[1][0], m[1][1], m[1][2], m[1][3],
            m[2][0], m[2][1], m[2][2], m[2][3],
            m[3][0], m[3][1], m[3][2], m[3][3],
        ];

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().map_or(true, |c| c.is_empty()) {
                None
            } else {
                children
            },
            extensions: None,
            extras: json::Extras::default(),
            matrix: Some(gltf_matrix),
            mesh: None,
            name: Some(bone.name.clone()),
            rotation: None,
            scale: None,
            translation: None,
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices.
    // IWM is the model->bone transform in DX convention.
    // glTF IBM in GL convention = IWM^T.
    // column-major(IWM^T) = row-major(IWM), so just write IWM rows flat.
    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        let m = &bone.inverse_world_matrix.rows;
        for row in m {
            for &val in row {
                buffer_data.extend_from_slice(&val.to_le_bytes());
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
        name: None,
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
        name: None,
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

    /// Helper: build a minimal vertex with position only.
    fn vertex(pos: [f32; 3]) -> UnpackedVertex {
        UnpackedVertex {
            position: pos,
            normal: [0.0, 1.0, 0.0],
            ..Default::default()
        }
    }

    /// Helper: find a semantic in a primitive's attributes.
    fn has_semantic(
        prim: &json::mesh::Primitive,
        semantic: json::mesh::Semantic,
    ) -> bool {
        prim.attributes.contains_key(&Valid(semantic))
    }

    /// Helper: get the accessor for a semantic.
    fn get_accessor<'a>(
        prim: &json::mesh::Primitive,
        accessors: &'a [json::Accessor],
        semantic: json::mesh::Semantic,
    ) -> Option<&'a json::Accessor> {
        prim.attributes
            .get(&Valid(semantic))
            .map(|idx| &accessors[idx.value()])
    }

    // ---- Multiple UV sets ----

    #[test]
    fn test_single_uv_set() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts {
            v.texcoords[0] = [0.5, 0.5];
            v.num_texcoords = 1;
        }
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        assert!(has_semantic(&prim, json::mesh::Semantic::TexCoords(0)));
        assert!(!has_semantic(&prim, json::mesh::Semantic::TexCoords(1)));
    }

    #[test]
    fn test_multiple_uv_sets() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts {
            v.texcoords[0] = [0.1, 0.2];
            v.texcoords[1] = [0.3, 0.4];
            v.texcoords[2] = [0.5, 0.6];
            v.num_texcoords = 3;
        }
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        assert!(has_semantic(&prim, json::mesh::Semantic::TexCoords(0)));
        assert!(has_semantic(&prim, json::mesh::Semantic::TexCoords(1)));
        assert!(has_semantic(&prim, json::mesh::Semantic::TexCoords(2)));
        assert!(!has_semantic(&prim, json::mesh::Semantic::TexCoords(3)));

        // Each UV accessor should be Vec2/F32
        for i in 0..3 {
            let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::TexCoords(i)).unwrap();
            assert_eq!(acc.type_, Valid(json::accessor::Type::Vec2));
            assert_eq!(acc.count, json::validation::USize64(3));
        }
    }

    #[test]
    fn test_no_uv_when_zero_texcoords() {
        let verts = vec![vertex([0.0, 0.0, 0.0]); 3]; // num_texcoords = 0
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        assert!(!has_semantic(&prim, json::mesh::Semantic::TexCoords(0)));
    }

    // ---- Tangent export ----

    #[test]
    fn test_tangent_exported_when_present() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts {
            v.tangent = [1.0, 0.0, 0.0, 1.0];
        }
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        assert!(has_semantic(&prim, json::mesh::Semantic::Tangents));
        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Tangents).unwrap();
        assert_eq!(acc.type_, Valid(json::accessor::Type::Vec4));
        assert_eq!(acc.count, json::validation::USize64(3));
    }

    #[test]
    fn test_no_tangent_when_zero() {
        let verts = vec![vertex([0.0, 0.0, 0.0]); 3]; // tangent = [0,0,0,0]
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        assert!(!has_semantic(&prim, json::mesh::Semantic::Tangents));
    }

    #[test]
    fn test_tangent_normalization() {
        // Tangent at length 0.5 (like HWDE data)
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].tangent = [0.5, 0.0, 0.0, 1.0];
        let indices: Vec<u16> = vec![0, 0, 0];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        // Find the tangent data in the buffer
        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Tangents).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        let tx = f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let ty = f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
        let tz = f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());
        let tw = f32::from_le_bytes(buf[offset + 12..offset + 16].try_into().unwrap());

        let length = (tx * tx + ty * ty + tz * tz).sqrt();
        assert!((length - 1.0).abs() < 1e-5, "tangent should be unit length, got {}", length);
        assert!((tx - 1.0).abs() < 1e-5, "expected tx=1.0, got {}", tx);
        assert_eq!(tw, 1.0, "handedness should be preserved");
    }

    #[test]
    fn test_tangent_handedness_preserved() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].tangent = [0.0, 0.0, 0.5, -1.0]; // negative handedness
        let indices: Vec<u16> = vec![0, 0, 0];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Tangents).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        let tw = f32::from_le_bytes(buf[offset + 12..offset + 16].try_into().unwrap());
        assert_eq!(tw, -1.0, "negative handedness should be preserved");
    }

    // ---- Normal normalization ----

    #[test]
    fn test_normal_normalization() {
        // Non-unit normal: (2, 0, 0) should become (1, 0, 0)
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].normal = [2.0, 0.0, 0.0];
        let indices: Vec<u16> = vec![0, 0, 0];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        let acc = get_accessor(
            &json::mesh::Primitive {
                attributes: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(Valid(json::mesh::Semantic::Normals), json::Index::new(1));
                    m
                },
                extensions: None,
                extras: json::Extras::default(),
                indices: None,
                material: None,
                mode: Valid(json::mesh::Mode::Triangles),
                targets: None,
            },
            &accessors,
            json::mesh::Semantic::Normals,
        ).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        let nx = f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let ny = f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
        let nz = f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());

        let length = (nx * nx + ny * ny + nz * nz).sqrt();
        assert!((length - 1.0).abs() < 1e-5, "normal should be unit length, got {}", length);
        assert!((nx - 1.0).abs() < 1e-5, "expected nx=1.0, got {}", nx);
    }

    #[test]
    fn test_normal_zero_fallback() {
        // Zero normal should fall back to (0, 1, 0)
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].normal = [0.0, 0.0, 0.0];
        let indices: Vec<u16> = vec![0, 0, 0];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, false, 0, -1);

        // Normal accessor is index 1 (after position at 0), view index 1
        let view = &views[1];
        let offset = view.byte_offset.unwrap().0 as usize;

        let nx = f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let ny = f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
        let nz = f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());

        assert!((nx - 0.0).abs() < 1e-5);
        assert!((ny - 1.0).abs() < 1e-5, "zero normal should fall back to up (0,1,0)");
        assert!((nz - 0.0).abs() < 1e-5);
    }

    // ---- Joint index types ----

    #[test]
    fn test_joints_u8_for_small_skeleton() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts {
            v.bone_indices = [1, 2, 0, 0]; // 1-based
            v.bone_weights = [0.7, 0.3, 0.0, 0.0];
        }
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, true, 50, -1);

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        // U8 joints for <=256 bones
        match &acc.component_type {
            Valid(json::accessor::GenericComponentType(ct)) => {
                assert!(matches!(ct, json::accessor::ComponentType::U8), "expected U8, got {:?}", ct);
            }
            other => panic!("unexpected component_type: {:?}", other),
        }
    }

    #[test]
    fn test_joints_u16_for_large_skeleton() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts {
            v.bone_indices = [1, 2, 0, 0];
            v.bone_weights = [0.7, 0.3, 0.0, 0.0];
        }
        let indices: Vec<u16> = vec![0, 1, 2];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, true, 300, -1);

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        // U16 joints for >256 bones
        match &acc.component_type {
            Valid(json::accessor::GenericComponentType(ct)) => {
                assert!(matches!(ct, json::accessor::ComponentType::U16), "expected U16, got {:?}", ct);
            }
            other => panic!("unexpected component_type: {:?}", other),
        }
    }

    #[test]
    fn test_joints_1based_to_0based() {
        // Bone indices in UGX are 1-based; glTF expects 0-based
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].bone_indices = [3, 1, 0, 0]; // 1-based: bone 2, bone 0, none, none
        verts[0].bone_weights = [0.8, 0.2, 0.0, 0.0];
        let indices: Vec<u16> = vec![0, 0, 0];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(&verts, &indices, -1, &mut buf, &mut accessors, &mut views, false, true, 10, -1);

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        // Should be 0-based: [2, 0, 0, 0]
        assert_eq!(buf[offset], 2);
        assert_eq!(buf[offset + 1], 0);
        assert_eq!(buf[offset + 2], 0);
        assert_eq!(buf[offset + 3], 0);
    }

    #[test]
    fn test_rigid_vertex_gets_rigid_bone() {
        // Vertex with zero weights = rigid, should get section's rigid bone
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].bone_weights = [0.0, 0.0, 0.0, 0.0]; // rigid
        let indices: Vec<u16> = vec![0, 0, 0];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(
            &verts, &indices, -1, &mut buf, &mut accessors, &mut views,
            false, true, 10, 5, // rigid_bone_index = 5
        );

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        assert_eq!(buf[offset], 5, "rigid vertex should use section rigid bone");
    }
}
