//! UV-related patterns: scale, scroll, scroll speed, scroll phase, scroll period.

use d3dasm::dxbc::shex::{Opcode, Operand, RegisterType};

use crate::cb_infer::context::{BlockCtx, feeds_sample, find_sampled_texture_slots};
use crate::cb_infer::operand::{
    has_neg_one_immediate, has_ones_immediate, has_texcoord_operand, is_default_xsc_time,
};
use crate::cb_infer::types::{Confidence, Semantic, SemanticKind};

pub(crate) fn try_classify(
    ctx: &BlockCtx<'_>,
    _cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> Option<(Semantic, Confidence)> {
    let insn = ctx.insn();
    let ops = insn.operands();

    // Pattern 1: mul rN, ?, cb → UV scale (result feeds sample)
    if matches!(insn.opcode, Opcode::Mul)
        && slot == ps_slot
        && ops.len() >= 3
        && let Some(dest) = ops.first()
        && feeds_sample(ctx, dest)
    {
        let slots = find_sampled_texture_slots(ctx, dest);
        let conf = if has_texcoord_operand(ops) {
            Confidence::High
        } else {
            Confidence::Medium
        };
        return Some((Semantic::with_slots(SemanticKind::UvScale, slots), conf));
    }

    // Pattern 1b: mad rN, texcoord, cb, temp → UV scale (result feeds sample)
    // Layered shaders: mad r.xy, v4.xy, cb[N].xy, r.xy
    if matches!(insn.opcode, Opcode::Mad)
        && slot == ps_slot
        && ops.len() >= 4
        && !has_ones_immediate(ops)
        && !has_neg_one_immediate(ops)
        && has_texcoord_operand(ops)
        && let Some(dest) = ops.first()
        && feeds_sample(ctx, dest)
    {
        let slots = find_sampled_texture_slots(ctx, dest);
        return Some((
            Semantic::with_slots(SemanticKind::UvScale, slots),
            Confidence::High,
        ));
    }

    // Pattern 12 (PS): mad rN, Time, cb, texcoord → UV scroll speed
    if matches!(insn.opcode, Opcode::Mad) && slot == ps_slot && ops.len() >= 4 {
        let has_default_xsc = ops[1..].iter().any(is_default_xsc_time);
        let has_input = ops[1..].iter().any(|o| o.reg_type == RegisterType::Input);
        if has_default_xsc && has_input {
            return Some((Semantic::new(SemanticKind::UvScrollSpeed), Confidence::High));
        }
        // Time * cb without explicit input (input may come from earlier)
        if has_default_xsc && !has_input {
            return Some((
                Semantic::new(SemanticKind::UvScrollSpeed),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 13: mul rN, cb, Time → animation speed
    if matches!(insn.opcode, Opcode::Mul)
        && slot == ps_slot
        && ops.len() >= 3
        && ops[1..].iter().any(is_default_xsc_time)
    {
        return Some((Semantic::new(SemanticKind::AnimSpeed), Confidence::Medium));
    }

    // Pattern 23 (PS): add rN, cb, cb0[21] → scroll phase
    if matches!(insn.opcode, Opcode::Add)
        && slot == ps_slot
        && ops.len() >= 3
        && ops[1..].iter().any(is_default_xsc_time)
    {
        return Some((Semantic::new(SemanticKind::ScrollPhase), Confidence::Medium));
    }

    // Pattern 24 (PS): mad rN, -rN, cb, texcoord → scroll frequency
    // The negated temp carries Time+phase, cb is the frequency
    if matches!(insn.opcode, Opcode::Mad) && slot == ps_slot && ops.len() >= 4 {
        let has_neg_temp = ops[1..]
            .iter()
            .any(|o| o.reg_type == RegisterType::Temp && o.negate);
        let has_input = ops[1..].iter().any(|o| o.reg_type == RegisterType::Input);
        if has_neg_temp && has_input && !has_ones_immediate(ops) {
            return Some((
                Semantic::new(SemanticKind::UvScrollSpeed),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 32 (PS): div rN, |rN|, cb → scroll wave period
    if matches!(insn.opcode, Opcode::Div) && !insn.saturate && slot == ps_slot && ops.len() >= 3 {
        let has_abs_temp = ops[1..]
            .iter()
            .any(|o| o.reg_type == RegisterType::Temp && o.abs);
        if has_abs_temp {
            return Some((
                Semantic::new(SemanticKind::ScrollPeriod),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 33 (PS): dp2 rN, rN, cb → scroll period
    if matches!(insn.opcode, Opcode::Dp2) && slot == ps_slot {
        return Some((Semantic::new(SemanticKind::ScrollPeriod), Confidence::Low));
    }

    None
}
