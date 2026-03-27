//! Material import from glTF.

use ugx::{
    HoganMaterialData, LegacyMaterialData, Map, MapType, Material, MaterialData, ShaderPermutation,
};

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
pub(crate) fn import_materials(root: &gltf_json::Root) -> Vec<Material> {
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

            // Read UGX extras (all material properties + maps)
            let mat_extras = read_material_extras(&mat.extras, &maps);

            // Use blend_type from extras if present, otherwise infer from alpha mode
            let blend_type = mat_extras.blend_type.unwrap_or({
                if let gltf_json::validation::Checked::Valid(
                    gltf_json::material::AlphaMode::Blend,
                ) = mat.alpha_mode
                {
                    1
                } else {
                    0
                }
            });

            // Merge extra maps into the maps array (extras override PBR-derived maps)
            let mut final_maps = maps;
            for (idx, extra) in mat_extras.extra_maps {
                final_maps[idx] = extra;
            }

            let data = if let Some(hogan) = mat_extras.hogan {
                MaterialData::Hogan(hogan)
            } else {
                MaterialData::Legacy(Box::new(LegacyMaterialData {
                    maps: final_maps,
                    spec_power: mat_extras.spec_power.unwrap_or((1.0 - roughness) * 100.0),
                    opacity: mat_extras.opacity.unwrap_or(base_color[3]),
                    blend_type,
                    flags: mat_extras.flags,
                    uvw_velocity: mat_extras.uvw_velocity,
                    spec_color: mat_extras.spec_color.unwrap_or([1.0, 1.0, 1.0]),
                    env_reflectivity: mat_extras.env_reflectivity.unwrap_or(1.0),
                    env_sharpness: mat_extras.env_sharpness.unwrap_or(1.0),
                    env_fresnel: mat_extras.env_fresnel.unwrap_or(0.5),
                    env_fresnel_power: mat_extras.env_fresnel_power.unwrap_or(4.0),
                    accessory_index: mat_extras.accessory_index.unwrap_or(0),
                }))
            };

            Material {
                name: mat.name.clone().unwrap_or_default(),
                material_version: mat_extras.material_version.unwrap_or(4),
                data,
            }
        })
        .collect()
}

/// Parsed material extras from glTF.
struct MaterialExtras {
    flags: u32,
    blend_type: Option<u8>,
    uvw_velocity: [[f32; 3]; MapType::NUM_TYPES],
    extra_maps: Vec<(usize, Vec<Map>)>,
    spec_power: Option<f32>,
    spec_color: Option<[f32; 3]>,
    env_reflectivity: Option<f32>,
    env_sharpness: Option<f32>,
    env_fresnel: Option<f32>,
    env_fresnel_power: Option<f32>,
    accessory_index: Option<u32>,
    opacity: Option<f32>,
    material_version: Option<u32>,
    hogan: Option<HoganMaterialData>,
}

/// Read UGX material extras from glTF extras JSON.
fn read_material_extras(
    extras: &gltf_json::Extras,
    _existing_maps: &[Vec<Map>; MapType::NUM_TYPES],
) -> MaterialExtras {
    let mut result = MaterialExtras {
        flags: 0,
        blend_type: None,
        uvw_velocity: [[0.0f32; 3]; MapType::NUM_TYPES],
        extra_maps: Vec::new(),
        spec_power: None,
        spec_color: None,
        env_reflectivity: None,
        env_sharpness: None,
        env_fresnel: None,
        env_fresnel_power: None,
        accessory_index: None,
        opacity: None,
        material_version: None,
        hogan: None,
    };

    let raw = match extras {
        Some(raw_value) => raw_value,
        None => return result,
    };

    let parsed: serde_json::Value = match serde_json::from_str(raw.get()) {
        Ok(v) => v,
        Err(_) => return result,
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return result,
    };

    // Read flags
    if let Some(v) = obj.get("ugx_flags") {
        result.flags = v.as_u64().unwrap_or(0) as u32;
    }

    // Read blend_type
    if let Some(v) = obj.get("ugx_blend_type") {
        result.blend_type = Some(v.as_u64().unwrap_or(0) as u8);
    }

    // Read material properties
    if let Some(v) = obj.get("ugx_spec_power") {
        result.spec_power = Some(v.as_f64().unwrap_or(10.0) as f32);
    }
    if let Some(serde_json::Value::Array(arr)) = obj.get("ugx_spec_color")
        && arr.len() >= 3
    {
        result.spec_color = Some([
            arr[0].as_f64().unwrap_or(1.0) as f32,
            arr[1].as_f64().unwrap_or(1.0) as f32,
            arr[2].as_f64().unwrap_or(1.0) as f32,
        ]);
    }
    if let Some(v) = obj.get("ugx_env_reflectivity") {
        result.env_reflectivity = Some(v.as_f64().unwrap_or(1.0) as f32);
    }
    if let Some(v) = obj.get("ugx_env_sharpness") {
        result.env_sharpness = Some(v.as_f64().unwrap_or(1.0) as f32);
    }
    if let Some(v) = obj.get("ugx_env_fresnel") {
        result.env_fresnel = Some(v.as_f64().unwrap_or(0.5) as f32);
    }
    if let Some(v) = obj.get("ugx_env_fresnel_power") {
        result.env_fresnel_power = Some(v.as_f64().unwrap_or(4.0) as f32);
    }
    if let Some(v) = obj.get("ugx_accessory_index") {
        result.accessory_index = Some(v.as_u64().unwrap_or(0) as u32);
    }
    if let Some(v) = obj.get("ugx_opacity") {
        result.opacity = Some(v.as_f64().unwrap_or(1.0) as f32);
    }

    // Read material version
    if let Some(v) = obj.get("ugx_material_version") {
        result.material_version = Some(v.as_u64().unwrap_or(4) as u32);
    }

    // Read Hogan material data (HW2)
    if let Some(serde_json::Value::Object(hogan_obj)) = obj.get("ugx_hogan") {
        result.hogan = Some(parse_hogan_extras(hogan_obj));
    }

    // Read UVW velocity
    if let Some(serde_json::Value::Array(arr)) = obj.get("ugx_uvw_velocity") {
        for (i, val) in arr.iter().enumerate() {
            if i >= MapType::NUM_TYPES {
                break;
            }
            if let serde_json::Value::Array(v) = val
                && v.len() >= 3
            {
                result.uvw_velocity[i][0] = v[0].as_f64().unwrap_or(0.0) as f32;
                result.uvw_velocity[i][1] = v[1].as_f64().unwrap_or(0.0) as f32;
                result.uvw_velocity[i][2] = v[2].as_f64().unwrap_or(0.0) as f32;
            }
        }
    }

    // Read maps (all types — extras override PBR-derived maps for flag fidelity)
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
                    result.extra_maps.push((map_type as usize, map_vec));
                }
            }
        }
    }

    result
}

/// Parse HW2 Hogan material data from glTF extras JSON.
fn parse_hogan_extras(obj: &serde_json::Map<String, serde_json::Value>) -> HoganMaterialData {
    let mut perms = Vec::new();
    if let Some(serde_json::Value::Array(arr)) = obj.get("shader_permutations") {
        for entry in arr {
            if let serde_json::Value::Object(p) = entry {
                let name = p
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let hash = p.get("hash").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                perms.push(ShaderPermutation { name, hash });
            }
        }
    }

    HoganMaterialData {
        shader_permutations: perms,
        ufx_version: obj.get("ufx_version").and_then(|v| v.as_u64()).unwrap_or(9) as u32,
        blend_mode: obj.get("blend_mode").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        shadow_requires_consts: obj
            .get("shadow_requires_consts")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        skinned: obj
            .get("skinned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        terrain_blending: obj
            .get("terrain_blending")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        vs_cb_data: obj.get("vs_cb_data").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        ps_cb_data: obj.get("ps_cb_data").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        textures: obj
            .get("textures")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}
