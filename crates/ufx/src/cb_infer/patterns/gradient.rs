//! VS gradient patterns: height blend range, color gradient endpoints.

use alloc::vec::Vec;

use d3dasm::dxbc::shex::{Opcode, Operand, OperandIndex, RegisterType};

use crate::cb_infer::context::BlockCtx;
use crate::cb_infer::operand::is_default_xsc_time;
use crate::cb_infer::types::{Confidence, Semantic, SemanticKind};

pub(crate) fn try_classify(
    ctx: &BlockCtx<'_>,
    cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> Option<(Semantic, Confidence)> {
    if slot == ps_slot {
        return None; // VS-only patterns
    }

    let insn = ctx.insn();
    let ops = insn.operands();

    // Pattern 27 (VS): add rN, -cb, cb (same slot, different comps) → height blend range
    if matches!(insn.opcode, Opcode::Add) && ops.len() >= 3 {
        let cb_ops: Vec<_> = ops[1..]
            .iter()
            .filter(|o| {
                o.reg_type == RegisterType::ConstantBuffer
                    && o.indices.len() >= 2
                    && matches!(o.indices[0], OperandIndex::Imm32(s) if s == slot)
            })
            .collect();
        if cb_ops.len() >= 2 && cb_ops.iter().any(|o| o.negate) {
            return Some((
                Semantic::new(SemanticKind::HeightBlendRange),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 28 (VS): add rN, v0.y (position), -cb → height blend min
    if matches!(insn.opcode, Opcode::Add) && ops.len() >= 3 && cb_op.negate {
        let has_position = ops[1..].iter().any(|o| {
            o.reg_type == RegisterType::Input
                && matches!(o.indices.first(), Some(OperandIndex::Imm32(0)))
        });
        if has_position {
            return Some((
                Semantic::new(SemanticKind::HeightBlendRange),
                Confidence::High,
            ));
        }
    }

    // Pattern 29 (VS): add rN, -cb[A], cb[B] (different registers) → color gradient
    if matches!(insn.opcode, Opcode::Add) && ops.len() >= 3 {
        let has_neg_cb = ops[1..].iter().any(|o| {
            o.reg_type == RegisterType::ConstantBuffer
                && o.negate
                && o.indices.len() >= 2
                && matches!(o.indices[0], OperandIndex::Imm32(s) if s == slot)
        });
        let has_pos_cb = ops[1..].iter().any(|o| {
            o.reg_type == RegisterType::ConstantBuffer
                && !o.negate
                && o.indices.len() >= 2
                && matches!(o.indices[0], OperandIndex::Imm32(s) if s == slot)
        });
        if has_neg_cb && has_pos_cb {
            return Some((
                Semantic::new(SemanticKind::ColorGradient),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 30 (VS): mad output, rN, rN, cb → color gradient (lerp target)
    // Skip if Time is present (that would be Pattern 4 = uv_scroll)
    if matches!(insn.opcode, Opcode::Mad)
        && ops.len() >= 4
        && let Some(dest) = ops.first()
        && dest.reg_type == RegisterType::Output
    {
        let has_time = ops[1..].iter().any(is_default_xsc_time);
        if !has_time {
            return Some((
                Semantic::new(SemanticKind::ColorGradient),
                Confidence::Medium,
            ));
        }
    }

    None
}
