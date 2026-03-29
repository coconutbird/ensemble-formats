//! Pattern dispatch — calls each sub-module in priority order.
//!
//! To add a new pattern group, create a new file in this directory and add
//! a call to its `try_classify` function in the chain below.

mod blend;
mod gradient;
mod material;
mod uv;
mod vertex;

use d3dasm::dxbc::shex::Operand;

use super::context::BlockCtx;
use super::types::{Confidence, Semantic, SemanticKind};

/// Classify a CB access by trying each pattern group in priority order.
///
/// Returns the first `(Semantic, Confidence)` that matches, or
/// `(Unknown, Low)` if nothing matched.
pub(super) fn classify_usage(
    ctx: &BlockCtx<'_>,
    _op_idx: usize,
    cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> (Semantic, Confidence) {
    // High-priority: UV patterns (most specific patterns first)
    if let Some(result) = uv::try_classify(ctx, cb_op, slot, ps_slot) {
        return result;
    }

    // Material / surface property patterns
    if let Some(result) = material::try_classify(ctx, cb_op, slot, ps_slot) {
        return result;
    }

    // Blend / dissolve / depth patterns
    if let Some(result) = blend::try_classify(ctx, cb_op, slot, ps_slot) {
        return result;
    }

    // VS-specific: scroll, displacement, tint, animation
    if let Some(result) = vertex::try_classify(ctx, cb_op, slot, ps_slot) {
        return result;
    }

    // VS-specific: height blending, color gradients
    if let Some(result) = gradient::try_classify(ctx, cb_op, slot, ps_slot) {
        return result;
    }

    (Semantic::new(SemanticKind::Unknown), Confidence::Low)
}
