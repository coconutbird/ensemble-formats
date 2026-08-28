//! Hogan ubershader constant buffer layout prediction from bitflags.
//!
//! The 64-bit hex value in Hogan permutation names (e.g.
//! `HOGAN_STANDARD_00080000A8000960`) encodes feature flags that determine
//! which parameters are packed into the constant buffers.
//!
//! This module predicts the exact CB layout (parameter names and offsets)
//! from those flags, enabling named key-value storage in glTF extras.

use ugx::types::HoganFlag::{
    Emissive, EmissiveSubA, EmissiveSubB, ExtraTextureLayer, HeightBlend, MaterialOverride,
    PerChannelUv, ReducedTexturing, RoughnessChannel, ScrollAnim, SimplifiedTexturing, VertexAnimA,
    VertexAnimB,
};

/// A named parameter in the predicted constant buffer layout.
#[derive(Debug, Clone)]
pub(crate) struct CbLayoutEntry {
    /// Human-readable parameter name.
    pub name: &'static str,
    /// Offset in floats from the start of the CB data.
    pub offset: u32,
    /// Number of float components (1–4).
    pub components: u8,
}

/// Predicted constant buffer layout for a Hogan shader variant.
#[derive(Debug, Clone, Default)]
pub(crate) struct CbLayout {
    /// Vertex shader (cb7) parameters.
    pub cb7: Vec<CbLayoutEntry>,
    /// Pixel shader (cb8) parameters.
    pub cb8: Vec<CbLayoutEntry>,
    /// Total registers predicted for cb7.
    pub cb7_registers: u32,
    /// Total registers predicted for cb8.
    pub cb8_registers: u32,
}

/// Extract the 64-bit flags from a Hogan permutation name.
///
/// Accepts names like `"HOGAN_STANDARD_00080000A8000960"` or
/// `"hogan_standard_00080000a8000960.ufx"`.
pub(crate) fn parse_flags(permutation_name: &str) -> Option<u64> {
    let stem = permutation_name
        .strip_suffix(".ufx")
        .unwrap_or(permutation_name);
    let hex_part = stem.rsplit('_').next()?;
    u64::from_str_radix(hex_part, 16).ok()
}

/// Returns true if the permutation name is any Hogan shader family.
pub(crate) fn is_hogan(permutation_name: &str) -> bool {
    let lower = permutation_name.to_ascii_lowercase();
    lower.starts_with("hogan_")
}

/// Returns true if the permutation name is a `hogan_standard` variant.
fn is_hogan_standard(permutation_name: &str) -> bool {
    let lower = permutation_name.to_ascii_lowercase();
    lower.starts_with("hogan_standard_")
}

/// Helper to build a layout sequentially.
struct LayoutBuilder {
    entries: Vec<CbLayoutEntry>,
    pos: u32,
}

impl LayoutBuilder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            pos: 0,
        }
    }

    fn push(&mut self, name: &'static str, components: u8) {
        self.entries.push(CbLayoutEntry {
            name,
            offset: self.pos,
            components,
        });
        self.pos += u32::from(components);
    }

    /// Advance position to the next register boundary (multiple of 4).
    fn align_to_register(&mut self) {
        self.pos = (self.pos + 3) & !3;
    }

    /// Number of float4 registers consumed.
    fn register_count(&self) -> u32 {
        self.pos.div_ceil(4)
    }
}

/// Predict CB layouts for any Hogan shader family.
///
/// Returns `None` if the permutation is not a Hogan shader or if the
/// flags cannot be parsed.
///
/// For `hogan_standard`, returns a fully semantic layout.
/// For all other families, returns an empty layout — the caller falls back
/// to positional naming so the raw CB data still round-trips as KV pairs.
pub(crate) fn predicted_layout(permutation_name: &str) -> Option<CbLayout> {
    if !is_hogan(permutation_name) {
        return None;
    }

    let flags = parse_flags(permutation_name)?;
    if is_hogan_standard(permutation_name) {
        Some(predicted_layout_from_flags(flags))
    } else {
        // Other families: return an empty layout.
        // The caller will generate positional names from the raw blob.
        Some(CbLayout::default())
    }
}

/// Predict CB layouts from raw 64-bit flags.
pub(crate) fn predicted_layout_from_flags(flags: u64) -> CbLayout {
    let (cb7, cb7_regs) = predict_cb7(flags);
    let (cb8, cb8_regs) = predict_cb8(flags);
    CbLayout {
        cb7,
        cb8,
        cb7_registers: cb7_regs,
        cb8_registers: cb8_regs,
    }
}

/// CB7 (Vertex Shader) layout prediction.
///
/// CB7 is relatively simple:
/// - Bit 23: `height_blend_range` (4) + `color_gradient` (4)
/// - Bit 41 or 44: `vertex_anim` (4)
fn predict_cb7(flags: u64) -> (Vec<CbLayoutEntry>, u32) {
    let mut b = LayoutBuilder::new();

    if HeightBlend.test(flags) {
        b.push("height_blend_range", 4);
        b.push("color_gradient", 4);
    }

    if VertexAnimA.test(flags) || VertexAnimB.test(flags) {
        b.push("vertex_anim", 4);
    }

    let regs = b.register_count();
    (b.entries, regs)
}

/// CB8 (Pixel Shader) layout prediction.
///
/// CB8 uses a two-group packing strategy based on HLSL struct alignment:
///
/// **Group A** (core texturing, sequential from register 0):
/// - `uv_scale_t0_t1` (2), `uv_scale_t2_t3` (2) — always
/// - `normal_intensity` (1) — unless bit 46/53
/// - `uv_scale_t4` (1) — if bit 27 or 32
/// - `emissive_intensity` (1) — if bit 32 or (bit 27 + (29|30))
/// - scroll params — if bit 35 + 32
///
/// **Group B** (material overrides, starts at register boundary):
/// - `roughness_channel_value` (1) — if bit 28
/// - `detail_blend` (1), `roughness_override` (1), `override_strength` (1) — if bit 49
/// - `spec_override_color` (3), `override_bias` (1) — if bit 49 (new register)
fn predict_cb8(flags: u64) -> (Vec<CbLayoutEntry>, u32) {
    let mut b = LayoutBuilder::new();

    // --- Group A: Core texturing (sequential packing) ---

    // Always present: UV scales for base texture pairs.
    b.push("uv_scale_t0_t1", 2);
    b.push("uv_scale_t2_t3", 2);

    // Normal intensity (absent in simplified/reduced texturing modes).
    if !SimplifiedTexturing.test(flags) && !ReducedTexturing.test(flags) {
        b.push("normal_intensity", 1);
    }

    // Extra texture layer UV scale.
    if ExtraTextureLayer.test(flags) || Emissive.test(flags) {
        b.push("uv_scale_t4", 1);
    }

    // Emissive intensity.
    if Emissive.test(flags)
        || (ExtraTextureLayer.test(flags) && (EmissiveSubA.test(flags) || EmissiveSubB.test(flags)))
    {
        b.push("emissive_intensity", 1);
    }

    // Per-channel UV scale.
    if PerChannelUv.test(flags) {
        b.push("uv_scale_t5", 1);
    }

    // Scroll animation params (requires Emissive).
    if ScrollAnim.test(flags) && Emissive.test(flags) {
        b.push("scroll_period", 1);
        b.push("scroll_phase", 1);
        b.push("fresnel_power", 1);
    }

    // --- Group B: Material overrides (starts at register boundary) ---

    let has_group_b = RoughnessChannel.test(flags) || MaterialOverride.test(flags);
    if has_group_b {
        b.align_to_register();

        // Roughness channel value (packed before material override params).
        if RoughnessChannel.test(flags) {
            b.push("roughness_channel_value", 1);
        }

        // Full material override block.
        if MaterialOverride.test(flags) {
            b.push("detail_blend", 1);
            b.push("roughness_override", 1);
            b.push("override_strength", 1);
            // float3 spec_override_color cannot straddle register boundary,
            // so it starts at the next register.
            b.align_to_register();
            b.push("spec_override_color", 3);
            b.push("override_bias", 1);
        }
    }

    let regs = b.register_count();
    (b.entries, regs)
}

/// Format the 64-bit flags as a zero-padded 16-char hex string.
pub(crate) fn flags_to_hex(flags: u64) -> String {
    format!("{flags:016x}")
}
