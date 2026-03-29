//! Blend/dissolve patterns: alpha test, fresnel bias, view angle cutoff,
//! soft depth, edge fade, dissolve, opacity blend, depth fade color.

use d3dasm::dxbc::shex::{ComponentSelect, Opcode, Operand, RegisterType};

use crate::cb_infer::context::BlockCtx;
use crate::cb_infer::operand::{component_count, has_ones_immediate};
use crate::cb_infer::types::{Confidence, Semantic, SemanticKind};

pub(crate) fn try_classify(
    ctx: &BlockCtx<'_>,
    cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> Option<(Semantic, Confidence)> {
    let insn = ctx.insn();
    let ops = insn.operands();

    // Pattern 5: lt/ge near discard → alpha test threshold
    if matches!(insn.opcode, Opcode::Lt | Opcode::Ge) && slot == ps_slot {
        let block_insns = ctx.block_insns();
        let has_discard = block_insns
            .iter()
            .any(|sm4| matches!(sm4.0.opcode, Opcode::Discard));
        if has_discard {
            return Some((
                Semantic::new(SemanticKind::AlphaTestRef),
                Confidence::Medium,
            ));
        }
        if ctx
            .successor_insns()
            .iter()
            .any(|i| matches!(i.opcode, Opcode::Discard))
        {
            return Some((Semantic::new(SemanticKind::AlphaTestRef), Confidence::Low));
        }
    }

    // Pattern 14: mad_sat rN, |v0.w|*cb, -cb → Fresnel bias
    if matches!(insn.opcode, Opcode::Mad) && insn.saturate && slot == ps_slot && cb_op.negate {
        return Some((Semantic::new(SemanticKind::FresnelBias), Confidence::Medium));
    }

    // Pattern 15: add_sat rN, |src|, -cb → view-angle cutoff
    if matches!(insn.opcode, Opcode::Add) && insn.saturate && slot == ps_slot && cb_op.negate {
        return Some((
            Semantic::new(SemanticKind::ViewAngleCutoff),
            Confidence::Medium,
        ));
    }

    // Pattern 16: div_sat rN, rN, cb → soft depth range
    if matches!(insn.opcode, Opcode::Div) && insn.saturate && slot == ps_slot {
        return Some((
            Semantic::new(SemanticKind::SoftDepthRange),
            Confidence::Medium,
        ));
    }

    // Pattern 17: mul rN.xyzw, rN, cb.xyzw → depth fade color (4-component)
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot && component_count(cb_op) == 4 {
        return Some((
            Semantic::new(SemanticKind::DepthFadeColor),
            Confidence::Medium,
        ));
    }

    // Pattern 21 (PS): add rN, cb, l(1.0) → edge fade
    if matches!(insn.opcode, Opcode::Add)
        && slot == ps_slot
        && ops.len() >= 3
        && has_ones_immediate(ops)
    {
        return Some((Semantic::new(SemanticKind::EdgeFade), Confidence::Medium));
    }

    // Pattern 25 (PS): mul rN, v0.w, cb → opacity/blend factor (view-angle)
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot && ops.len() >= 3 {
        let has_view_angle = ops[1..].iter().any(|o| {
            o.reg_type == RegisterType::Input
                && matches!(&o.components, ComponentSelect::Swizzle(sw) if sw.iter().all(|&c| c == 3))
        });
        if has_view_angle {
            return Some((
                Semantic::new(SemanticKind::OpacityBlend),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 26 (PS): add rN, rN, -cb → dissolve width
    if matches!(insn.opcode, Opcode::Add)
        && slot == ps_slot
        && ops.len() >= 3
        && cb_op.negate
        && ops[1..]
            .iter()
            .any(|o| o.reg_type == RegisterType::Temp && !o.negate)
    {
        return Some((Semantic::new(SemanticKind::DissolveWidth), Confidence::Low));
    }

    // Pattern 22 (PS): add rN, rN, cb (non-negated) → dissolve threshold
    // NOTE: Broad catch-all — placed last intentionally. May produce false positives.
    if matches!(insn.opcode, Opcode::Add)
        && slot == ps_slot
        && ops.len() >= 3
        && !cb_op.negate
        && ops[1..]
            .iter()
            .any(|o| o.reg_type == RegisterType::Temp && !o.negate)
    {
        return Some((
            Semantic::new(SemanticKind::DissolveThreshold),
            Confidence::Low,
        ));
    }

    None
}
