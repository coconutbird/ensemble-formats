//! Material patterns: normal intensity, emissive, fresnel, spec override,
//! roughness, detail blend, env reflection.

use d3dasm::dxbc::shex::{Opcode, Operand, RegisterType};

use crate::cb_infer::context::{BlockCtx, feeds_mad_addend, feeds_output, was_sampled};
use crate::cb_infer::operand::{component_count, has_neg_one_immediate, has_ones_immediate};
use crate::cb_infer::types::{Confidence, Semantic, SemanticKind};

pub(crate) fn try_classify(
    ctx: &BlockCtx<'_>,
    cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> Option<(Semantic, Confidence)> {
    let insn = ctx.insn();
    let ops = insn.operands();

    // Pattern 2: mad rN, cb, normal_perturb, (1,1,1) → normal intensity
    if matches!(insn.opcode, Opcode::Mad)
        && slot == ps_slot
        && ops.len() >= 4
        && has_ones_immediate(ops)
    {
        return Some((
            Semantic::new(SemanticKind::NormalIntensity),
            Confidence::High,
        ));
    }

    // Pattern 3: mul rN, sampled_color, cb → emissive/tint multiplier
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot && ops.len() >= 3 {
        for src in &ops[1..] {
            if src.reg_type == RegisterType::Temp && was_sampled(ctx, src) {
                return Some((
                    Semantic::new(SemanticKind::EmissiveIntensity),
                    Confidence::Medium,
                ));
            }
        }
    }

    // Pattern 6: log/exp Fresnel — mul cb in a log/exp sequence
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot && ops.len() >= 3 {
        let has_log = ctx
            .earlier_in_block()
            .iter()
            .any(|sm4| matches!(sm4.0.opcode, Opcode::Log))
            || ctx
                .predecessor_insns()
                .iter()
                .any(|i| matches!(i.opcode, Opcode::Log));
        let has_exp = ctx
            .later_in_block()
            .iter()
            .any(|sm4| matches!(sm4.0.opcode, Opcode::Exp))
            || ctx
                .successor_insns()
                .iter()
                .any(|i| matches!(i.opcode, Opcode::Exp));
        if has_log && has_exp {
            return Some((
                Semantic::new(SemanticKind::FresnelPower),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 7: mad rN, -spec*normal, cb.xyz → spec override color
    if matches!(insn.opcode, Opcode::Mad) && slot == ps_slot && ops.len() >= 4 {
        let has_neg = ops[1..].iter().any(|o| o.negate);
        let cb_comps = component_count(cb_op);
        if has_neg && cb_comps >= 3 {
            return Some((
                Semantic::new(SemanticKind::SpecOverrideColor),
                Confidence::Medium,
            ));
        }
    }

    // Pattern 8: add rN, -sampled, cb → override value (roughness/spec)
    if matches!(insn.opcode, Opcode::Add) && slot == ps_slot && ops.len() >= 3 {
        let has_neg_temp = ops[1..]
            .iter()
            .any(|o| o.reg_type == RegisterType::Temp && o.negate);
        if has_neg_temp {
            if feeds_output(ctx) {
                return Some((
                    Semantic::new(SemanticKind::RoughnessOverride),
                    Confidence::Medium,
                ));
            }
            return Some((Semantic::new(SemanticKind::OverrideValue), Confidence::Low));
        }
    }

    // Pattern 9: mul rN, vertex_alpha (v2.w), cb → blend/override strength
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot && ops.len() >= 3 {
        let has_input = ops[1..].iter().any(|o| o.reg_type == RegisterType::Input);
        if has_input && component_count(cb_op) == 1 && feeds_output(ctx) {
            return Some((
                Semantic::new(SemanticKind::OverrideStrength),
                Confidence::Low,
            ));
        }
    }

    // Pattern 10: add cb, -1 → detail blend factor
    if matches!(insn.opcode, Opcode::Add)
        && slot == ps_slot
        && ops.len() >= 3
        && has_neg_one_immediate(ops)
    {
        return Some((
            Semantic::new(SemanticKind::DetailBlendFactor),
            Confidence::Low,
        ));
    }

    // Pattern 11: mul rN, fresnel_result, cb → env reflection intensity
    if matches!(insn.opcode, Opcode::Mul)
        && slot == ps_slot
        && ops.len() >= 3
        && component_count(cb_op) == 1
        && feeds_mad_addend(ctx)
    {
        return Some((
            Semantic::new(SemanticKind::EnvReflectionIntensity),
            Confidence::Low,
        ));
    }

    None
}
