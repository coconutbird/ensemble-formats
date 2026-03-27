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

/// Material definition (from BBinaryDataTree packed document).
///
/// Materials are stored in UGX chunk 0x704 as a BBinaryDataTree document.
/// Each material has 13 map type slots, UVW velocities per map type,
/// and properties from a BNameValueMap.
#[derive(Debug, Clone)]
pub struct Material {
    /// Material name.
    pub name: String,
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
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
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
        }
    }
}
