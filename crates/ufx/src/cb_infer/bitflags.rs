//! Hogan ubershader bitflag decoder.
//!
//! The 64-bit hex value in Hogan filenames (e.g.
//! `hogan_standard_00080000a8000960`) encodes feature flags that determine
//! which parameters are packed into the constant buffers.  This module maps
//! individual bits to the shader features they enable.
//!
//! # Bit classification (derived from 679 `hogan_standard` variants)
//!
//! ## Always-on
//! - **Bit 11**: Baseline flag, set in every variant.
//!
//! ## UV addressing mode (mutually exclusive)
//! - **Bit 5**: Standard paired UV (91%).
//! - **Bits 2, 3, 4**: Alternative UV modes (exclusive with 5).
//!
//! ## Compilation-only flags (change bytecode hash, not CB layout)
//! - **Bits 0, 1, 6, 7, 8, 10**: Texture sampling / compilation options.
//! - **Bits 33, 36, 37, 42, 43**: Sub-mode modifiers (emissive/vertex-anim).
//!
//! ## Lighting model
//! - **Bit 20**: Base lighting toggle.
//! - **Bit 21**: Lighting model A (with 20, exclusive with 22).
//! - **Bit 22**: Lighting model B (with 20, exclusive with 21).
//!
//! ## CB parameter features
//! See [`decode_features`] and [`predicted_semantics`] for the full mapping.
//!
//! # Usage
//!
//! ```ignore
//! use ufx::cb_infer::bitflags::{HoganFlags, decode_features};
//!
//! let flags = HoganFlags(0x00080000a8000960);
//! let features = decode_features(flags);
//! for f in &features {
//!     println!("{}: cb{} — {}", f.name, f.cb_slot, f.description);
//! }
//! ```

use alloc::string::String;
use alloc::vec::Vec;

/// A parsed 64-bit Hogan feature flag word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HoganFlags(pub u64);

impl HoganFlags {
    /// Parse from a hex string (no `0x` prefix).
    pub fn from_hex(s: &str) -> Option<Self> {
        u64::from_str_radix(s, 16).ok().map(Self)
    }

    /// Extract from a Hogan filename like `hogan_standard_00080000a8000960.ufx`.
    pub fn from_filename(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".ufx").unwrap_or(name);
        let hex_part = stem.rsplit('_').next()?;
        Self::from_hex(hex_part)
    }

    /// Test whether a specific bit is set.
    #[inline]
    pub fn has(self, bit: u32) -> bool {
        self.0 & (1u64 << bit) != 0
    }

    /// Return all feature descriptors for the set bits.
    pub fn features(self) -> Vec<Feature> {
        decode_features(self)
    }
}

/// A single shader feature toggled by one or more flag bits.
#[derive(Debug, Clone)]
pub struct Feature {
    /// Human-readable feature name.
    pub name: String,
    /// The primary bit that toggles this feature.
    pub bit: u32,
    /// Which constant buffer slot the parameters land in (7=VS, 8=PS).
    pub cb_slot: u32,
    /// Brief description.
    pub description: String,
    /// Whether this feature requires other bits to be active.
    pub requires_bits: Vec<u32>,
}

/// Known bit → feature definitions derived from single-bit-flip analysis
/// across 679 `hogan_standard` variants.
///
/// ## Single-bit features (always active when bit is set)
///
/// | Bit | CB | Feature                |
/// |-----|----|------------------------|
/// |  23 |  7 | Height blend + gradient|
/// |  27 |  8 | Extra texture (t4)     |
/// |  41 |  7 | Vertex anim (mode A)   |
/// |  44 |  7 | Vertex anim (mode B)   |
/// |  49 |  8 | Material overrides     |
/// |  50 |  8 | Overlay texture (t4)   |
///
/// ## Compound features (require other bits)
///
/// | Bit | Requires | CB | Feature              |
/// |-----|----------|----|----------------------|
/// |  32 | 20, 21   |  8 | Emissive + t4 layer  |
/// |  35 | 32       |  8 | Scroll animation     |
/// |  38 | —        |  8 | Per-channel UV scale |
pub fn decode_features(flags: HoganFlags) -> Vec<Feature> {
    let mut out = Vec::new();

    // --- Single-bit features ---

    if flags.has(23) {
        out.push(Feature {
            name: String::from("height_blend"),
            bit: 23,
            cb_slot: 7,
            description: String::from(
                "Height-based blending: adds height_blend_range and color_gradient to cb7",
            ),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(27) {
        out.push(Feature {
            name: String::from("extra_texture_t4"),
            bit: 27,
            cb_slot: 8,
            description: String::from("Extra texture layer: adds uv_scale_t4 to cb8"),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(41) {
        out.push(Feature {
            name: String::from("vertex_anim_a"),
            bit: 41,
            cb_slot: 7,
            description: String::from(
                "Vertex animation mode A: adds vertex_anim params (cb7[0].zw, cb7[1].xy)",
            ),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(44) {
        out.push(Feature {
            name: String::from("vertex_anim_b"),
            bit: 44,
            cb_slot: 7,
            description: String::from(
                "Vertex animation mode B: adds vertex_anim params (cb7[0].zw, cb7[1].x)",
            ),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(49) {
        out.push(Feature {
            name: String::from("material_overrides"),
            bit: 49,
            cb_slot: 8,
            description: String::from(
                "Material overrides: detail_blend, spec_override, override_strength, roughness_override",
            ),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(50) {
        out.push(Feature {
            name: String::from("overlay_texture"),
            bit: 50,
            cb_slot: 8,
            description: String::from("Overlay texture: adds uv_scale_t4_t0 to cb8"),
            requires_bits: Vec::new(),
        });
    }

    // --- Compound / multi-bit features ---
    //
    // Cross-validation across 679 variants shows these bits interact:
    //
    // Bit 28: roughness_override_value (appears with 27+28, independent of bit 49)
    // Bit 29: emissive_intensity (with bit 27; also adds uv_scale variants)
    // Bit 30: emissive_intensity (with bit 27; similar to 29)
    // Bit 32: emissive + extra texture layer (adds uv_scale_t4/t5 + emissive)
    // Bit 35: scroll animation (requires bit 32)

    if flags.has(28) {
        out.push(Feature {
            name: String::from("roughness_channel"),
            bit: 28,
            cb_slot: 8,
            description: String::from("Roughness override: adds roughness_override_value to cb8"),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(29) && flags.has(27) {
        out.push(Feature {
            name: String::from("emissive_layer_a"),
            bit: 29,
            cb_slot: 8,
            description: String::from(
                "Emissive layer (mode A): adds emissive_intensity to cb8 (with bit 27)",
            ),
            requires_bits: alloc::vec![27],
        });
    }

    if flags.has(30) && flags.has(27) {
        out.push(Feature {
            name: String::from("emissive_layer_b"),
            bit: 30,
            cb_slot: 8,
            description: String::from(
                "Emissive layer (mode B): adds emissive_intensity to cb8 (with bit 27)",
            ),
            requires_bits: alloc::vec![27],
        });
    }

    if flags.has(32) {
        out.push(Feature {
            name: String::from("emissive_texture"),
            bit: 32,
            cb_slot: 8,
            description: String::from(
                "Emissive texture layer: adds emissive_intensity and uv_scale_t4/t5 to cb8",
            ),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(35) {
        out.push(Feature {
            name: String::from("scroll_animation"),
            bit: 35,
            cb_slot: 8,
            description: String::from(
                "Scroll animation: adds scroll_phase, scroll_period, uv_scroll_speed",
            ),
            requires_bits: alloc::vec![32],
        });
    }

    if flags.has(38) {
        out.push(Feature {
            name: String::from("per_channel_uv"),
            bit: 38,
            cb_slot: 8,
            description: String::from(
                "Per-channel UV scales: splits paired uv_scale into individual components",
            ),
            requires_bits: Vec::new(),
        });
    }

    // --- Emissive / texture sub-modes (require bit 32) ---

    if flags.has(33) && flags.has(32) {
        out.push(Feature {
            name: String::from("emissive_submode"),
            bit: 33,
            cb_slot: 8,
            description: String::from(
                "Emissive sub-mode: changes instruction ordering (no new params)",
            ),
            requires_bits: alloc::vec![32],
        });
    }

    if flags.has(34) && flags.has(32) {
        out.push(Feature {
            name: String::from("special_instance"),
            bit: 34,
            cb_slot: 8,
            description: String::from(
                "SpecialInstanceSC: adds LayerEffectData/ReflectionPlane/EmissiveTintSlot cbuffer, swaps UV channel order",
            ),
            requires_bits: alloc::vec![32],
        });
    }

    if flags.has(39) && flags.has(32) {
        out.push(Feature {
            name: String::from("extra_texcoord"),
            bit: 39,
            cb_slot: 7,
            description: String::from(
                "Extra TEXCOORD interpolant: adds TEXCOORD5 VS output and PS input",
            ),
            requires_bits: alloc::vec![32],
        });
    }

    // --- Skinning and color ---

    if flags.has(40) {
        out.push(Feature {
            name: String::from("skinning"),
            bit: 40,
            cb_slot: 7,
            description: String::from("Skinning: adds BLENDINDICES and BLENDWEIGHT vertex inputs"),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(52) {
        out.push(Feature {
            name: String::from("constant_colour"),
            bit: 52,
            cb_slot: 12,
            description: String::from("Constant colour: adds ConstantColourXSC cbuffer (slot 12)"),
            requires_bits: alloc::vec![51],
        });
    }

    // --- Texture reduction / modification ---

    if flags.has(46) {
        out.push(Feature {
            name: String::from("simplified_texturing"),
            bit: 46,
            cb_slot: 8,
            description: String::from(
                "Simplified texturing: removes normal_intensity and InstanceSC from PS, reorders UV naming",
            ),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(48) {
        out.push(Feature {
            name: String::from("opacity_blend"),
            bit: 48,
            cb_slot: 8,
            description: String::from(
                "Opacity blend: adds opacity_blend parameter (requires 23+32+38+49)",
            ),
            requires_bits: alloc::vec![23, 32, 38, 49],
        });
    }

    if flags.has(53) {
        out.push(Feature {
            name: String::from("reduced_texturing"),
            bit: 53,
            cb_slot: 8,
            description: String::from(
                "Reduced texturing: removes normal/BRDF/specular texture maps, much smaller PS",
            ),
            requires_bits: Vec::new(),
        });
    }

    // --- Lighting model ---

    if flags.has(20) {
        let model = if flags.has(21) {
            "A (bit 21)"
        } else if flags.has(22) {
            "B (bit 22)"
        } else {
            "base"
        };
        out.push(Feature {
            name: String::from("lighting"),
            bit: 20,
            cb_slot: 8,
            description: alloc::format!(
                "Lighting enabled: model {model} — adds InstanceSC with TeamTint/EmissiveTint"
            ),
            requires_bits: Vec::new(),
        });
    }

    // --- UV addressing mode ---

    if flags.has(5) {
        out.push(Feature {
            name: String::from("uv_paired"),
            bit: 5,
            cb_slot: 8,
            description: String::from("UV mode: standard paired channels (t0_t1, t2_t3)"),
            requires_bits: Vec::new(),
        });
    } else if flags.has(2) {
        out.push(Feature {
            name: String::from("uv_mode_a"),
            bit: 2,
            cb_slot: 8,
            description: String::from("UV mode A: alternative addressing (exclusive with bit 5)"),
            requires_bits: Vec::new(),
        });
    } else if flags.has(3) {
        out.push(Feature {
            name: String::from("uv_mode_b"),
            bit: 3,
            cb_slot: 8,
            description: String::from("UV mode B: alternative addressing (exclusive with bit 5)"),
            requires_bits: Vec::new(),
        });
    } else if flags.has(4) {
        out.push(Feature {
            name: String::from("uv_mode_c"),
            bit: 4,
            cb_slot: 8,
            description: String::from("UV mode C: alternative addressing (exclusive with bit 5)"),
            requires_bits: Vec::new(),
        });
    }

    if flags.has(6) {
        out.push(Feature {
            name: String::from("uv_mode_individual"),
            bit: 6,
            cb_slot: 8,
            description: String::from("UV mode: individual scale per texture (uv_scale_t0)"),
            requires_bits: Vec::new(),
        });
    }

    out
}

/// Extract the hex flags portion from a Hogan shader filename.
///
/// Returns the hex string (without `0x` prefix) from names like
/// `hogan_standard_00080000a8000960` or `hogan_standard_00080000a8000960.ufx`.
pub fn extract_hex_from_name(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(".ufx").unwrap_or(name);
    stem.rsplit('_').next()
}

/// Summary of all active feature bits for display purposes.
pub fn flags_summary(flags: HoganFlags) -> String {
    let mut bits: Vec<u32> = Vec::new();
    for b in 0..64u32 {
        if flags.has(b) {
            bits.push(b);
        }
    }
    use alloc::format;
    use core::fmt::Write;
    let mut s = format!("0x{:016x} ({} bits set: ", flags.0, bits.len());
    for (i, b) in bits.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let _ = write!(s, "{b}");
    }
    s.push(')');
    s
}

/// Return the set of semantic names predicted by the bitflags.
///
/// This is the ground-truth prediction: if the flags say a feature is
/// enabled, these are the CB parameter semantics that *should* be present,
/// even if the pattern matcher can't find them (dead code elimination, etc.).
///
/// Useful for cross-validation against [`super::infer_cb_params`] results.
pub fn predicted_semantics(flags: HoganFlags) -> Vec<&'static str> {
    let mut sems = Vec::new();

    // Bit 23: height blending (cb7)
    if flags.has(23) {
        sems.push("height_blend_range");
        sems.push("color_gradient");
    }

    // Bit 27: extra texture layer t4
    if flags.has(27) {
        sems.push("uv_scale_t4");
    }

    // Bit 28: roughness override (independent of bit 49)
    if flags.has(28) {
        sems.push("roughness_override_value");
    }

    // Bits 29, 30 (with 27): emissive intensity
    if flags.has(27) && (flags.has(29) || flags.has(30)) {
        sems.push("emissive_intensity");
    }

    // Bit 32: emissive + extra texture layer (also adds uv_scale_t4)
    if flags.has(32) {
        if !sems.contains(&"emissive_intensity") {
            sems.push("emissive_intensity");
        }
        if !sems.contains(&"uv_scale_t4") {
            sems.push("uv_scale_t4");
        }
    }

    // Bit 35 (with 32): scroll animation + roughness_override_value
    if flags.has(35) && flags.has(32) {
        sems.push("scroll_phase");
        sems.push("scroll_period");
        sems.push("uv_scroll_speed");
        if !sems.contains(&"roughness_override_value") {
            sems.push("roughness_override_value");
        }
    }

    // Bits 41, 44: vertex animation (cb7)
    if flags.has(41) || flags.has(44) {
        sems.push("vertex_anim");
    }

    // Bit 25: rare combined feature (always with 23+41+43), adds override_strength
    if flags.has(25) && !sems.contains(&"override_strength") {
        sems.push("override_strength");
    }

    // Bit 49: material overrides
    if flags.has(49) {
        sems.push("detail_blend_factor");
        sems.push("spec_override_color");
        if !sems.contains(&"override_strength") {
            sems.push("override_strength");
        }
        if !sems.contains(&"roughness_override_value") {
            sems.push("roughness_override_value");
        }
    }

    // Bits 50, 51: overlay texture (adds uv_scale_t4 variant)
    if (flags.has(50) || flags.has(51)) && !sems.contains(&"uv_scale_t4") {
        sems.push("uv_scale_t4");
    }

    // Opacity blend: bit 48 (with 23+32+49) OR bits 20+32+35+49
    if (flags.has(48) && flags.has(23) && flags.has(32) && flags.has(49))
        || (flags.has(20) && flags.has(32) && flags.has(35) && flags.has(49))
    {
        sems.push("opacity_blend");
    }

    sems
}
