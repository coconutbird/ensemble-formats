//! Material and texture building for glTF export.
//!
//! Extracts UGX material data into glTF material objects with PBR
//! metallic-roughness workflow and UGX-specific extras for lossless roundtrip.

use gltf_json as json;
use json::validation::Checked::Valid;

use ugx::types::MaterialData;
use ugx::types::material::{BlendType, material_flags};
use ugx::{Error, HoganMaterialData, LegacyMaterialData, Map, MapType, Material, Result};

use crate::extras::{
    HoganExtrasJson, MapEntryJson, MaterialExtrasJson, ShaderPermJson, cb_bytes_to_named,
    cb_bytes_to_params,
};
use crate::hogan_cb_layout;

type TextureRegistry = std::collections::HashMap<String, u32>;

struct TextureReferences {
    base_color: Option<json::texture::Info>,
    normal: Option<json::material::NormalTexture>,
    occlusion: Option<json::material::OcclusionTexture>,
    emissive: Option<json::texture::Info>,
}

/// Build glTF material extras JSON for UGX-specific data.
///
/// Stores material flags, UVW velocity, and non-PBR texture maps
/// so they survive a glTF roundtrip.
pub(super) fn build_material_extras(material: &Material) -> json::Extras {
    let mut extras = MaterialExtrasJson {
        material_version: material.material_version,
        ..Default::default()
    };
    match &material.data {
        MaterialData::Legacy(legacy) => populate_legacy_extras(&mut extras, legacy),
        MaterialData::Hogan(hogan) => extras.hogan = Some(build_hogan_extras(hogan)),
    }
    crate::extras::to_raw_value(&extras)
}

fn populate_legacy_extras(extras: &mut MaterialExtrasJson, legacy: &LegacyMaterialData) {
    let uses_opacity = legacy.flags & material_flags::OPACITY_VALID != 0;
    let flags_without_two_sided = legacy.flags & !material_flags::TWO_SIDED;
    if flags_without_two_sided != 0 {
        extras.flags = Some(flags_without_two_sided);
    }
    // glTF alphaMode cannot distinguish every UGX blend mode (notably
    // additive from over), so retain the raw byte for lossless round-trips.
    extras.blend_type = Some(legacy.blend_type);
    if !uses_opacity {
        extras.opacity = Some(legacy.opacity);
    }
    extras.spec_power = Some(legacy.spec_power);
    extras.spec_color = Some(legacy.spec_color);
    extras.env_reflectivity = Some(legacy.env_reflectivity);
    extras.env_sharpness = Some(legacy.env_sharpness);
    extras.env_fresnel = Some(legacy.env_fresnel);
    extras.env_fresnel_power = Some(legacy.env_fresnel_power);
    extras.accessory_index = Some(legacy.accessory_index);

    if legacy
        .uvw_velocity
        .iter()
        .flatten()
        .any(|value| value.abs() > f32::EPSILON)
    {
        extras.uvw_velocity = Some(legacy.uvw_velocity.to_vec());
    }
    let maps: std::collections::BTreeMap<_, _> = MapType::ALL
        .into_iter()
        .filter_map(|map_type| {
            let entries: Vec<_> = legacy.maps[map_type as usize]
                .iter()
                .map(|map| MapEntryJson {
                    name: map.name.clone(),
                    channel: map.channel,
                    flags: map.flags,
                })
                .collect();
            (!entries.is_empty()).then(|| (map_type.name().to_string(), entries))
        })
        .collect();
    if !maps.is_empty() {
        extras.maps = Some(maps);
    }
}

fn build_hogan_extras(hogan: &HoganMaterialData) -> HoganExtrasJson {
    let layout = hogan
        .shader_permutations
        .first()
        .and_then(|permutation| hogan_cb_layout::predicted_layout(&permutation.name));
    let (shader_flags, vs_cb, ps_cb) = layout.as_ref().map_or((None, None, None), |layout| {
        let flags = hogan
            .shader_permutations
            .first()
            .and_then(|permutation| hogan_cb_layout::parse_flags(&permutation.name))
            .map(hogan_cb_layout::flags_to_hex);
        (
            flags,
            Some(cb_bytes_to_named(&hogan.vs_cb_data, &layout.cb7)),
            Some(cb_bytes_to_named(&hogan.ps_cb_data, &layout.cb8)),
        )
    });
    let (vs_params, ps_params) = if vs_cb.is_some() {
        (Vec::new(), Vec::new())
    } else {
        (
            cb_bytes_to_params(&hogan.vs_cb_data),
            cb_bytes_to_params(&hogan.ps_cb_data),
        )
    };
    HoganExtrasJson {
        shader_permutations: hogan
            .shader_permutations
            .iter()
            .map(|permutation| ShaderPermJson {
                name: permutation.name.clone(),
                hash: permutation.hash,
            })
            .collect(),
        ufx_version: hogan.ufx_version,
        blend_mode: hogan.blend_mode,
        shadow_requires_consts: hogan.shadow_requires_consts,
        skinned: hogan.skinned,
        terrain_blending: hogan.terrain_blending,
        shader_flags,
        vs_cb,
        ps_cb,
        vs_params,
        ps_params,
        hs_params: cb_bytes_to_params(&hogan.hs_cb_data),
        ds_params: cb_bytes_to_params(&hogan.ds_cb_data),
        gs_params: cb_bytes_to_params(&hogan.gs_cb_data),
        textures: hogan.textures.clone(),
    }
}

/// Result of building glTF materials from UGX material data.
pub(super) struct MaterialBuildResult {
    pub materials: Vec<json::Material>,
    pub images: Vec<json::Image>,
    pub textures: Vec<json::Texture>,
}

/// Build glTF materials, images, and textures from UGX materials.
///
/// # Errors
///
/// Returns an error if a generated glTF index or texture-coordinate channel
/// cannot be represented by the glTF schema.
pub(super) fn build_materials(materials: &[Material]) -> Result<MaterialBuildResult> {
    let (images, textures, texture_registry) = build_texture_registry(materials)?;
    let materials = materials
        .iter()
        .map(|material| build_material(material, &texture_registry))
        .collect::<Result<Vec<_>>>()?;
    Ok(MaterialBuildResult {
        materials,
        images,
        textures,
    })
}

fn build_texture_registry(
    materials: &[Material],
) -> Result<(Vec<json::Image>, Vec<json::Texture>, TextureRegistry)> {
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut registry = TextureRegistry::new();
    for map in materials
        .iter()
        .filter_map(Material::legacy)
        .flat_map(|legacy| MapType::ALL.map(|map_type| &legacy.maps[map_type as usize]))
        .flatten()
        .filter(|map| !map.name.is_empty())
    {
        if registry.contains_key(&map.name) {
            continue;
        }
        let image_index = checked_u32(images.len(), "glTF image index")?;
        images.push(json::Image {
            buffer_view: None,
            mime_type: None,
            name: Some(map.name.clone()),
            uri: Some(map.name.clone()),
            extensions: None,
            extras: json::Extras::default(),
        });
        let texture_index = checked_u32(textures.len(), "glTF texture index")?;
        textures.push(json::Texture {
            name: None,
            sampler: None,
            source: json::Index::new(image_index),
            extensions: None,
            extras: json::Extras::default(),
        });
        registry.insert(map.name.clone(), texture_index);
    }
    Ok((images, textures, registry))
}

fn build_material(material: &Material, registry: &TextureRegistry) -> Result<json::Material> {
    let references = build_texture_references(material.legacy(), registry)?;
    let legacy = material.legacy();
    let blend_type = legacy.map_or(0, |data| data.blend_type);
    let opacity = legacy.map_or(1.0, |data| data.opacity);
    let flags = legacy.map_or(0, |data| data.flags);
    let spec_power = legacy.map_or(10.0, |data| data.spec_power);
    let uses_opacity = flags & material_flags::OPACITY_VALID != 0;
    let (alpha_mode, alpha_cutoff, visual_alpha) =
        alpha_settings(BlendType::from_raw(blend_type), uses_opacity, opacity);
    let emissive_factor = if references.emissive.is_some() {
        [1.0, 1.0, 1.0]
    } else {
        [0.0, 0.0, 0.0]
    };
    Ok(json::Material {
        alpha_cutoff,
        alpha_mode,
        double_sided: flags & material_flags::TWO_SIDED != 0,
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, visual_alpha]),
            base_color_texture: references.base_color,
            metallic_factor: json::material::StrengthFactor(0.0),
            roughness_factor: json::material::StrengthFactor(
                1.0 - (spec_power / 100.0).clamp(0.0, 1.0),
            ),
            metallic_roughness_texture: None,
            extensions: None,
            extras: json::Extras::default(),
        },
        normal_texture: references.normal,
        occlusion_texture: references.occlusion,
        emissive_texture: references.emissive,
        emissive_factor: json::material::EmissiveFactor(emissive_factor),
        extensions: None,
        extras: build_material_extras(material),
        name: Some(material.name.clone()),
    })
}

fn build_texture_references(
    legacy: Option<&LegacyMaterialData>,
    registry: &TextureRegistry,
) -> Result<TextureReferences> {
    let base_color = basic_texture_info(first_map(legacy, MapType::Diffuse), registry)?;
    let emissive = basic_texture_info(first_map(legacy, MapType::Emissive), registry)?;
    let normal = texture_reference(first_map(legacy, MapType::Normal), registry)?.map(
        |(index, tex_coord)| json::material::NormalTexture {
            index: json::Index::new(index),
            scale: 1.0,
            tex_coord,
            extensions: None,
            extras: json::Extras::default(),
        },
    );
    let occlusion =
        texture_reference(first_map(legacy, MapType::AO), registry)?.map(|(index, tex_coord)| {
            json::material::OcclusionTexture {
                index: json::Index::new(index),
                strength: json::material::StrengthFactor(1.0),
                tex_coord,
                extensions: None,
                extras: json::Extras::default(),
            }
        });
    Ok(TextureReferences {
        base_color,
        normal,
        occlusion,
        emissive,
    })
}

fn first_map(legacy: Option<&LegacyMaterialData>, map_type: MapType) -> Option<&Map> {
    legacy?.maps[map_type as usize]
        .first()
        .filter(|map| !map.name.is_empty())
}

fn basic_texture_info(
    map: Option<&Map>,
    registry: &TextureRegistry,
) -> Result<Option<json::texture::Info>> {
    Ok(
        texture_reference(map, registry)?.map(|(index, tex_coord)| json::texture::Info {
            index: json::Index::new(index),
            tex_coord,
            extensions: None,
            extras: json::Extras::default(),
        }),
    )
}

fn texture_reference(map: Option<&Map>, registry: &TextureRegistry) -> Result<Option<(u32, u32)>> {
    let Some(map) = map else {
        return Ok(None);
    };
    let index = registry.get(&map.name).copied().ok_or_else(|| {
        Error::UnsupportedFormat(format!(
            "Texture '{}' is missing from the registry",
            map.name
        ))
    })?;
    let tex_coord = u32::try_from(map.channel)
        .map_err(|_| Error::UnsupportedFormat("Texture channel cannot be negative".into()))?;
    Ok(Some((index, tex_coord)))
}

fn alpha_settings(
    blend_type: BlendType,
    uses_opacity: bool,
    opacity: f32,
) -> (
    json::validation::Checked<json::material::AlphaMode>,
    Option<json::material::AlphaCutoff>,
    f32,
) {
    match blend_type {
        BlendType::AlphaTest => (
            Valid(json::material::AlphaMode::Mask),
            Some(json::material::AlphaCutoff(0.5)),
            1.0,
        ),
        BlendType::Additive | BlendType::Over => (
            Valid(json::material::AlphaMode::Blend),
            None,
            if uses_opacity { opacity } else { 1.0 },
        ),
        BlendType::AlphaToCoverage if uses_opacity && opacity < 1.0 => {
            (Valid(json::material::AlphaMode::Blend), None, opacity)
        }
        BlendType::AlphaToCoverage => (Valid(json::material::AlphaMode::Opaque), None, 1.0),
    }
}

fn checked_u32(value: usize, context: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}
