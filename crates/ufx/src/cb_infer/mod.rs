//! Infer semantic names for constant-buffer parameter accesses by analyzing
//! DXBC instruction data flow using a control-flow graph.
//!
//! The Hogan shader system packs artist-editable material parameters into
//! `rp_parameter_ps` ([`HOGAN_PS_SLOT`] = cb8) and `rp_parameter_vs`
//! ([`HOGAN_VS_SLOT`] = cb7) as flat `float4[]` arrays.  The original names
//! are compiled away — this module recovers them by pattern-matching
//! instruction usage within a [`cfglib::Cfg`].
//!
//! # Usage
//!
//! ```ignore
//! let ufx = ufx::parse(&data)?;
//! if let Some(ps) = ufx.pixel_shaders.first()
//!     && let Some(prog) = ps.program()
//! {
//!     let params = ufx::cb_infer::infer_cb_params(prog, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
//!     for p in &params {
//!         println!("{}: {} [{}]", p.semantic, p.components, p.confidence);
//!     }
//! }
//! ```

pub mod bitflags;
mod context;
mod operand;
mod patterns;
pub mod types;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use d3dasm::dxbc::shex::{Opcode, Program, RegisterType};

pub use types::{CbParam, Confidence, HOGAN_PS_SLOT, HOGAN_VS_SLOT, Semantic, SemanticKind};

use context::BlockCtx;
use operand::{cb_indices, component_string};
use patterns::classify_usage;

/// Analyze a shader program and return inferred CB parameter semantics.
///
/// `ps_slot` and `vs_slot` identify the constant-buffer slots to inspect
/// (typically [`HOGAN_PS_SLOT`] and [`HOGAN_VS_SLOT`]).
///
/// This builds a control-flow graph from the program and traverses each
/// basic block, using CFG successor/predecessor edges for data-flow
/// analysis instead of fixed-size index windows.
pub fn infer_cb_params(program: &Program, ps_slot: u32, vs_slot: u32) -> Vec<CbParam> {
    let cfg = match cfglib_dxbc::build_cfg(program) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();
    let mut global_insn_idx: usize = 0;

    for block in cfg.blocks() {
        let block_insns = block.instructions();

        for (local_idx, sm4_insn) in block_insns.iter().enumerate() {
            let insn = &sm4_insn.0;
            let ops = insn.operands();
            if ops.is_empty() {
                global_insn_idx += 1;
                continue;
            }

            // Skip declaration instructions — they define buffer sizes, not usage.
            if matches!(insn.opcode, Opcode::DclConstantBuffer) {
                global_insn_idx += 1;
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
                let ctx = BlockCtx {
                    cfg: &cfg,
                    block_id: block.id(),
                    local_idx,
                };
                let (semantic, confidence) = classify_usage(&ctx, op_idx, op, slot, ps_slot);

                results.push(CbParam {
                    cb_slot: slot,
                    reg_index: reg,
                    components: comp,
                    semantic,
                    confidence,
                    insn_index: global_insn_idx,
                });
            }
            global_insn_idx += 1;
        }
    }

    deduplicate(&mut results);
    results
}

fn deduplicate(results: &mut Vec<CbParam>) {
    let mut best: BTreeMap<(u32, u32, alloc::string::String), usize> = BTreeMap::new();
    for (i, p) in results.iter().enumerate() {
        let key = (p.cb_slot, p.reg_index, p.components.clone());
        match best.get(&key) {
            Some(&e) if p.confidence > results[e].confidence => {
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
