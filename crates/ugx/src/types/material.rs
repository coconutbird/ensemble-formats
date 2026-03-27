//! Material types: MapType, Map, Material.

use alloc::string::String;
use alloc::vec::Vec;

/// Unigeom map types (13 types, matching Ensemble's eMapType enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapType {
    Diffuse = 0,
    Normal = 1,
    Gloss = 2,
    Opacity = 3,
    XForm = 4,
    Emissive = 5,
    AO = 6,
    Env = 7,
    EnvMask = 8,
    EmXForm = 9,
    Distortion = 10,
    Highlight = 11,
    Modulate = 12,
}

impl MapType {
    pub const NUM_TYPES: usize = 13;

    pub const ALL: [MapType; 13] = [
        MapType::Diffuse,
        MapType::Normal,
        MapType::Gloss,
        MapType::Opacity,
        MapType::XForm,
        MapType::Emissive,
        MapType::AO,
        MapType::Env,
        MapType::EnvMask,
        MapType::EmXForm,
        MapType::Distortion,
        MapType::Highlight,
        MapType::Modulate,
    ];

    /// Get the node name used in the BBinaryDataTree document.
    /// Names are lowercase to match the packed BDT format in UGX material chunks.
    pub fn name(&self) -> &'static str {
        match self {
            MapType::Diffuse => "diffuse",
            MapType::Normal => "normal",
            MapType::Gloss => "gloss",
            MapType::Opacity => "opacity",
            MapType::XForm => "xform",
            MapType::Emissive => "emissive",
            MapType::AO => "ao",
            MapType::Env => "env",
            MapType::EnvMask => "envmask",
            MapType::EmXForm => "emxform",
            MapType::Distortion => "distortion",
            MapType::Highlight => "highlight",
            MapType::Modulate => "modulate",
        }
    }
}

/// A texture map reference (from Unigeom::BMap).
#[derive(Debug, Clone, Default)]
pub struct Map {
    /// Texture filename.
    pub name: String,
    /// UV channel index.
    pub channel: i16,
    /// Flags.
    pub flags: u8,
}

/// A shader permutation entry used in HW2 Hogan materials.
#[derive(Debug, Clone)]
pub struct ShaderPermutation {
    /// Permutation name (e.g. `"HOGAN_STANDARD_00000000003009A0"`).
    pub name: String,
    /// Permutation hash.
    pub hash: u32,
}

/// HW2 "Hogan" material data — a shader-based material system used by
/// Halo Wars 2 in place of the legacy NameValues+Maps format.
///
/// Stored as a `<HoganMaterial>` child node in the BDT material tree.
#[derive(Debug, Clone)]
pub struct HoganMaterialData {
    /// Up to 4 shader permutations (name + hash pairs).
    pub shader_permutations: Vec<ShaderPermutation>,
    /// UFX version (typically 9).
    pub ufx_version: u32,
    /// Blend mode.
    pub blend_mode: u32,
    /// Whether the shadow pass requires constant buffer data.
    pub shadow_requires_consts: bool,
    /// Whether the material is used on a skinned mesh.
    pub skinned: bool,
    /// Whether terrain blending is enabled.
    pub terrain_blending: bool,
    /// Vertex shader constant buffer data (0 = none).
    pub vs_cb_data: u32,
    /// Pixel shader constant buffer data (0 = none).
    pub ps_cb_data: u32,
    /// Texture path pattern (e.g. `"bespoke\\archetypes\\...\\model_[al]"`).
    pub textures: String,
}

/// Material definition (from BBinaryDataTree packed document).
///
/// Materials are stored in UGX chunk 0x704 as a BBinaryDataTree document.
/// Each material has 13 map type slots, UVW velocities per map type,
/// and properties from a BNameValueMap.
///
/// HW2 files may use either the legacy format (same as HW1 but `@Ver=5`)
/// or the Hogan shader-based format. When `hogan` is `Some`, the material
/// uses the Hogan format and legacy fields may contain defaults.
#[derive(Debug, Clone)]
pub struct Material {
    /// Material name (from `@Name` attribute; empty for Hogan materials).
    pub name: String,
    /// Material version from `@Ver` attribute (4 = HW1, 5 = HW2 legacy).
    pub material_version: u32,
    /// Texture maps indexed by MapType (13 slots, each can have multiple maps).
    pub maps: [Vec<Map>; MapType::NUM_TYPES],
    /// UVW velocity per map type.
    pub uvw_velocity: [[f32; 3]; MapType::NUM_TYPES],
    /// Specular power (default: 10.0).
    pub spec_power: f32,
    /// Specular color (R, G, B). Default: (1.0, 1.0, 1.0).
    pub spec_color: [f32; 3],
    /// Environment reflectivity (default: 1.0).
    pub env_reflectivity: f32,
    /// Environment sharpness (default: 1.0).
    pub env_sharpness: f32,
    /// Environment fresnel (default: 0.5).
    pub env_fresnel: f32,
    /// Environment fresnel power (default: 4.0).
    pub env_fresnel_power: f32,
    /// Accessory index (default: 0).
    pub accessory_index: u32,
    /// Material flags (default: 0).
    pub flags: u32,
    /// Blend type (default: 0).
    pub blend_type: u8,
    /// Opacity (default: 1.0).
    pub opacity: f32,
    /// HW2 Hogan material data (if present, this material uses the Hogan format).
    pub hogan: Option<HoganMaterialData>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            material_version: 4,
            maps: Default::default(),
            uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            spec_power: 10.0,
            spec_color: [1.0, 1.0, 1.0],
            env_reflectivity: 1.0,
            env_sharpness: 1.0,
            env_fresnel: 0.5,
            env_fresnel_power: 4.0,
            accessory_index: 0,
            flags: 0,
            blend_type: 0,
            opacity: 1.0,
            hogan: None,
        }
    }
}
