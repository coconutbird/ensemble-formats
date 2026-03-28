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
                MaterialData::Hogan(Box::new(hogan))
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
    use crate::extras::MaterialExtrasJson;

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

    let ext: MaterialExtrasJson = match serde_json::from_str(raw.get()) {
        Ok(v) => v,
        Err(_) => return result,
    };

    result.material_version = Some(ext.ugx_material_version);
    result.flags = ext.ugx_flags.unwrap_or(0);
    result.blend_type = ext.ugx_blend_type;
    result.spec_power = ext.ugx_spec_power;
    result.spec_color = ext.ugx_spec_color;
    result.env_reflectivity = ext.ugx_env_reflectivity;
    result.env_sharpness = ext.ugx_env_sharpness;
    result.env_fresnel = ext.ugx_env_fresnel;
    result.env_fresnel_power = ext.ugx_env_fresnel_power;
    result.accessory_index = ext.ugx_accessory_index;
    result.opacity = ext.ugx_opacity;

    // UVW velocity
    if let Some(uvw) = ext.ugx_uvw_velocity {
        for (i, v) in uvw.iter().enumerate() {
            if i >= MapType::NUM_TYPES {
                break;
            }
            result.uvw_velocity[i] = *v;
        }
    }

    // Maps
    if let Some(maps) = ext.ugx_maps {
        for map_type in MapType::ALL {
            if let Some(entries) = maps.get(map_type.name()) {
                let map_vec: Vec<Map> = entries
                    .iter()
                    .map(|e| Map {
                        name: e.name.clone(),
                        channel: e.channel,
                        flags: e.flags,
                    })
                    .collect();
                if !map_vec.is_empty() {
                    result.extra_maps.push((map_type as usize, map_vec));
                }
            }
        }
    }

    // Hogan
    if let Some(h) = ext.ugx_hogan {
        use crate::extras::params_to_cb_bytes;
        result.hogan = Some(HoganMaterialData {
            shader_permutations: h
                .shader_permutations
                .into_iter()
                .map(|p| ShaderPermutation {
                    name: p.name,
                    hash: p.hash,
                })
                .collect(),
            ufx_version: h.ufx_version,
            blend_mode: h.blend_mode,
            shadow_requires_consts: h.shadow_requires_consts,
            skinned: h.skinned,
            terrain_blending: h.terrain_blending,
            vs_cb_data: params_to_cb_bytes(&h.vs_params),
            ps_cb_data: params_to_cb_bytes(&h.ps_params),
            hs_cb_data: params_to_cb_bytes(&h.hs_params),
            ds_cb_data: params_to_cb_bytes(&h.ds_params),
            gs_cb_data: params_to_cb_bytes(&h.gs_params),
            textures: h.textures,
        });
    }

    result
}
