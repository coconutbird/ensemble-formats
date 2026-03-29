//! Vertex-shader patterns: UV scroll, color tint, vertex displacement,
//! vertex animation, edge fade.

use d3dasm::dxbc::shex::{Opcode, Operand, OperandIndex, RegisterType};

use crate::cb_infer::context::BlockCtx;
use crate::cb_infer::operand::is_default_xsc_time;
use crate::cb_infer::types::{Confidence, Semantic, SemanticKind};

pub(crate) fn try_classify(
    ctx: &BlockCtx<'_>,
    _cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> Option<(Semantic, Confidence)> {
    if slot == ps_slot {
        return None; // VS-only patterns
    }

    let insn = ctx.insn();
    let ops = insn.operands();

    // Pattern 4 (VS): mad output, cb, time, texcoord → UV scroll
    // Only match when Time (cb0[21]) is a source OR an input register is present.
    if matches!(insn.opcode, Opcode::Mad)
        && ops.len() >= 4
        && let Some(dest) = ops.first()
        && dest.reg_type == RegisterType::Output
        && (ops[1..].iter().any(is_default_xsc_time)
            || ops[1..].iter().any(|o| o.reg_type == RegisterType::Input))
    {
        return Some((Semantic::new(SemanticKind::UvScroll), Confidence::High));
    }

    // Pattern 18 (VS): mov output, cb → color tint passthrough
    if matches!(insn.opcode, Opcode::Mov)
        && ops.len() >= 2
        && let Some(dest) = ops.first()
        && dest.reg_type == RegisterType::Output
    {
        return Some((Semantic::new(SemanticKind::ColorTint), Confidence::Medium));
    }

    // Pattern 19 (VS): mad rN, rN, cb, position → vertex displacement
    if matches!(insn.opcode, Opcode::Mad) && ops.len() >= 4 {
        let has_position_input = ops[1..].iter().any(|o| {
            o.reg_type == RegisterType::Input
                && matches!(o.indices.first(), Some(OperandIndex::Imm32(0)))
        });
        if has_position_input {
            return Some((
                Semantic::new(SemanticKind::VertexDisplacement),
                Confidence::High,
            ));
        }
    }

    // Pattern 20 (VS): div_sat rN, rN, cb → VS scale factor / edge fade
    if matches!(insn.opcode, Opcode::Div) && insn.saturate {
        return Some((Semantic::new(SemanticKind::EdgeFade), Confidence::Low));
    }

    // Pattern 31 (VS): mul/mad rN near sincos → vertex animation parameter
    if matches!(insn.opcode, Opcode::Mul | Opcode::Mad) {
        let has_sincos = ctx
            .later_in_block()
            .iter()
            .any(|sm4| matches!(sm4.0.opcode, Opcode::Sincos))
            || ctx
                .earlier_in_block()
                .iter()
                .any(|sm4| matches!(sm4.0.opcode, Opcode::Sincos))
            || ctx
                .successor_insns()
                .iter()
                .any(|i| matches!(i.opcode, Opcode::Sincos))
            || ctx
                .predecessor_insns()
                .iter()
                .any(|i| matches!(i.opcode, Opcode::Sincos));
        if has_sincos {
            return Some((
                Semantic::new(SemanticKind::VertexAnimation),
                Confidence::Medium,
            ));
        }
    }

    None
}
