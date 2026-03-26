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

mod primitive;
mod skeleton;

use base64::{Engine, engine::general_purpose::STANDARD};
use gltf_json as json;
use json::validation::Checked::Valid;

use ugx::{MapType, Material, Result, UgxGeom};

use primitive::create_primitive;
use skeleton::{
    build_section_to_mesh_mapping, create_skeleton_nodes, create_skeleton_nodes_from_granny,
};

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

    // Always store flags and blend_type (even if 0, for roundtrip fidelity)
    extras.insert(
        "ugx_flags".into(),
        serde_json::Value::Number(mat.flags.into()),
    );
    extras.insert(
        "ugx_blend_type".into(),
        serde_json::Value::Number(mat.blend_type.into()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ugx::UnpackedVertex;

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

    /// Helper: get the accessor for a semantic from the primitive returned by create_primitive.
    fn get_accessor<'a>(
        prim: &json::mesh::Primitive,
        accessors: &'a [json::Accessor],
        semantic: json::mesh::Semantic,
    ) -> Option<&'a json::Accessor> {
        prim.attributes
            .get(&Valid(semantic))
            .map(|idx| &accessors[idx.value()])
    }

    /// Helper: read 3 consecutive f32s from a buffer at the given offset.
    fn read_f32x3(buf: &[u8], offset: usize) -> [f32; 3] {
        [
            f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap()),
            f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap()),
            f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap()),
        ]
    }

    /// Helper: read 4 consecutive f32s from a buffer at the given offset.
    fn read_f32x4(buf: &[u8], offset: usize) -> [f32; 4] {
        [
            f32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap()),
            f32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap()),
            f32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap()),
            f32::from_le_bytes(buf[offset + 12..offset + 16].try_into().unwrap()),
        ]
    }

    /// Helper: get byte offset of an accessor's buffer view.
    fn accessor_offset(
        prim: &json::mesh::Primitive,
        accessors: &[json::Accessor],
        views: &[json::buffer::View],
        semantic: json::mesh::Semantic,
    ) -> usize {
        let acc = get_accessor(prim, accessors, semantic).unwrap();
        let view = &views[acc.buffer_view.unwrap().value()];
        view.byte_offset.unwrap().0 as usize
    }

    /// Helper: invoke create_primitive with common defaults.
    fn make_prim(
        verts: &[UnpackedVertex],
        has_skin: bool,
        bone_count: usize,
        rigid_bone_index: i32,
    ) -> (
        json::mesh::Primitive,
        Vec<u8>,
        Vec<json::Accessor>,
        Vec<json::buffer::View>,
    ) {
        let indices: Vec<u16> = vec![0; verts.len().max(3)];
        let mut buf = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();

        let prim = create_primitive(
            verts,
            &indices,
            -1,
            &mut buf,
            &mut accessors,
            &mut views,
            false,
            has_skin,
            bone_count,
            rigid_bone_index,
        );
        (prim, buf, accessors, views)
    }

    // ---- UV set count ----

    #[test]
    fn test_uv_set_count() {
        // 0 UVs → no TEXCOORD attributes
        let verts_0 = vec![vertex([0.0, 0.0, 0.0]); 3];
        let (prim, ..) = make_prim(&verts_0, false, 0, -1);
        assert!(!has_semantic(&prim, json::mesh::Semantic::TexCoords(0)));

        // 1 UV → only TEXCOORD_0
        let mut verts_1 = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts_1 {
            v.texcoords[0] = [0.5, 0.5];
            v.num_texcoords = 1;
        }
        let (prim, ..) = make_prim(&verts_1, false, 0, -1);
        assert!(has_semantic(&prim, json::mesh::Semantic::TexCoords(0)));
        assert!(!has_semantic(&prim, json::mesh::Semantic::TexCoords(1)));

        // 3 UVs → TEXCOORD_0..2, each Vec2/F32
        let mut verts_3 = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts_3 {
            v.texcoords[0] = [0.1, 0.2];
            v.texcoords[1] = [0.3, 0.4];
            v.texcoords[2] = [0.5, 0.6];
            v.num_texcoords = 3;
        }
        let (prim, _, accessors, _) = make_prim(&verts_3, false, 0, -1);
        for i in 0..3 {
            let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::TexCoords(i)).unwrap();
            assert_eq!(acc.type_, Valid(json::accessor::Type::Vec2));
            assert_eq!(acc.count, json::validation::USize64(3));
        }
        assert!(!has_semantic(&prim, json::mesh::Semantic::TexCoords(3)));
    }

    // ---- Tangent presence and normalization ----

    #[test]
    fn test_tangent_presence() {
        // Zero tangent → no TANGENT attribute
        let verts_zero = vec![vertex([0.0, 0.0, 0.0]); 3];
        let (prim, ..) = make_prim(&verts_zero, false, 0, -1);
        assert!(!has_semantic(&prim, json::mesh::Semantic::Tangents));

        // Non-zero tangent → TANGENT attribute present as Vec4
        let mut verts_set = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts_set {
            v.tangent = [1.0, 0.0, 0.0, 1.0];
        }
        let (prim, _, accessors, _) = make_prim(&verts_set, false, 0, -1);
        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Tangents).unwrap();
        assert_eq!(acc.type_, Valid(json::accessor::Type::Vec4));
    }

    #[test]
    fn test_tangent_normalization_and_handedness() {
        // Sub-unit tangent with positive handedness → normalized to unit length, w=1
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].tangent = [0.5, 0.0, 0.0, 1.0];
        let (prim, buf, accessors, views) = make_prim(&verts, false, 0, -1);

        let off = accessor_offset(&prim, &accessors, &views, json::mesh::Semantic::Tangents);
        let [tx, ty, tz, tw] = read_f32x4(&buf, off);
        let length = (tx * tx + ty * ty + tz * tz).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "should be unit length, got {length}"
        );
        assert!((tx - 1.0).abs() < 1e-5);
        assert_eq!(tw, 1.0, "positive handedness preserved");

        // Negative handedness → w=-1 preserved
        let mut verts_neg = vec![vertex([0.0, 0.0, 0.0])];
        verts_neg[0].tangent = [0.0, 0.0, 0.5, -1.0];
        let (prim, buf, accessors, views) = make_prim(&verts_neg, false, 0, -1);

        let off = accessor_offset(&prim, &accessors, &views, json::mesh::Semantic::Tangents);
        let [_, _, _, tw] = read_f32x4(&buf, off);
        assert_eq!(tw, -1.0, "negative handedness preserved");
    }

    // ---- Normal normalization ----

    #[test]
    fn test_normal_normalization_and_zero_fallback() {
        // Non-unit normal (2,0,0) → normalized to (1,0,0)
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].normal = [2.0, 0.0, 0.0];
        let (prim, buf, accessors, views) = make_prim(&verts, false, 0, -1);

        let off = accessor_offset(&prim, &accessors, &views, json::mesh::Semantic::Normals);
        let [nx, ny, nz] = read_f32x3(&buf, off);
        let length = (nx * nx + ny * ny + nz * nz).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "should be unit length, got {length}"
        );
        assert!((nx - 1.0).abs() < 1e-5);

        // Zero normal → falls back to up (0,1,0)
        let mut verts_zero = vec![vertex([0.0, 0.0, 0.0])];
        verts_zero[0].normal = [0.0, 0.0, 0.0];
        let (prim, buf, accessors, views) = make_prim(&verts_zero, false, 0, -1);

        let off = accessor_offset(&prim, &accessors, &views, json::mesh::Semantic::Normals);
        let [nx, ny, nz] = read_f32x3(&buf, off);
        assert!((nx).abs() < 1e-5);
        assert!(
            (ny - 1.0).abs() < 1e-5,
            "zero normal should fall back to up"
        );
        assert!((nz).abs() < 1e-5);
    }

    // ---- Joint encoding ----

    #[test]
    fn test_joint_component_type_by_bone_count() {
        let mut verts = vec![vertex([0.0, 0.0, 0.0]); 3];
        for v in &mut verts {
            v.bone_indices = [1, 2, 0, 0];
            v.bone_weights = [0.7, 0.3, 0.0, 0.0];
        }

        // ≤256 bones → U8
        let (prim, _, accessors, _) = make_prim(&verts, true, 50, -1);
        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        match &acc.component_type {
            Valid(json::accessor::GenericComponentType(ct)) => {
                assert!(
                    matches!(ct, json::accessor::ComponentType::U8),
                    "≤256 bones should use U8, got {ct:?}"
                );
            }
            other => panic!("unexpected: {other:?}"),
        }

        // >256 bones → U16
        let (prim, _, accessors, _) = make_prim(&verts, true, 300, -1);
        let acc = get_accessor(&prim, &accessors, json::mesh::Semantic::Joints(0)).unwrap();
        match &acc.component_type {
            Valid(json::accessor::GenericComponentType(ct)) => {
                assert!(
                    matches!(ct, json::accessor::ComponentType::U16),
                    ">256 bones should use U16, got {ct:?}"
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_joints_1based_to_0based() {
        // UGX uses 1-based bone indices; glTF expects 0-based
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].bone_indices = [3, 1, 0, 0]; // 1-based: bone 2, bone 0, none, none
        verts[0].bone_weights = [0.8, 0.2, 0.0, 0.0];
        let (prim, buf, accessors, views) = make_prim(&verts, true, 10, -1);

        let off = accessor_offset(&prim, &accessors, &views, json::mesh::Semantic::Joints(0));
        assert_eq!(
            [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]],
            [2, 0, 0, 0]
        );
    }

    #[test]
    fn test_rigid_vertex_gets_rigid_bone() {
        // Zero-weight vertex = rigid, should get section's rigid_bone_index
        let mut verts = vec![vertex([0.0, 0.0, 0.0])];
        verts[0].bone_weights = [0.0, 0.0, 0.0, 0.0];
        let (prim, buf, accessors, views) = make_prim(&verts, true, 10, 5);

        let off = accessor_offset(&prim, &accessors, &views, json::mesh::Semantic::Joints(0));
        assert_eq!(buf[off], 5, "rigid vertex should use section rigid bone");
    }
}
