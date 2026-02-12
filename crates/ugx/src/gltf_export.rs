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
use crate::types::{Bone, MapType, Material};
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

/// Non-PBR map types that need to be stored in extras.
/// Diffuse, Normal, AO, and Emissive are handled by standard PBR fields.
const NON_PBR_MAP_TYPES: &[MapType] = &[
    MapType::Gloss,
    MapType::Opacity,
    MapType::XForm,
    MapType::Env,
    MapType::EnvMask,
    MapType::EmXForm,
    MapType::Distortion,
    MapType::Highlight,
    MapType::Modulate,
];

/// Build glTF material extras JSON for UGX-specific data.
///
/// Stores material flags, UVW velocity, and non-PBR texture maps
/// so they survive a glTF roundtrip.
fn build_material_extras(mat: &Material) -> json::Extras {
    let mut extras = serde_json::Map::new();

    // Always store flags (even if 0, for roundtrip fidelity)
    extras.insert(
        "ugx_flags".into(),
        serde_json::Value::Number(mat.flags.into()),
    );

    // Store UVW velocity arrays that have non-zero values
    let has_any_uvw = mat
        .uvw_velocity
        .iter()
        .any(|v| v[0] != 0.0 || v[1] != 0.0 || v[2] != 0.0);
    if has_any_uvw {
        let uvw_arr: Vec<serde_json::Value> = mat
            .uvw_velocity
            .iter()
            .map(|v| {
                serde_json::Value::Array(vec![
                    serde_json::Value::from(v[0]),
                    serde_json::Value::from(v[1]),
                    serde_json::Value::from(v[2]),
                ])
            })
            .collect();
        extras.insert("ugx_uvw_velocity".into(), serde_json::Value::Array(uvw_arr));
    }

    // Store non-PBR texture maps
    let mut maps_obj = serde_json::Map::new();
    for &map_type in NON_PBR_MAP_TYPES {
        let idx = map_type as usize;
        if !mat.maps[idx].is_empty() {
            let maps_arr: Vec<serde_json::Value> = mat.maps[idx]
                .iter()
                .map(|m| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("name".into(), serde_json::Value::String(m.name.clone()));
                    obj.insert(
                        "channel".into(),
                        serde_json::Value::Number((m.channel as i64).into()),
                    );
                    obj.insert(
                        "flags".into(),
                        serde_json::Value::Number((m.flags as u64).into()),
                    );
                    serde_json::Value::Object(obj)
                })
                .collect();
            maps_obj.insert(
                map_type.name().to_string(),
                serde_json::Value::Array(maps_arr),
            );
        }
    }
    if !maps_obj.is_empty() {
        extras.insert("ugx_maps".into(), serde_json::Value::Object(maps_obj));
    }

    if extras.is_empty() {
        None
    } else {
        let json_str = serde_json::to_string(&serde_json::Value::Object(extras)).unwrap();
        Some(serde_json::value::RawValue::from_string(json_str).unwrap())
    }
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
    let mut images_json: Vec<json::Image> = Vec::new();
    let mut textures_json: Vec<json::Texture> = Vec::new();

    // Create materials with texture references if requested
    if options.include_materials {
        // Pass 1: Build texture registry (deduplicated image/texture objects)
        let mut texture_map: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for mat in &geom.materials {
            for map_type in MapType::ALL {
                for map in &mat.maps[map_type as usize] {
                    if !map.name.is_empty() && !texture_map.contains_key(&map.name) {
                        let image_idx = images_json.len() as u32;
                        images_json.push(json::Image {
                            buffer_view: None,
                            mime_type: None,
                            name: Some(map.name.clone()),
                            uri: Some(map.name.clone()),
                            extensions: None,
                            extras: json::Extras::default(),
                        });
                        let texture_idx = textures_json.len() as u32;
                        textures_json.push(json::Texture {
                            name: None,
                            sampler: None,
                            source: json::Index::new(image_idx),
                            extensions: None,
                            extras: json::Extras::default(),
                        });
                        texture_map.insert(map.name.clone(), texture_idx);
                    }
                }
            }
        }

        // Pass 2: Create glTF materials with texture references
        for mat in &geom.materials {
            // Diffuse → baseColorTexture
            let base_color_texture = mat.maps[MapType::Diffuse as usize]
                .first()
                .filter(|m| !m.name.is_empty())
                .map(|m| json::texture::Info {
                    index: json::Index::new(texture_map[&m.name]),
                    tex_coord: m.channel as u32,
                    extensions: None,
                    extras: json::Extras::default(),
                });

            // Normal → normalTexture
            let normal_texture = mat.maps[MapType::Normal as usize]
                .first()
                .filter(|m| !m.name.is_empty())
                .map(|m| json::material::NormalTexture {
                    index: json::Index::new(texture_map[&m.name]),
                    scale: 1.0,
                    tex_coord: m.channel as u32,
                    extensions: None,
                    extras: json::Extras::default(),
                });

            // AO → occlusionTexture
            let occlusion_texture = mat.maps[MapType::AO as usize]
                .first()
                .filter(|m| !m.name.is_empty())
                .map(|m| json::material::OcclusionTexture {
                    index: json::Index::new(texture_map[&m.name]),
                    strength: json::material::StrengthFactor(1.0),
                    tex_coord: m.channel as u32,
                    extensions: None,
                    extras: json::Extras::default(),
                });

            // Emissive → emissiveTexture
            let emissive_texture = mat.maps[MapType::Emissive as usize]
                .first()
                .filter(|m| !m.name.is_empty())
                .map(|m| json::texture::Info {
                    index: json::Index::new(texture_map[&m.name]),
                    tex_coord: m.channel as u32,
                    extensions: None,
                    extras: json::Extras::default(),
                });

            // Emissive factor must be [1,1,1] for emissive texture to have effect
            let emissive_factor = if emissive_texture.is_some() {
                json::material::EmissiveFactor([1.0, 1.0, 1.0])
            } else {
                json::material::EmissiveFactor([0.0, 0.0, 0.0])
            };

            // Alpha mode: blend if blend_type > 0 or opacity < 1.0
            let alpha_mode = if mat.blend_type > 0 || mat.opacity < 1.0 {
                Valid(json::material::AlphaMode::Blend)
            } else {
                Valid(json::material::AlphaMode::Opaque)
            };

            let pbr = json::material::PbrMetallicRoughness {
                base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, mat.opacity]),
                base_color_texture,
                metallic_factor: json::material::StrengthFactor(0.0),
                roughness_factor: json::material::StrengthFactor(
                    1.0 - (mat.spec_power / 100.0).clamp(0.0, 1.0),
                ),
                metallic_roughness_texture: None,
                extensions: None,
                extras: json::Extras::default(),
            };

            // Build extras JSON for UGX-specific data that doesn't map to PBR
            let extras = build_material_extras(mat);

            materials_json.push(json::Material {
                alpha_cutoff: None,
                alpha_mode,
                double_sided: false,
                pbr_metallic_roughness: pbr,
                normal_texture,
                occlusion_texture,
                emissive_texture,
                emissive_factor,
                extensions: None,
                extras,
                name: Some(mat.name.clone()),
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

    // Build section-to-mesh mapping by matching section bone usage against granny_mesh bone_bindings.
    // This helps determine the correct mesh name for each section.
    let section_to_mesh = build_section_to_mesh_mapping(geom, &geom.granny_bones);

    // Process each section as a separate glTF mesh (one primitive per mesh).
    // We can't reliably group sections into meshes because multiple meshes can have
    // the same bone_bindings (e.g., banshee has 6 meshes all using "bone_impact_01").
    // The import side generates granny_meshes from vertex skin data, which is more accurate.
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

        // Use the matching granny_mesh name if available, otherwise generate from section index
        let mesh_idx = section_to_mesh[section_idx];
        let mesh_name = if mesh_idx < geom.granny_meshes.len() {
            Some(geom.granny_meshes[mesh_idx].name.clone())
        } else {
            Some(format!("mesh_{}", section_idx))
        };

        meshes.push(json::Mesh {
            extensions: None,
            extras: json::Extras::default(),
            name: mesh_name,
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
                skin: skin_index,
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
    if !images_json.is_empty() {
        root.images = images_json;
    }
    if !textures_json.is_empty() {
        root.textures = textures_json;
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
#[allow(clippy::too_many_arguments)]
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
    while !buffer_data.len().is_multiple_of(4) {
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
        let len =
            (v.normal[0] * v.normal[0] + v.normal[1] * v.normal[1] + v.normal[2] * v.normal[2])
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
    let has_tangents = vertices
        .iter()
        .any(|v| v.tangent[0] != 0.0 || v.tangent[1] != 0.0 || v.tangent[2] != 0.0);
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
            let w = if v.tangent[3] == 0.0 {
                1.0f32
            } else {
                v.tangent[3]
            };
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
    //
    // TODO: Bone remap — when section.bone_remap is non-empty, vertex bone indices
    // are section-local and need to be remapped to global skeleton indices using the
    // remap table before writing to glTF JOINTS_0. Currently we write indices as-is
    // (converting from 1-based to 0-based), which is correct only when global_bones
    // is true or bone_remap is empty. To fix: pass bone_remap into create_primitive,
    // and when non-empty, do `global_idx = bone_remap[local_idx]` before the 1-based
    // to 0-based conversion. Need a real skinned UGX file with per-section bone
    // remaps to verify.
    if has_skeleton && bone_count > 0 {
        let max_bone_idx = (bone_count - 1) as u16;
        let use_u16_joints = bone_count > 256;
        // rigid_bone_index can be INT_MAX (0x7FFFFFFF) meaning "no rigid bone".
        // Default to bone 0 when invalid.
        let rigid_idx: u16 = if rigid_bone_index >= 0 && (rigid_bone_index as usize) < bone_count {
            rigid_bone_index as u16
        } else {
            0
        };

        // JOINTS_0 - bone indices as u8 (<=256 bones) or u16 (>256 bones)
        // Pad to 2-byte boundary if using u16
        if use_u16_joints && !buffer_data.len().is_multiple_of(2) {
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
            component_type: Valid(json::accessor::GenericComponentType(joints_component_type)),
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
        while !buffer_data.len().is_multiple_of(4) {
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

    // Write vertex colors (COLOR_0) if any vertex has non-zero diffuse
    let has_colors = vertices.iter().any(|v| {
        v.diffuse[0] != 0.0 || v.diffuse[1] != 0.0 || v.diffuse[2] != 0.0 || v.diffuse[3] != 0.0
    });
    if has_colors {
        // Pad to 4-byte boundary for float alignment
        while !buffer_data.len().is_multiple_of(4) {
            buffer_data.push(0);
        }
        let color_view_idx = buffer_views.len() as u32;
        let color_offset = buffer_data.len();
        for v in vertices {
            buffer_data.extend_from_slice(&v.diffuse[0].to_le_bytes());
            buffer_data.extend_from_slice(&v.diffuse[1].to_le_bytes());
            buffer_data.extend_from_slice(&v.diffuse[2].to_le_bytes());
            buffer_data.extend_from_slice(&v.diffuse[3].to_le_bytes());
        }
        let color_byte_length = buffer_data.len() - color_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(color_byte_length as u64),
            byte_offset: Some(json::validation::USize64(color_offset as u64)),
            byte_stride: Some(json::buffer::Stride(16)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let color_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(color_view_idx)),
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
            Valid(json::mesh::Semantic::Colors(0)),
            json::Index::new(color_accessor_idx),
        );
    }

    // Write indices
    // Pad to 2-byte boundary for u16 alignment
    if !buffer_data.len().is_multiple_of(2) {
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

/// Build a mapping from section index to mesh index.
/// Analyzes which bones each section uses and matches against granny_mesh bone_bindings.
fn build_section_to_mesh_mapping(geom: &UgxGeom, granny_bones: &[GrannyBone]) -> Vec<usize> {
    // If no granny_meshes, each section is its own mesh
    if geom.granny_meshes.is_empty() {
        return (0..geom.sections.len()).collect();
    }

    // Build a set of bone names for each granny_mesh
    let mesh_bone_sets: Vec<std::collections::HashSet<&str>> = geom
        .granny_meshes
        .iter()
        .map(|m| m.bone_bindings.iter().map(|s| s.as_str()).collect())
        .collect();

    // For each section, find which mesh it belongs to by matching bone usage
    let mut section_to_mesh = Vec::with_capacity(geom.sections.len());

    for section_idx in 0..geom.sections.len() {
        // Get bones used by this section's vertices
        let section_bones = get_section_bone_names(geom, section_idx, granny_bones);

        // Find the mesh whose bone_bindings best matches this section's bones
        let mesh_idx = find_best_matching_mesh(&section_bones, &mesh_bone_sets);
        section_to_mesh.push(mesh_idx);
    }

    section_to_mesh
}

/// Get the bone names used by a section's vertices.
fn get_section_bone_names(
    geom: &UgxGeom,
    section_idx: usize,
    granny_bones: &[GrannyBone],
) -> std::collections::HashSet<String> {
    let mut used_bones = std::collections::HashSet::new();

    // Try to unpack vertices; if that fails, fall back to rigid_bone_index
    if let Ok(vertices) = geom.unpack_section_vertices(section_idx) {
        for v in &vertices {
            for k in 0..4 {
                if v.bone_weights[k] > 0.0 {
                    let bone_idx = v.bone_indices[k] as usize;
                    if bone_idx > 0 && bone_idx <= granny_bones.len() {
                        // bone_indices are 1-based
                        used_bones.insert(granny_bones[bone_idx - 1].name.clone());
                    }
                }
            }
        }
    }

    // If no bones found from vertices (e.g., global_bones section), use rigid_bone_index
    if used_bones.is_empty() && section_idx < geom.sections.len() {
        let section = &geom.sections[section_idx];
        let rigid_idx = section.rigid_bone_index as usize;
        if rigid_idx > 0 && rigid_idx <= granny_bones.len() {
            used_bones.insert(granny_bones[rigid_idx - 1].name.clone());
        }
    }

    used_bones
}

/// Find the mesh index whose bone_bindings best matches the section's bones.
/// Prefers:
/// 1. Exact match (section bones == mesh bones)
/// 2. Smallest superset (mesh contains all section bones with fewest extras)
/// 3. Highest overlap if no complete containment
///
/// Returns the mesh index, or mesh count to create a new mesh if none match.
fn find_best_matching_mesh(
    section_bones: &std::collections::HashSet<String>,
    mesh_bone_sets: &[std::collections::HashSet<&str>],
) -> usize {
    if section_bones.is_empty() || mesh_bone_sets.is_empty() {
        return mesh_bone_sets.len(); // Fallback: create new mesh
    }

    let mut best_mesh = mesh_bone_sets.len();
    let mut best_score = (false, usize::MAX, 0usize); // (is_superset, mesh_size, overlap)

    for (mesh_idx, mesh_bones) in mesh_bone_sets.iter().enumerate() {
        // Count how many section bones are in this mesh's bone_bindings
        let overlap = section_bones
            .iter()
            .filter(|b| mesh_bones.contains(b.as_str()))
            .count();

        // Check if mesh contains ALL section bones (is a superset)
        let is_superset = overlap == section_bones.len();
        let mesh_size = mesh_bones.len();

        // Score: prefer superset, then smallest mesh, then highest overlap
        let score = (is_superset, mesh_size, overlap);

        // Better if: is superset when best isn't, OR both superset and smaller, OR more overlap
        let is_better = if is_superset && !best_score.0 {
            true // Superset beats non-superset
        } else if is_superset && best_score.0 {
            mesh_size < best_score.1 // Among supersets, prefer smaller
        } else if !is_superset && !best_score.0 {
            overlap > best_score.2 // Among non-supersets, prefer more overlap
        } else {
            false // Non-superset doesn't beat superset
        };

        if is_better {
            best_score = score;
            best_mesh = mesh_idx;
        }
    }

    // If no overlap found at all, return mesh count to create a new mesh
    if best_score.2 == 0 {
        mesh_bone_sets.len()
    } else {
        best_mesh
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
            m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0],
            m[2][1], m[2][2], m[2][3], m[3][0], m[3][1], m[3][2], m[3][3],
        ];

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().is_none_or(|c| c.is_empty()) {
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
    while !buffer_data.len().is_multiple_of(4) {
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
            m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0],
            m[2][1], m[2][2], m[2][3], m[3][0], m[3][1], m[3][2], m[3][3],
        ];

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().is_none_or(|c| c.is_empty()) {
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
    while !buffer_data.len().is_multiple_of(4) {
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
    fn has_semantic(prim: &json::mesh::Primitive, semantic: json::mesh::Semantic) -> bool {
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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

        // Find the tangent data in the buffer
        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Tangents).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        let tx = f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let ty = f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
        let tz = f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());
        let tw = f32::from_le_bytes(buf[offset + 12..offset + 16].try_into().unwrap());

        let length = (tx * tx + ty * ty + tz * tz).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "tangent should be unit length, got {}",
            length
        );
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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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

        create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

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
        )
        .unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        let nx = f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let ny = f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
        let nz = f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());

        let length = (nx * nx + ny * ny + nz * nz).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "normal should be unit length, got {}",
            length
        );
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

        create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            false,
            0,
            -1,
        );

        // Normal accessor is index 1 (after position at 0), view index 1
        let view = &views[1];
        let offset = view.byte_offset.unwrap().0 as usize;

        let nx = f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let ny = f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
        let nz = f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());

        assert!((nx - 0.0).abs() < 1e-5);
        assert!(
            (ny - 1.0).abs() < 1e-5,
            "zero normal should fall back to up (0,1,0)"
        );
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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            true,
            50,
            -1,
        );

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        // U8 joints for <=256 bones
        match &acc.component_type {
            Valid(json::accessor::GenericComponentType(ct)) => {
                assert!(
                    matches!(ct, json::accessor::ComponentType::U8),
                    "expected U8, got {:?}",
                    ct
                );
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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            true,
            300,
            -1,
        );

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        // U16 joints for >256 bones
        match &acc.component_type {
            Valid(json::accessor::GenericComponentType(ct)) => {
                assert!(
                    matches!(ct, json::accessor::ComponentType::U16),
                    "expected U16, got {:?}",
                    ct
                );
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

        let prim = create_primitive(
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            true,
            10,
            -1,
        );

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
            &verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            true,
            10,
            5, // rigid_bone_index = 5
        );

        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        let offset = view.byte_offset.unwrap().0 as usize;

        assert_eq!(buf[offset], 5, "rigid vertex should use section rigid bone");
    }

    // ---- Material export ----

    #[test]
    fn test_material_names_and_textures_exported() {
        use crate::types::*;
        use crate::univert_packer::{UnivertPacker, MAX_UV};
        use crate::vertex_element::VertexElementType;

        // Build a minimal geometry with materials
        let packer = UnivertPacker {
            pack_order: "P".to_string(),
            decl_order: "P".to_string(),
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Ignore,
            basis_scale_type: VertexElementType::Ignore,
            tangent_type: VertexElementType::Ignore,
            normal_type: VertexElementType::Ignore,
            uv_types: [VertexElementType::Ignore; MAX_UV],
            indices_type: VertexElementType::Ignore,
            weights_type: VertexElementType::Ignore,
            diffuse_type: VertexElementType::Ignore,
            index_type: VertexElementType::Ignore,
        };

        let mut vb = Vec::new();
        for pos in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            let v = UnpackedVertex {
                position: pos,
                ..Default::default()
            };
            packer.pack_vertex(&mut vb, &v).unwrap();
        }

        let geom = UgxGeom {
            bounding_sphere: Sphere {
                center: [0.0; 3],
                radius: 1.0,
            },
            bounds: AABB {
                min: [0.0; 3],
                max: [1.0; 3],
            },
            materials: vec![
                Material {
                    name: "grass_mat".to_string(),
                    spec_power: 40.0,
                    flags: 0,
                    blend_type: 0,
                    opacity: 1.0,
                    maps: {
                        let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                        maps[MapType::Diffuse as usize] = vec![Map {
                            name: "art/grass_diff.ddx".to_string(),
                            channel: 0,
                            flags: 7,
                        }];
                        maps[MapType::Normal as usize] = vec![Map {
                            name: "art/grass_norm.ddx".to_string(),
                            channel: 0,
                            flags: 7,
                        }];
                        maps
                    },
                    uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
                },
                Material {
                    name: "glass_mat".to_string(),
                    spec_power: 80.0,
                    flags: 0,
                    blend_type: 1,
                    opacity: 0.5,
                    maps: {
                        let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                        maps[MapType::Diffuse as usize] = vec![Map {
                            name: "art/glass_diff.ddx".to_string(),
                            channel: 0,
                            flags: 7,
                        }];
                        maps[MapType::Emissive as usize] = vec![Map {
                            name: "art/glass_emit.ddx".to_string(),
                            channel: 1,
                            flags: 3,
                        }];
                        maps
                    },
                    uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
                },
            ],
            bones: Vec::new(),
            granny_bones: Vec::new(),
            granny_meshes: Vec::new(),
            bone_bounds: Vec::new(),
            sections: vec![Section {
                material_index: 0,
                accessory_index: -1,
                max_bones: 0,
                rigid_bone_index: -1,
                ib_offset: 0,
                num_tris: 1,
                vb_offset: 0,
                vb_bytes: vb.len() as i32,
                vert_size: packer.vertex_size() as i32,
                num_verts: 3,
                base_vert_packer: packer,
                bone_remap: Vec::new(),
                rigid_only: true,
                global_bones: false,
            }],
            vertex_buffer: vb,
            index_buffer: vec![0, 1, 2],
            rigid_only: true,
            rigid_bone_index: -1,
            all_sections_rigid: true,
            all_sections_skinned: false,
            global_bones: false,
        };

        let options = GltfExportOptions {
            embed_buffers: true,
            include_materials: true,
            include_skeleton: false,
        };
        let export = export_to_gltf(&geom, &options).unwrap();
        let root: json::Root = serde_json::from_str(&export.json).unwrap();

        // Verify material count and names
        assert_eq!(root.materials.len(), 2);
        assert_eq!(root.materials[0].name, Some("grass_mat".to_string()));
        assert_eq!(root.materials[1].name, Some("glass_mat".to_string()));

        // Verify first material is opaque
        assert_eq!(
            root.materials[0].alpha_mode,
            Valid(json::material::AlphaMode::Opaque)
        );

        // Verify second material uses blend (blend_type=1, opacity=0.5)
        assert_eq!(
            root.materials[1].alpha_mode,
            Valid(json::material::AlphaMode::Blend)
        );

        // Verify textures were created (4 unique textures)
        assert_eq!(root.textures.len(), 4);
        assert_eq!(root.images.len(), 4);

        // Verify image URIs
        let image_uris: Vec<_> = root
            .images
            .iter()
            .map(|img| img.uri.as_deref().unwrap())
            .collect();
        assert!(image_uris.contains(&"art/grass_diff.ddx"));
        assert!(image_uris.contains(&"art/grass_norm.ddx"));
        assert!(image_uris.contains(&"art/glass_diff.ddx"));
        assert!(image_uris.contains(&"art/glass_emit.ddx"));

        // Verify first material has diffuse and normal textures
        assert!(root.materials[0]
            .pbr_metallic_roughness
            .base_color_texture
            .is_some());
        assert!(root.materials[0].normal_texture.is_some());
        assert!(root.materials[0].emissive_texture.is_none());

        // Verify second material has diffuse and emissive textures
        assert!(root.materials[1]
            .pbr_metallic_roughness
            .base_color_texture
            .is_some());
        assert!(root.materials[1].normal_texture.is_none());
        assert!(root.materials[1].emissive_texture.is_some());

        // Verify emissive factor is [1,1,1] when emissive texture present
        assert_eq!(root.materials[1].emissive_factor.0, [1.0, 1.0, 1.0]);
        // And [0,0,0] when no emissive texture
        assert_eq!(root.materials[0].emissive_factor.0, [0.0, 0.0, 0.0]);

        // Verify emissive texture uses UV channel 1
        let emit_info = root.materials[1].emissive_texture.as_ref().unwrap();
        assert_eq!(emit_info.tex_coord, 1);
    }
}
