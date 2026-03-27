//! Serializable material extras for glTF roundtrip.
//!
//! These structs map 1:1 to the JSON stored in glTF material `extras`.
//! Using `#[derive(Serialize, Deserialize)]` replaces ~200 lines of manual
//! `serde_json::Map::insert` / `obj.get(...)` calls in export and import.

use serde::{Deserialize, Serialize};

/// Top-level material extras stored in glTF.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MaterialExtrasJson {
    /// Material version from `@Ver` attribute (4 = HW1, 5 = HW2 legacy).
    #[serde(default = "default_mat_version")]
    pub ugx_material_version: u32,

    // --- Legacy fields (present when material is Legacy) ---
    #[serde(default)]
    pub ugx_flags: Option<u32>,
    #[serde(default)]
    pub ugx_blend_type: Option<u8>,
    #[serde(default)]
    pub ugx_spec_power: Option<f32>,
    #[serde(default)]
    pub ugx_spec_color: Option<[f32; 3]>,
    #[serde(default)]
    pub ugx_env_reflectivity: Option<f32>,
    #[serde(default)]
    pub ugx_env_sharpness: Option<f32>,
    #[serde(default)]
    pub ugx_env_fresnel: Option<f32>,
    #[serde(default)]
    pub ugx_env_fresnel_power: Option<f32>,
    #[serde(default)]
    pub ugx_accessory_index: Option<u32>,
    #[serde(default)]
    pub ugx_opacity: Option<f32>,

    /// UVW velocity per map type (only present if any non-zero).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_uvw_velocity: Option<Vec<[f32; 3]>>,

    /// Texture maps keyed by map type name, each with name/channel/flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_maps: Option<std::collections::BTreeMap<String, Vec<MapEntryJson>>>,

    // --- Hogan fields (present when material is Hogan) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_hogan: Option<HoganExtrasJson>,
}

/// A single texture map entry in extras.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MapEntryJson {
    pub name: String,
    pub channel: i16,
    pub flags: u8,
}

/// HW2 Hogan material data stored in extras.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HoganExtrasJson {
    #[serde(default)]
    pub shader_permutations: Vec<ShaderPermJson>,
    #[serde(default = "default_ufx_version")]
    pub ufx_version: u32,
    #[serde(default)]
    pub blend_mode: u32,
    #[serde(default)]
    pub shadow_requires_consts: bool,
    #[serde(default)]
    pub skinned: bool,
    #[serde(default)]
    pub terrain_blending: bool,
    #[serde(default)]
    pub vs_cb_data: u32,
    #[serde(default)]
    pub ps_cb_data: u32,
    #[serde(default)]
    pub textures: String,
}

/// Shader permutation entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShaderPermJson {
    pub name: String,
    pub hash: u32,
}

fn default_mat_version() -> u32 {
    4
}

fn default_ufx_version() -> u32 {
    9
}
