//! CFG-aware context for analyzing instructions within basic blocks.

use alloc::vec::Vec;

use cfglib::Cfg;
use cfglib::block::BlockId;
use d3dasm::dxbc::shex::{Instruction, Opcode, Operand, OperandIndex, RegisterType};

use super::instruction::Sm4Instruction;
use super::operand::{is_sample_op, read_mask, temp_index, write_mask};

/// Context for analyzing an instruction within a basic block of the CFG.
pub(super) struct BlockCtx<'a> {
    pub cfg: &'a Cfg<Sm4Instruction>,
    pub block_id: BlockId,
    pub local_idx: usize,
}

impl<'a> BlockCtx<'a> {
    /// Instructions in the current block.
    pub fn block_insns(&self) -> &'a [Sm4Instruction] {
        self.cfg.block(self.block_id).instructions()
    }

    /// The current instruction.
    pub fn insn(&self) -> &'a Instruction {
        self.block_insns()[self.local_idx].instruction()
    }

    /// Instructions *after* the current one in this block.
    pub fn later_in_block(&self) -> &'a [Sm4Instruction] {
        &self.block_insns()[self.local_idx + 1..]
    }

    /// Instructions *before* the current one in this block.
    pub fn earlier_in_block(&self) -> &'a [Sm4Instruction] {
        &self.block_insns()[..self.local_idx]
    }

    /// Collect instructions from successor blocks (one level deep).
    pub fn successor_insns(&self) -> Vec<&'a Instruction> {
        let mut out = Vec::new();
        for succ_id in self.cfg.successors(self.block_id) {
            for sm4 in self.cfg.block(succ_id).instructions() {
                out.push(sm4.instruction());
            }
        }
        out
    }

    /// Collect instructions from predecessor blocks (one level deep).
    pub fn predecessor_insns(&self) -> Vec<&'a Instruction> {
        let mut out = Vec::new();
        for pred_id in self.cfg.predecessors(self.block_id) {
            for sm4 in self.cfg.block(pred_id).instructions() {
                out.push(sm4.instruction());
            }
        }
        out
    }
}

/// Check if the current instruction's result feeds an output register,
/// searching within the block and then one level of CFG successors.
pub(super) fn feeds_output(ctx: &BlockCtx<'_>) -> bool {
    let Some(dest) = ctx.insn().operands().first().and_then(temp_index) else {
        return false;
    };
    for sm4 in ctx.later_in_block() {
        let ops = sm4.instruction().operands();
        for src in ops.iter().skip(1) {
            if temp_index(src) == Some(dest)
                && let Some(d) = ops.first()
                && d.reg_type == RegisterType::Output
            {
                return true;
            }
        }
    }
    for insn in ctx.successor_insns() {
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

/// Check if the current instruction's result is used in a subsequent mad.
pub(super) fn feeds_mad_addend(ctx: &BlockCtx<'_>) -> bool {
    let Some(dest) = ctx.insn().operands().first().and_then(temp_index) else {
        return false;
    };
    for sm4 in ctx.later_in_block() {
        if matches!(sm4.instruction().opcode, Opcode::Mad) {
            let ops = sm4.instruction().operands();
            if ops.len() >= 4 && ops[1..].iter().any(|o| temp_index(o) == Some(dest)) {
                return true;
            }
        }
    }
    for insn in ctx.successor_insns() {
        if matches!(insn.opcode, Opcode::Mad) {
            let ops = insn.operands();
            if ops.len() >= 4 && ops[1..].iter().any(|o| temp_index(o) == Some(dest)) {
                return true;
            }
        }
    }
    false
}

/// Check if the dest register feeds a sample instruction.
pub(super) fn feeds_sample(ctx: &BlockCtx<'_>, dest: &Operand) -> bool {
    let Some(dest_reg) = temp_index(dest) else {
        return false;
    };
    let dest_comps = write_mask(dest);
    for sm4 in ctx.later_in_block() {
        if check_sample_use(sm4.instruction(), dest_reg, dest_comps) {
            return true;
        }
    }
    for insn in ctx.successor_insns() {
        if check_sample_use(insn, dest_reg, dest_comps) {
            return true;
        }
    }
    false
}

/// Helper: does this instruction sample using the given register + components?
fn check_sample_use(insn: &Instruction, dest_reg: u32, dest_comps: u8) -> bool {
    if is_sample_op(insn.opcode) {
        let ops = insn.operands();
        if ops.len() >= 2 && temp_index(&ops[1]) == Some(dest_reg) {
            let read = read_mask(&ops[1]);
            if dest_comps & read != 0 {
                return true;
            }
        }
    }
    false
}

/// Find ALL texture resource slots sampled using a given dest register.
pub(super) fn find_sampled_texture_slots(ctx: &BlockCtx<'_>, dest: &Operand) -> Vec<u32> {
    let Some(dest_reg) = temp_index(dest) else {
        return Vec::new();
    };
    let dest_comps = write_mask(dest);
    let mut slots: Vec<u32> = Vec::new();

    let mut collect = |insn: &Instruction| {
        if is_sample_op(insn.opcode) {
            let ops = insn.operands();
            if ops.len() >= 3 && temp_index(&ops[1]) == Some(dest_reg) {
                let read = read_mask(&ops[1]);
                if dest_comps & read != 0
                    && ops[2].reg_type == RegisterType::Resource
                    && let Some(OperandIndex::Imm32(t)) = ops[2].indices.first()
                    && !slots.contains(t)
                {
                    slots.push(*t);
                }
            }
        }
    };

    for sm4 in ctx.later_in_block() {
        collect(sm4.instruction());
    }
    for insn in ctx.successor_insns() {
        collect(insn);
    }
    slots
}

/// Check if a register was written by a sample instruction (looking backward).
pub(super) fn was_sampled(ctx: &BlockCtx<'_>, src: &Operand) -> bool {
    let Some(src_reg) = temp_index(src) else {
        return false;
    };
    for sm4 in ctx.earlier_in_block() {
        if is_sample_op(sm4.instruction().opcode) {
            let ops = sm4.instruction().operands();
            if !ops.is_empty() && temp_index(&ops[0]) == Some(src_reg) {
                return true;
            }
        }
    }
    for insn in ctx.predecessor_insns() {
        if is_sample_op(insn.opcode) {
            let ops = insn.operands();
            if !ops.is_empty() && temp_index(&ops[0]) == Some(src_reg) {
                return true;
            }
        }
    }
    false
}
