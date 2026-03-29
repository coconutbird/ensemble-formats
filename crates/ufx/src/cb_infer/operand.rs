//! Small operand utility functions used by pattern matching and context helpers.

use alloc::string::String;

use d3dasm::dxbc::shex::{ComponentSelect, Opcode, Operand, OperandIndex, RegisterType};

/// Extract (cb_slot, register_index) from a CB operand.
pub(super) fn cb_indices(op: &Operand) -> Option<(u32, u32)> {
    if op.indices.len() >= 2
        && let (OperandIndex::Imm32(slot), OperandIndex::Imm32(reg)) =
            (&op.indices[0], &op.indices[1])
    {
        return Some((*slot, *reg));
    }
    None
}

/// Convert component selection to a readable string.
pub(super) fn component_string(cs: &ComponentSelect) -> String {
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
        ComponentSelect::Scalar(c) => {
            let mut s = String::new();
            s.push(names[*c as usize]);
            s
        }
    }
}

/// Check if any operand is a vertex/pixel input register (texcoord, etc).
pub(super) fn has_texcoord_operand(ops: &[Operand]) -> bool {
    ops.iter().any(|op| op.reg_type == RegisterType::Input)
}

/// Check if an operand reads from the DefaultXSC Time register (`cb0[21]`).
/// In Hogan shaders, Time is at cbuffer slot 0, register 21.
pub(super) fn is_default_xsc_time(op: &Operand) -> bool {
    op.reg_type == RegisterType::ConstantBuffer
        && op.indices.len() >= 2
        && matches!(op.indices[0], OperandIndex::Imm32(0))
        && matches!(op.indices[1], OperandIndex::Imm32(21))
}

/// Check if any operand is an immediate with value `1.0`.
pub(super) fn has_ones_immediate(ops: &[Operand]) -> bool {
    ops.iter().any(|op| {
        op.reg_type == RegisterType::Immediate32
            && op
                .immediate_values
                .iter()
                .any(|&v| f32::from_bits(v) == 1.0)
    })
}

/// Check if any operand is an immediate with value `-1.0`.
pub(super) fn has_neg_one_immediate(ops: &[Operand]) -> bool {
    ops.iter().any(|op| {
        op.reg_type == RegisterType::Immediate32
            && op
                .immediate_values
                .iter()
                .any(|&v| f32::from_bits(v) == -1.0)
    })
}

/// Count unique components in an operand's selection.
pub(super) fn component_count(op: &Operand) -> u8 {
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

/// Get the write mask of an operand as a bitmask (bit 0 = x, bit 1 = y, etc).
pub(super) fn write_mask(op: &Operand) -> u8 {
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
pub(super) fn read_mask(op: &Operand) -> u8 {
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

/// Check if an opcode is a texture sample operation.
pub(super) fn is_sample_op(op: Opcode) -> bool {
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

/// Extract the temp register index from an operand.
pub(super) fn temp_index(op: &Operand) -> Option<u32> {
    if op.reg_type == RegisterType::Temp
        && let Some(OperandIndex::Imm32(r)) = op.indices.first()
    {
        return Some(*r);
    }
    None
}
