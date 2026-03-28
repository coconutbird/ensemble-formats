//! Infer semantic names for constant-buffer parameter accesses by analyzing
//! DXBC instruction data flow.
//!
//! The Hogan shader system packs artist-editable material parameters into
//! `rp_parameter_ps` (cb8) and `rp_parameter_vs` (cb7) as flat `float4[]`
//! arrays. The original names are compiled away — this module recovers them
//! by pattern-matching the instruction stream.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use d3dasm::dxbc::shex::{
    ComponentSelect, Instruction, Opcode, Operand, OperandIndex, Program, RegisterType,
};

/// A single identified parameter access.
#[derive(Debug, Clone)]
pub struct CbParam {
    /// Constant buffer slot (7 = VS, 8 = PS).
    pub cb_slot: u32,
    /// Register index within the CB array (e.g. 0, 1, 2).
    pub reg_index: u32,
    /// Component mask or swizzle string (e.g. "xy", "xxy", "z").
    pub components: String,
    /// Inferred semantic name (e.g. "uv_scale_albedo", "normal_intensity").
    pub semantic: String,
    /// Confidence: "high", "medium", "low".
    pub confidence: &'static str,
    /// Instruction index where this access occurs.
    pub insn_index: usize,
}

/// Analyze a shader program and return inferred CB parameter semantics.
pub fn infer_cb_params(program: &Program, ps_slot: u32, vs_slot: u32) -> Vec<CbParam> {
    let mut results = Vec::new();
    let insns = &program.instructions;

    for (i, insn) in insns.iter().enumerate() {
        let ops = insn.operands();
        if ops.is_empty() {
            continue;
        }

        // Skip declaration instructions — they define buffer sizes, not usage.
        if matches!(insn.opcode, Opcode::DclConstantBuffer) {
            continue;
        }

        for (op_idx, op) in ops.iter().enumerate() {
            if op.reg_type != RegisterType::ConstantBuffer {
                continue;
            }
            let (slot, reg) = match cb_indices(op) {
                Some(v) => v,
                None => continue,
            };
            if slot != ps_slot && slot != vs_slot {
                continue;
            }

            let comp = component_string(&op.components);
            let semantic = classify_usage(insns, i, op_idx, op, slot, ps_slot);

            results.push(CbParam {
                cb_slot: slot,
                reg_index: reg,
                components: comp,
                semantic: semantic.0,
                confidence: semantic.1,
                insn_index: i,
            });
        }
    }

    deduplicate(&mut results);
    results
}

/// Extract (cb_slot, register_index) from a CB operand.
fn cb_indices(op: &Operand) -> Option<(u32, u32)> {
    if op.indices.len() >= 2
        && let (OperandIndex::Imm32(slot), OperandIndex::Imm32(reg)) =
            (&op.indices[0], &op.indices[1])
    {
        return Some((*slot, *reg));
    }
    None
}

/// Convert component selection to a readable string.
fn component_string(cs: &ComponentSelect) -> String {
    let names = ['x', 'y', 'z', 'w'];
    match cs {
        ComponentSelect::ZeroComponent | ComponentSelect::OneComponent => String::new(),
        ComponentSelect::Mask(m) => {
            let mut s = String::new();
            for i in 0..4u8 {
                if m & (1 << i) != 0 {
                    s.push(names[i as usize]);
                }
            }
            s
        }
        ComponentSelect::Swizzle(sw) => sw.iter().map(|&c| names[c as usize]).collect(),
        ComponentSelect::Scalar(c) => names[*c as usize].to_string(),
    }
}

/// Classify what a CB access is used for by examining surrounding instructions.
fn classify_usage(
    insns: &[Instruction],
    insn_idx: usize,
    _op_idx: usize,
    _cb_op: &Operand,
    slot: u32,
    ps_slot: u32,
) -> (String, &'static str) {
    let insn = &insns[insn_idx];

    // Pattern 1: mul rN, ?, cb → UV scale (result feeds sample)
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3
            && let Some(dest) = ops.first()
            && feeds_sample(insns, insn_idx, dest)
        {
            let tex = find_sampled_textures(insns, insn_idx, dest);
            let conf = if has_texcoord_operand(ops) {
                "high"
            } else {
                "medium"
            };
            return (format!("uv_scale{}", tex.as_deref().unwrap_or("")), conf);
        }
    }

    // Pattern 2: mad rN, cb, normal_perturb, (1,1,1) → normal intensity
    if matches!(insn.opcode, Opcode::Mad) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 4 && has_ones_immediate(ops) {
            return (String::from("normal_intensity"), "high");
        }
    }

    // Pattern 3: mul rN, sampled_color, cb → emissive/tint multiplier
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3 {
            for src in &ops[1..] {
                if src.reg_type == RegisterType::Temp && was_sampled(insns, insn_idx, src) {
                    return (String::from("emissive_intensity"), "medium");
                }
            }
        }
    }

    // Pattern 4 (VS): mad output, cb, time, texcoord → UV scroll
    if matches!(insn.opcode, Opcode::Mad) && slot != ps_slot {
        let ops = insn.operands();
        if ops.len() >= 4
            && let Some(dest) = ops.first()
            && dest.reg_type == RegisterType::Output
        {
            return (String::from("uv_scroll"), "high");
        }
    }

    // Pattern 5: lt/ge near discard → alpha test threshold
    if matches!(insn.opcode, Opcode::Lt | Opcode::Ge) && slot == ps_slot {
        let lo = insn_idx.saturating_sub(3);
        let hi = insns.len().min(insn_idx + 3);
        if insns[lo..hi]
            .iter()
            .any(|i| matches!(i.opcode, Opcode::Discard))
        {
            return (String::from("alpha_test_ref"), "medium");
        }
    }

    // Pattern 6: log/exp Fresnel — mul cb in a log/exp sequence
    // Detected as: mul rN, temp, cb where the temp was written by log within 3 insns
    // and an exp follows within 3 insns.
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3 {
            let lo = insn_idx.saturating_sub(4);
            let hi = insns.len().min(insn_idx + 4);
            let has_log = insns[lo..insn_idx]
                .iter()
                .any(|i| matches!(i.opcode, Opcode::Log));
            let has_exp = insns[insn_idx + 1..hi]
                .iter()
                .any(|i| matches!(i.opcode, Opcode::Exp));
            if has_log && has_exp {
                return (String::from("fresnel_power"), "medium");
            }
        }
    }

    // Pattern 7: mad rN, -spec*normal, cb.xyz → spec override color
    // Detected as: mad where one source is negated and cb has 3+ components
    if matches!(insn.opcode, Opcode::Mad) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 4 {
            let has_neg = ops[1..].iter().any(|o| o.negate);
            let cb_comps = component_count(_cb_op);
            if has_neg && cb_comps >= 3 {
                return (String::from("spec_override_color"), "medium");
            }
        }
    }

    // Pattern 8: add rN, -sampled, cb → override value (roughness/spec)
    // The key is: add with a negated temp source and a scalar cb
    if matches!(insn.opcode, Opcode::Add) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3 {
            let has_neg_temp = ops[1..]
                .iter()
                .any(|o| o.reg_type == RegisterType::Temp && o.negate);
            if has_neg_temp {
                // Check if result feeds a mad that writes to o3 (roughness output)
                if feeds_output_near(insns, insn_idx, 3) {
                    return (String::from("roughness_override_value"), "medium");
                }
                return (String::from("override_value"), "low");
            }
        }
    }

    // Pattern 9: mul rN, vertex_alpha (v2.w), cb → blend/override strength
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3 {
            let has_input = ops[1..].iter().any(|o| o.reg_type == RegisterType::Input);
            if has_input && component_count(_cb_op) == 1 {
                // Check context: near roughness output = roughness_override_strength
                // near spec output = spec_override_strength
                if feeds_output_near(insns, insn_idx, 5) {
                    return (String::from("override_strength"), "low");
                }
            }
        }
    }

    // Pattern 10: add cb, -1 → detail blend factor
    // add rN, cb, l(-1.0) then mad rN, vertex_input, rN, l(1.0)
    if matches!(insn.opcode, Opcode::Add) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3 && has_neg_one_immediate(ops) {
            return (String::from("detail_blend_factor"), "low");
        }
    }

    // Pattern 11: mul rN, fresnel_result, cb → env reflection intensity
    // Detected as: mul involving cb where result mads into normal/view direction
    if matches!(insn.opcode, Opcode::Mul) && slot == ps_slot {
        let ops = insn.operands();
        if ops.len() >= 3 && component_count(_cb_op) == 1 {
            // Check if the result feeds a mad that adds to a direction vector
            if feeds_mad_addend(insns, insn_idx) {
                return (String::from("env_reflection_intensity"), "low");
            }
        }
    }

    (String::from("unknown"), "low")
}

fn has_texcoord_operand(ops: &[Operand]) -> bool {
    ops.iter().any(|op| op.reg_type == RegisterType::Input)
}

fn has_ones_immediate(ops: &[Operand]) -> bool {
    ops.iter().any(|op| {
        op.reg_type == RegisterType::Immediate32
            && op
                .immediate_values
                .iter()
                .any(|&v| f32::from_bits(v) == 1.0)
    })
}

fn has_neg_one_immediate(ops: &[Operand]) -> bool {
    ops.iter().any(|op| {
        op.reg_type == RegisterType::Immediate32
            && op
                .immediate_values
                .iter()
                .any(|&v| f32::from_bits(v) == -1.0)
    })
}

/// Count unique components in an operand's selection.
fn component_count(op: &Operand) -> u8 {
    match &op.components {
        ComponentSelect::Scalar(_) => 1,
        ComponentSelect::Mask(m) => m.count_ones() as u8,
        ComponentSelect::Swizzle(sw) => {
            let mut seen = 0u8;
            for &c in sw.iter() {
                seen |= 1 << c;
            }
            seen.count_ones() as u8
        }
        _ => 0,
    }
}

/// Check if an instruction's result feeds an output register within `window` insns.
fn feeds_output_near(insns: &[Instruction], from: usize, window: usize) -> bool {
    let dest = match insns[from].operands().first().and_then(temp_index) {
        Some(r) => r,
        None => return false,
    };
    for insn in &insns[from + 1..insns.len().min(from + 1 + window)] {
        let ops = insn.operands();
        for src in ops.iter().skip(1) {
            if temp_index(src) == Some(dest)
                && let Some(d) = ops.first()
                && d.reg_type == RegisterType::Output
            {
                return true;
            }
        }
    }
    false
}

/// Check if an instruction's result is used in a subsequent mad (any source position).
fn feeds_mad_addend(insns: &[Instruction], from: usize) -> bool {
    let dest = match insns[from].operands().first().and_then(temp_index) {
        Some(r) => r,
        None => return false,
    };
    for insn in &insns[from + 1..insns.len().min(from + 5)] {
        if matches!(insn.opcode, Opcode::Mad) {
            let ops = insn.operands();
            // Check all source operands (ops[1], ops[2], ops[3])
            if ops.len() >= 4 && ops[1..].iter().any(|o| temp_index(o) == Some(dest)) {
                return true;
            }
        }
    }
    false
}

fn feeds_sample(insns: &[Instruction], from: usize, dest: &Operand) -> bool {
    let dest_reg = match temp_index(dest) {
        Some(r) => r,
        None => return false,
    };
    let dest_comps = write_mask(dest);
    for insn in &insns[from + 1..insns.len().min(from + 12)] {
        if is_sample_op(insn.opcode) {
            let ops = insn.operands();
            if ops.len() >= 2 && temp_index(&ops[1]) == Some(dest_reg) {
                // Check that the sample actually reads components written by dest
                let read = read_mask(&ops[1]);
                if dest_comps & read != 0 {
                    return true;
                }
            }
        }
    }
    false
}

/// Find ALL texture slots sampled using a given dest register, filtering by
/// which components the dest actually wrote (fixes parallax slot confusion).
fn find_sampled_textures(insns: &[Instruction], from: usize, dest: &Operand) -> Option<String> {
    let dest_reg = temp_index(dest)?;
    let dest_comps = write_mask(dest);
    let mut slots: Vec<u32> = Vec::new();
    for insn in &insns[from + 1..insns.len().min(from + 12)] {
        if is_sample_op(insn.opcode) {
            let ops = insn.operands();
            if ops.len() >= 3 && temp_index(&ops[1]) == Some(dest_reg) {
                // Only match if the sample reads components we actually wrote
                let read = read_mask(&ops[1]);
                if dest_comps & read == 0 {
                    continue;
                }
                if ops[2].reg_type == RegisterType::Resource
                    && let Some(OperandIndex::Imm32(t)) = ops[2].indices.first()
                    && !slots.contains(t)
                {
                    slots.push(*t);
                }
            }
        }
    }
    if slots.is_empty() {
        return None;
    }
    let mut s = String::from("_t");
    for (i, t) in slots.iter().enumerate() {
        if i > 0 {
            s.push_str("_t");
        }
        s.push_str(&format!("{}", t));
    }
    Some(s)
}

/// Get the write mask of an operand as a bitmask (bit 0 = x, bit 1 = y, etc).
fn write_mask(op: &Operand) -> u8 {
    match &op.components {
        ComponentSelect::Mask(m) => *m,
        ComponentSelect::Scalar(c) => 1 << *c,
        ComponentSelect::Swizzle(sw) => {
            let mut m = 0u8;
            for &c in sw.iter() {
                m |= 1 << c;
            }
            m
        }
        _ => 0xF, // assume all
    }
}

/// Get the read mask of an operand (which components it actually reads).
fn read_mask(op: &Operand) -> u8 {
    match &op.components {
        ComponentSelect::Swizzle(sw) => {
            let mut m = 0u8;
            for &c in sw.iter() {
                m |= 1 << c;
            }
            m
        }
        ComponentSelect::Scalar(c) => 1 << *c,
        ComponentSelect::Mask(m) => *m,
        _ => 0xF,
    }
}

fn was_sampled(insns: &[Instruction], before: usize, src: &Operand) -> bool {
    let src_reg = match temp_index(src) {
        Some(r) => r,
        None => return false,
    };
    for insn in &insns[before.saturating_sub(8)..before] {
        if is_sample_op(insn.opcode) {
            let ops = insn.operands();
            if !ops.is_empty() && temp_index(&ops[0]) == Some(src_reg) {
                return true;
            }
        }
    }
    false
}

fn is_sample_op(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::Sample
            | Opcode::SampleL
            | Opcode::SampleD
            | Opcode::SampleB
            | Opcode::SampleC
            | Opcode::SampleCLz
    )
}

fn temp_index(op: &Operand) -> Option<u32> {
    if op.reg_type == RegisterType::Temp
        && let Some(OperandIndex::Imm32(r)) = op.indices.first()
    {
        return Some(*r);
    }
    None
}

fn deduplicate(results: &mut Vec<CbParam>) {
    let rank = |c: &str| -> u8 {
        match c {
            "high" => 3,
            "medium" => 2,
            _ => 1,
        }
    };
    let mut best: BTreeMap<(u32, u32, String), usize> = BTreeMap::new();
    for (i, p) in results.iter().enumerate() {
        let key = (p.cb_slot, p.reg_index, p.components.clone());
        match best.get(&key) {
            Some(&e) if rank(p.confidence) > rank(results[e].confidence) => {
                best.insert(key, i);
            }
            None => {
                best.insert(key, i);
            }
            _ => {}
        }
    }
    let keep: alloc::collections::BTreeSet<usize> = best.values().copied().collect();
    let mut i = 0;
    results.retain(|_| {
        let k = keep.contains(&i);
        i += 1;
        k
    });
}
