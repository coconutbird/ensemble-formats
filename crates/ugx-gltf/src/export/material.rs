//! Material and texture building for glTF export.
//!
//! Extracts UGX material data into glTF material objects with PBR
//! metallic-roughness workflow and UGX-specific extras for lossless roundtrip.

use gltf_json as json;
use json::validation::Checked::Valid;

use ugx::types::MaterialData;
use ugx::{MapType, Material};

use crate::extras::{
    HoganExtrasJson, MapEntryJson, MaterialExtrasJson, ShaderPermJson, cb_bytes_to_params,
};

/// Build glTF material extras JSON for UGX-specific data.
///
/// Stores material flags, UVW velocity, and non-PBR texture maps
/// so they survive a glTF roundtrip.
pub(super) fn build_material_extras(mat: &Material) -> json::Extras {
    let mut ext = MaterialExtrasJson {
        ugx_material_version: mat.material_version,
        ..Default::default()
    };

    match &mat.data {
        MaterialData::Legacy(legacy) => {
            ext.ugx_flags = Some(legacy.flags);
            ext.ugx_blend_type = Some(legacy.blend_type);
            ext.ugx_spec_power = Some(legacy.spec_power);
            ext.ugx_spec_color = Some(legacy.spec_color);
            ext.ugx_env_reflectivity = Some(legacy.env_reflectivity);
            ext.ugx_env_sharpness = Some(legacy.env_sharpness);
            ext.ugx_env_fresnel = Some(legacy.env_fresnel);
            ext.ugx_env_fresnel_power = Some(legacy.env_fresnel_power);
            ext.ugx_accessory_index = Some(legacy.accessory_index);
            ext.ugx_opacity = Some(legacy.opacity);

            let has_any_uvw = legacy
                .uvw_velocity
                .iter()
                .any(|v| v[0] != 0.0 || v[1] != 0.0 || v[2] != 0.0);
            if has_any_uvw {
                ext.ugx_uvw_velocity = Some(legacy.uvw_velocity.to_vec());
            }

            let mut maps = std::collections::BTreeMap::new();
            for map_type in MapType::ALL {
                let idx = map_type as usize;
                if !legacy.maps[idx].is_empty() {
                    let entries: Vec<MapEntryJson> = legacy.maps[idx]
                        .iter()
                        .map(|m| MapEntryJson {
                            name: m.name.clone(),
                            channel: m.channel,
                            flags: m.flags,
                        })
                        .collect();
                    maps.insert(map_type.name().to_string(), entries);
                }
            }
            if !maps.is_empty() {
                ext.ugx_maps = Some(maps);
            }
        }
        MaterialData::Hogan(hogan) => {
            ext.ugx_hogan = Some(HoganExtrasJson {
                shader_permutations: hogan
                    .shader_permutations
                    .iter()
                    .map(|p| ShaderPermJson {
                        name: p.name.clone(),
                        hash: p.hash,
                    })
                    .collect(),
                ufx_version: hogan.ufx_version,
                blend_mode: hogan.blend_mode,
                shadow_requires_consts: hogan.shadow_requires_consts,
                skinned: hogan.skinned,
                terrain_blending: hogan.terrain_blending,
                vs_params: cb_bytes_to_params(&hogan.vs_cb_data),
                ps_params: cb_bytes_to_params(&hogan.ps_cb_data),
                hs_params: cb_bytes_to_params(&hogan.hs_cb_data),
                ds_params: cb_bytes_to_params(&hogan.ds_cb_data),
                gs_params: cb_bytes_to_params(&hogan.gs_cb_data),
                textures: hogan.textures.clone(),
            });
        }
    }

    crate::extras::to_raw_value(&ext)
}

/// Result of building glTF materials from UGX material data.
pub(super) struct MaterialBuildResult {
    pub materials: Vec<json::Material>,
    pub images: Vec<json::Image>,
    pub textures: Vec<json::Texture>,
}

/// Build glTF materials, images, and textures from UGX materials.
pub(super) fn build_materials(materials: &[Material]) -> MaterialBuildResult {
    let mut images_json: Vec<json::Image> = Vec::new();
    let mut textures_json: Vec<json::Texture> = Vec::new();
    let mut materials_json = Vec::new();

    // Pass 1: Build texture registry (deduplicated image/texture objects)
    let mut texture_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for mat in materials {
        if let Some(legacy) = mat.legacy() {
            for map_type in MapType::ALL {
                for map in &legacy.maps[map_type as usize] {
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
    }

    // Pass 2: Create glTF materials with texture references
    for mat in materials {
        let (base_color_texture, normal_texture, occlusion_texture, emissive_texture) =
            if let Some(legacy) = mat.legacy() {
                let bct = legacy.maps[MapType::Diffuse as usize]
                    .first()
                    .filter(|m| !m.name.is_empty())
                    .map(|m| json::texture::Info {
                        index: json::Index::new(texture_map[&m.name]),
                        tex_coord: m.channel as u32,
                        extensions: None,
                        extras: json::Extras::default(),
                    });
                let nt = legacy.maps[MapType::Normal as usize]
                    .first()
                    .filter(|m| !m.name.is_empty())
                    .map(|m| json::material::NormalTexture {
                        index: json::Index::new(texture_map[&m.name]),
                        scale: 1.0,
                        tex_coord: m.channel as u32,
                        extensions: None,
                        extras: json::Extras::default(),
                    });
                let ot = legacy.maps[MapType::AO as usize]
                    .first()
                    .filter(|m| !m.name.is_empty())
                    .map(|m| json::material::OcclusionTexture {
                        index: json::Index::new(texture_map[&m.name]),
                        strength: json::material::StrengthFactor(1.0),
                        tex_coord: m.channel as u32,
                        extensions: None,
                        extras: json::Extras::default(),
                    });
                let et = legacy.maps[MapType::Emissive as usize]
                    .first()
                    .filter(|m| !m.name.is_empty())
                    .map(|m| json::texture::Info {
                        index: json::Index::new(texture_map[&m.name]),
                        tex_coord: m.channel as u32,
                        extensions: None,
                        extras: json::Extras::default(),
                    });
                (bct, nt, ot, et)
            } else {
                (None, None, None, None)
            };

        let emissive_factor = if emissive_texture.is_some() {
            json::material::EmissiveFactor([1.0, 1.0, 1.0])
        } else {
            json::material::EmissiveFactor([0.0, 0.0, 0.0])
        };

        let (blend_type, opacity, spec_power) = if let Some(legacy) = mat.legacy() {
            (legacy.blend_type, legacy.opacity, legacy.spec_power)
        } else {
            (0u8, 1.0f32, 10.0f32)
        };

        let alpha_mode = if blend_type > 0 || opacity < 1.0 {
            Valid(json::material::AlphaMode::Blend)
        } else {
            Valid(json::material::AlphaMode::Opaque)
        };

        let pbr = json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, opacity]),
            base_color_texture,
            metallic_factor: json::material::StrengthFactor(0.0),
            roughness_factor: json::material::StrengthFactor(
                1.0 - (spec_power / 100.0).clamp(0.0, 1.0),
            ),
            metallic_roughness_texture: None,
            extensions: None,
            extras: json::Extras::default(),
        };

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

    MaterialBuildResult {
        materials: materials_json,
        images: images_json,
        textures: textures_json,
    }
}
