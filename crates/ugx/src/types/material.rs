//! Material types: `MapType`, Map, Material, `MaterialFlags`, `BlendType`.

use alloc::string::String;
use alloc::vec::Vec;

/// HW1 (`@Ver=4`) legacy material flags bitmask (from `Flags` in BDT `NameValues`).
///
/// Matches the C++ `Unigeom::BMaterial` flags enum. Used by
/// `BUGXGeomSectionRenderer_initFromMaterial` (`0x1406C93E0`) to
/// configure per-section rendering state.
///
/// These flags only apply to legacy (v4) materials; HW2 Hogan materials
/// use a separate shader-driven system.
pub mod material_flags {
    /// Bit 0 — **Color Gloss**: specular color comes from the gloss map.
    pub const COLOR_GLOSS: u32 = 1 << 0;

    /// Bit 1 — **Opacity Valid**: gates whether the `Opacity` value from
    /// BDT is read and applied. When clear, opacity stays at 1.0 (fully
    /// opaque) regardless of the stored `Opacity` value.
    pub const OPACITY_VALID: u32 = 1 << 1;

    /// Bit 2 — **Two-Sided**: enables double-sided / backface rendering.
    /// Maps to `doubleSided: true` in glTF.
    pub const TWO_SIDED: u32 = 1 << 2;

    /// Bit 3 — **Disable Shadows**: disables shadow casting for this material.
    pub const DISABLE_SHADOWS: u32 = 1 << 3;

    /// Bit 4 — **Global Env**: forces global environment mapping even when
    /// no environment map texture is present.
    pub const GLOBAL_ENV: u32 = 1 << 4;

    /// Bit 5 — **Terrain Conform**: enables terrain conformance rendering.
    pub const TERRAIN_CONFORM: u32 = 1 << 5;

    /// Bit 6 — **Local Reflection**: enables local reflections
    /// (shader constant `gLocalReflectionEnabled`).
    pub const LOCAL_REFLECTION: u32 = 1 << 6;

    /// Bit 7 — **Disable Shadow Reception**: prevents this material from
    /// receiving shadows cast by other objects.
    pub const DISABLE_SHADOW_RECEPTION: u32 = 1 << 7;
}

/// HW1 (`@Ver=4`) legacy blend type values (from `BlendType` byte in BDT `NameValues`).
///
/// Matches the C++ `Unigeom::BMaterial` blend type enum. Used by
/// `BUGXGeomSectionRenderer_initFromMaterial` (`0x1406c97d2`).
///
/// These blend types only apply to legacy (v4) materials; HW2 Hogan
/// materials have their own `blend_mode` field with different semantics.
///
/// Values ≥ 4 are treated identically to 0 by the engine (catch-all default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendType {
    /// Alpha-to-coverage (engine blend mode 0, flag 1).
    AlphaToCoverage = 0,
    /// Additive blending (engine blend mode 2, flag 2).
    Additive = 1,
    /// Over operator / alpha blend (engine blend mode 1, flag 4).
    Over = 2,
    /// Alpha test (engine blend mode 3, flag 1).
    AlphaTest = 3,
}

impl BlendType {
    /// Parse a raw byte into a `BlendType`.
    ///
    /// Values ≥ 4 are treated as `AlphaToCoverage` by the engine (catch-all).
    #[must_use]
    pub fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::Additive,
            2 => Self::Over,
            3 => Self::AlphaTest,
            _ => Self::AlphaToCoverage,
        }
    }

    /// Returns `true` if this blend type uses the default alpha-to-coverage
    /// path (value 0 or ≥ 4).
    #[must_use]
    pub fn is_alpha_to_coverage(self) -> bool {
        matches!(self, Self::AlphaToCoverage)
    }
}

/// Unigeom map types (13 types, matching Ensemble's eMapType enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapType {
    /// Diffuse (albedo) color texture.
    Diffuse = 0,
    /// Normal map (tangent-space).
    Normal = 1,
    /// Gloss / specular power map.
    Gloss = 2,
    /// Opacity / alpha mask.
    Opacity = 3,
    /// UV transform / detail texture.
    XForm = 4,
    /// Emissive (self-illumination) map.
    Emissive = 5,
    /// Ambient occlusion map.
    AO = 6,
    /// Environment / reflection map.
    Env = 7,
    /// Environment mask (controls reflection intensity).
    EnvMask = 8,
    /// Emissive UV transform map.
    EmXForm = 9,
    /// Distortion / refraction map.
    Distortion = 10,
    /// Highlight / rim-light map.
    Highlight = 11,
    /// Modulation (blend) map.
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

    /// Get the node name used in the `BBinaryDataTree` document.
    /// Names are lowercase to match the packed BDT format in UGX material chunks.
    #[must_use]
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

/// A texture map reference (from `Unigeom::BMap`).
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
    /// Vertex shader constant buffer initialization data.
    ///
    /// Raw binary blob stored as a BDT "string" node. The engine allocates a
    /// buffer, then `memcpy`s this data into the VS constant buffer before
    /// rendering.  Empty means the shader uses its compiled-in defaults.
    ///
    /// **Note:** BDT currently decodes all "string" nodes via
    /// `from_utf8_lossy`, so non-UTF-8 binary data may be corrupted on
    /// roundtrip.  A future `Bytes` variant would fix this.
    pub vs_cb_data: Vec<u8>,
    /// Pixel shader constant buffer initialization data (see [`vs_cb_data`](Self::vs_cb_data)).
    pub ps_cb_data: Vec<u8>,
    /// Hull shader constant buffer initialization data (see [`vs_cb_data`](Self::vs_cb_data)).
    pub hs_cb_data: Vec<u8>,
    /// Domain shader constant buffer initialization data (see [`vs_cb_data`](Self::vs_cb_data)).
    pub ds_cb_data: Vec<u8>,
    /// Geometry shader constant buffer initialization data (see [`vs_cb_data`](Self::vs_cb_data)).
    pub gs_cb_data: Vec<u8>,
    /// Texture path pattern (e.g. `"bespoke\\archetypes\\...\\model_[al]"`).
    pub textures: String,
}

/// Legacy material data — the HW1 fixed-function material system.
///
/// Uses 13 explicit map slots (diffuse, normal, gloss, etc.) and
/// properties from a `BNameValueMap` (specular, env reflectivity, etc.).
#[derive(Debug, Clone)]
pub struct LegacyMaterialData {
    /// Texture maps indexed by `MapType` (13 slots, each can have multiple maps).
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

impl Default for LegacyMaterialData {
    fn default() -> Self {
        Self {
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

/// Discriminated material format — either Legacy (HW1 map-based) or
/// Hogan (HW2 shader-based).
///
/// This enum gives type-level separation between the two material systems
/// and makes cross-version conversion a natural `match` arm.
#[derive(Debug, Clone)]
pub enum MaterialData {
    /// HW1 fixed-function material (also used by HW2 legacy `@Ver=5`).
    Legacy(alloc::boxed::Box<LegacyMaterialData>),
    /// HW2 Hogan shader-based material.
    Hogan(alloc::boxed::Box<HoganMaterialData>),
}

/// Material definition (from `BBinaryDataTree` packed document).
///
/// Materials are stored in UGX chunk 0x704 as a `BBinaryDataTree` document.
/// The `data` field determines whether this is a legacy (map-based) or
/// Hogan (shader-based) material.
#[derive(Debug, Clone)]
pub struct Material {
    /// Material name (from `@Name` attribute; empty for Hogan materials).
    pub name: String,
    /// Material version from `@Ver` attribute (4 = HW1, 5 = HW2 legacy).
    pub material_version: u32,
    /// Format-specific material data.
    pub data: MaterialData,
}

impl Material {
    /// Returns `true` if this is a legacy (map-based) material.
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        matches!(self.data, MaterialData::Legacy(_))
    }

    /// Returns `true` if this is a Hogan (shader-based) material.
    #[must_use]
    pub fn is_hogan(&self) -> bool {
        matches!(self.data, MaterialData::Hogan(_))
    }

    /// Returns a reference to the legacy data, or `None` if Hogan.
    #[must_use]
    pub fn legacy(&self) -> Option<&LegacyMaterialData> {
        match &self.data {
            MaterialData::Legacy(l) => Some(l),
            MaterialData::Hogan(_) => None,
        }
    }

    /// Returns a mutable reference to the legacy data, or `None` if Hogan.
    pub fn legacy_mut(&mut self) -> Option<&mut LegacyMaterialData> {
        match &mut self.data {
            MaterialData::Legacy(l) => Some(l),
            MaterialData::Hogan(_) => None,
        }
    }

    /// Returns a reference to the Hogan data, or `None` if legacy.
    #[must_use]
    pub fn hogan(&self) -> Option<&HoganMaterialData> {
        match &self.data {
            MaterialData::Hogan(h) => Some(h),
            MaterialData::Legacy(_) => None,
        }
    }

    /// Returns a mutable reference to the Hogan data, or `None` if legacy.
    pub fn hogan_mut(&mut self) -> Option<&mut HoganMaterialData> {
        match &mut self.data {
            MaterialData::Hogan(h) => Some(h),
            MaterialData::Legacy(_) => None,
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            material_version: 4,
            data: MaterialData::Legacy(alloc::boxed::Box::default()),
        }
    }
}
