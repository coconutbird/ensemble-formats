//! Material import from glTF.

use ugx::{Map, MapType, Material};

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

            // Read UGX extras (flags, blend_type, uvw_velocity, non-PBR maps)
            let (flags, extras_blend_type, uvw_velocity, extra_maps) =
                read_material_extras(&mat.extras, &maps);

            // Use blend_type from extras if present, otherwise infer from alpha mode
            let blend_type = extras_blend_type.unwrap_or({
                if let gltf_json::validation::Checked::Valid(
                    gltf_json::material::AlphaMode::Blend,
                ) = mat.alpha_mode
                {
                    1
                } else {
                    0
                }
            });

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
                ..Material::default()
            }
        })
        .collect()
}

/// Read UGX material extras from glTF extras JSON.
///
/// Returns (flags, blend_type, uvw_velocity, extra_maps) where extra_maps is a vec of
/// (map_type_index, Vec<Map>) for non-PBR map types. blend_type is None if not
/// present in extras (caller should fall back to alpha_mode heuristic).
#[allow(clippy::type_complexity)]
fn read_material_extras(
    extras: &gltf_json::Extras,
    _existing_maps: &[Vec<Map>; MapType::NUM_TYPES],
) -> (
    u32,
    Option<u8>,
    [[f32; 3]; MapType::NUM_TYPES],
    Vec<(usize, Vec<Map>)>,
) {
    let mut flags = 0u32;
    let mut blend_type: Option<u8> = None;
    let mut uvw_velocity = [[0.0f32; 3]; MapType::NUM_TYPES];
    let mut extra_maps: Vec<(usize, Vec<Map>)> = Vec::new();

    let raw = match extras {
        Some(raw_value) => raw_value,
        None => return (flags, blend_type, uvw_velocity, extra_maps),
    };

    let parsed: serde_json::Value = match serde_json::from_str(raw.get()) {
        Ok(v) => v,
        Err(_) => return (flags, blend_type, uvw_velocity, extra_maps),
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return (flags, blend_type, uvw_velocity, extra_maps),
    };

    // Read flags
    if let Some(v) = obj.get("ugx_flags") {
        flags = v.as_u64().unwrap_or(0) as u32;
    }

    // Read blend_type
    if let Some(v) = obj.get("ugx_blend_type") {
        blend_type = Some(v.as_u64().unwrap_or(0) as u8);
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
                uvw_velocity[i][0] = v[0].as_f64().unwrap_or(0.0) as f32;
                uvw_velocity[i][1] = v[1].as_f64().unwrap_or(0.0) as f32;
                uvw_velocity[i][2] = v[2].as_f64().unwrap_or(0.0) as f32;
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

    (flags, blend_type, uvw_velocity, extra_maps)
}
