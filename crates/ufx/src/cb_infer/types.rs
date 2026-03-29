//! Public types for constant-buffer parameter inference.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Default pixel-shader constant buffer slot for the Hogan ubershader.
pub const HOGAN_PS_SLOT: u32 = 8;

/// Default vertex-shader constant buffer slot for the Hogan ubershader.
pub const HOGAN_VS_SLOT: u32 = 7;

/// Confidence level for an inferred parameter semantic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// Weak contextual evidence only.
    Low = 1,
    /// Reasonable pattern match but some ambiguity.
    Medium = 2,
    /// Strong, unambiguous pattern with corroborating operands.
    High = 3,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => f.write_str("low"),
            Self::Medium => f.write_str("medium"),
            Self::High => f.write_str("high"),
        }
    }
}

/// Semantic category inferred for a constant-buffer parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SemanticKind {
    /// UV scale factor — multiplied with texcoords before sampling.
    UvScale,
    /// UV scroll offset — added to texcoords (usually in the vertex shader).
    UvScroll,
    /// UV scroll speed — multiplied with Time then added to texcoords (PS).
    UvScrollSpeed,
    /// Normal map intensity (mad with `(1,1,1)` bias).
    NormalIntensity,
    /// Emissive / tint multiplier applied to a sampled color.
    EmissiveIntensity,
    /// Alpha test reference value near a `discard`.
    AlphaTestRef,
    /// Fresnel exponent in a `log → mul → exp` chain.
    FresnelPower,
    /// Fresnel bias — `mad_sat` with `|v0.w|` (view-angle dependent).
    FresnelBias,
    /// Specular override color (negated mad with ≥3 components).
    SpecOverrideColor,
    /// Roughness override value (add with negated temp → output `o3`).
    RoughnessOverride,
    /// Generic override value (add with negated temp, target unclear).
    OverrideValue,
    /// Override strength (mul with vertex input, scalar).
    OverrideStrength,
    /// Detail blend factor (add with `−1` immediate).
    DetailBlendFactor,
    /// Environment reflection intensity (mul → mad addend).
    EnvReflectionIntensity,
    /// Soft depth intersection range (`div_sat rN, depth, cb`).
    SoftDepthRange,
    /// Color tint — passed through directly (`mov output, cb`) in VS.
    ColorTint,
    /// Vertex displacement scale — `mad pos, cb*normal, pos`.
    VertexDisplacement,
    /// Edge fade / softness threshold in dissolve or smoothstep sequences.
    EdgeFade,
    /// Dissolve or pulsing animation speed (multiplied with Time).
    AnimSpeed,
    /// Dissolve threshold offset used in smoothstep-like sequences.
    DissolveThreshold,
    /// View-dependent cutoff (`add_sat rN, |v0.w|, -cb`).
    ViewAngleCutoff,
    /// Depth-fade color/alpha — multiplied by depth falloff result.
    DepthFadeColor,
    /// Height-based blend range bounds (VS smoothstep `min`/`max`).
    HeightBlendRange,
    /// Color gradient endpoint used in a VS height-based `lerp`.
    ColorGradient,
    /// Opacity / blend factor multiplied by view angle (`v0.w`).
    OpacityBlend,
    /// Scroll phase offset — added directly to Time for UV animation.
    ScrollPhase,
    /// Dissolve width / range (negated cb subtracted from temp).
    DissolveWidth,
    /// Vertex animation parameter (amplitude, frequency, or scale near `sincos`).
    VertexAnimation,
    /// Scroll wave period / repeat distance (used in `div` for triangle waves).
    ScrollPeriod,
    /// Could not be determined.
    Unknown,
}

impl fmt::Display for SemanticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UvScale => f.write_str("uv_scale"),
            Self::UvScroll => f.write_str("uv_scroll"),
            Self::UvScrollSpeed => f.write_str("uv_scroll_speed"),
            Self::NormalIntensity => f.write_str("normal_intensity"),
            Self::EmissiveIntensity => f.write_str("emissive_intensity"),
            Self::AlphaTestRef => f.write_str("alpha_test_ref"),
            Self::FresnelPower => f.write_str("fresnel_power"),
            Self::FresnelBias => f.write_str("fresnel_bias"),
            Self::SpecOverrideColor => f.write_str("spec_override_color"),
            Self::RoughnessOverride => f.write_str("roughness_override_value"),
            Self::OverrideValue => f.write_str("override_value"),
            Self::OverrideStrength => f.write_str("override_strength"),
            Self::DetailBlendFactor => f.write_str("detail_blend_factor"),
            Self::EnvReflectionIntensity => f.write_str("env_reflection_intensity"),
            Self::SoftDepthRange => f.write_str("soft_depth_range"),
            Self::ColorTint => f.write_str("color_tint"),
            Self::VertexDisplacement => f.write_str("vertex_displacement"),
            Self::EdgeFade => f.write_str("edge_fade"),
            Self::AnimSpeed => f.write_str("anim_speed"),
            Self::DissolveThreshold => f.write_str("dissolve_threshold"),
            Self::ViewAngleCutoff => f.write_str("view_angle_cutoff"),
            Self::DepthFadeColor => f.write_str("depth_fade_color"),
            Self::HeightBlendRange => f.write_str("height_blend_range"),
            Self::ColorGradient => f.write_str("color_gradient"),
            Self::OpacityBlend => f.write_str("opacity_blend"),
            Self::ScrollPhase => f.write_str("scroll_phase"),
            Self::DissolveWidth => f.write_str("dissolve_width"),
            Self::VertexAnimation => f.write_str("vertex_anim"),
            Self::ScrollPeriod => f.write_str("scroll_period"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// Structured semantic for an inferred parameter, combining a [`SemanticKind`]
/// with optional texture slot information (e.g. `_t0_t1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Semantic {
    /// The broad category of usage.
    pub kind: SemanticKind,
    /// Texture resource slots this parameter is associated with (if any).
    pub texture_slots: Vec<u32>,
}

impl Semantic {
    pub(super) fn new(kind: SemanticKind) -> Self {
        Self {
            kind,
            texture_slots: Vec::new(),
        }
    }

    pub(super) fn with_slots(kind: SemanticKind, slots: Vec<u32>) -> Self {
        Self {
            kind,
            texture_slots: slots,
        }
    }
}

impl fmt::Display for Semantic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        for t in &self.texture_slots {
            write!(f, "_t{t}")?;
        }
        Ok(())
    }
}

/// A single identified parameter access.
#[derive(Debug, Clone)]
pub struct CbParam {
    /// Constant buffer slot ([`HOGAN_VS_SLOT`] or [`HOGAN_PS_SLOT`]).
    pub cb_slot: u32,
    /// Register index within the CB array (e.g. 0, 1, 2).
    pub reg_index: u32,
    /// Component mask or swizzle string (e.g. `"xy"`, `"xxy"`, `"z"`).
    pub components: String,
    /// Inferred semantic — what this parameter is used for.
    pub semantic: Semantic,
    /// How confident the inference is.
    pub confidence: Confidence,
    /// Instruction index where this access occurs.
    pub insn_index: usize,
}
