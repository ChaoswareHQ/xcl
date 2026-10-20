//! Data flow analysis: basic block construction, reaching definitions, and live variable analysis.
//!
//! Provides the foundation for dead store elimination and constant propagation.

#[cfg(not(feature = "std"))]
extern crate alloc;


// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{
    vec::Vec,
};

use crate::bytecode::Instr;
use crate::runtime::vm::ops::consts::{
    OP_HALT, OP_RETURN,
};
use crate::runtime::vm::ops::meta;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet;
#[cfg(feature = "std")]
use std::collections::BTreeSet;

/// Expand a slot mask into slot indices for the given instruction.
fn slot_indices(mask: u8, instr: Instr) -> Vec<u32> {
    let mut slots = Vec::new();
    if mask & 1 != 0 {
        slots.push(instr.a());
    }
    if mask & 2 != 0 {
        slots.push(instr.b() as u32);
    }
    if mask & 4 != 0 {
        slots.push(instr.c() as u32);
    }
    slots
}

/// A basic block in the bytecode control flow graph.
#[derive(Debug, Clone)]
pub(crate) struct BasicBlock {
    /// Starting instruction index (leader).
    pub(crate) start: usize,
    /// Ending instruction index (inclusive).
    pub(crate) end: usize,
    /// Successors in the CFG (indices of successor block leaders).
    pub(crate) successors: Vec<usize>,
    /// Set of local slots defined (STORE_LOCAL) in this block.
    defs: Vec<u32>,
    /// Set of local slots used (LOAD_LOCAL) in this block.
    uses: Vec<u32>,
}

/// Build basic blocks from a linear instruction stream.
///
/// Leaders are:
/// - The first instruction (index 0)
/// - Target of any jump instruction
/// - Instruction immediately following a jump
pub(crate) fn build_basic_blocks(code: &[Instr], wide: &[i64]) -> Vec<BasicBlock> {
    let len = code.len();
    if len == 0 {
        return Vec::new();
    }

    let mut is_leader = vec![false; len];
    is_leader[0] = true;

    // First pass: mark all jump targets and fall-through targets
    for (i, instr) in code.iter().enumerate() {
        let kind = meta::branch_kind(instr.op());
        match kind {
            1 => {
                // Absolute branch: target in operand a
                let target = instr.a() as usize;
                if target < len {
                    is_leader[target] = true;
                }
                // Fall-through
                if i + 1 < len {
                    is_leader[i + 1] = true;
                }
            }
            2 => {
                // Wide branch: exit target in wide operand c
                // We can't resolve it here without wide_operands, so mark fall-through
                if i + 1 < len {
                    is_leader[i + 1] = true;
                }
            }
            _ => {}
        }
    }

    // Group instructions into blocks
    let mut blocks: Vec<BasicBlock> = Vec::new();
    let mut i = 0;
    while i < len {
        if is_leader[i] {
            let start = i;
            // Block extends until the next leader or end
            let mut end = start;
            while end + 1 < len && !is_leader[end + 1] {
                end += 1;
            }
            blocks.push(BasicBlock {
                start,
                end,
                successors: Vec::new(),
                defs: Vec::new(),
                uses: Vec::new(),
            });
            i = end + 1;
        } else {
            i += 1;
        }
    }

    // Build successor edges and collect defs/uses
    for block in &mut blocks {
        for (idx, instr) in code[block.start..=block.end].iter().enumerate() {
            let idx = block.start + idx;
            let op = instr.op();
            // Use metadata for slot reads/writes
            for s in slot_indices(meta::reads(op), *instr) {
                block.uses.push(s);
            }
            for s in slot_indices(meta::writes(op), *instr) {
                block.defs.push(s);
            }
            // OP_FOR_NUMBER_LOOP is a packed-operand opt-out — special case
            if op == crate::bytecode::OP_FOR_NUMBER_LOOP {
                let var_slot = instr.a() >> 16;
                block.uses.push(var_slot);
                block.defs.push(var_slot);
            }
            let kind = meta::branch_kind(op);
            match kind {
                1 => {
                    // Absolute branch: target in operand a
                    let target = instr.a() as usize;
                    if target < len && !block.successors.contains(&target) {
                        block.successors.push(target);
                    }
                    // Fall-through successor
                    if idx + 1 < len && idx == block.end {
                        let ft = idx + 1;
                        if !block.successors.contains(&ft) {
                            block.successors.push(ft);
                        }
                    }
                }
                2 => {
                    // Wide branch: exit target in wide operand c
                    // Fall-through to loop body (next instruction)
                    if idx + 1 < len && idx == block.end {
                        let ft = idx + 1;
                        if !block.successors.contains(&ft) {
                            block.successors.push(ft);
                        }
                    }
                    // Exit target is in wide operand c; resolve it now
                    let exit_wide_idx = instr.c() as usize;
                    if let Some(&exit_ip) = wide.get(exit_wide_idx) {
                        let exit_ip = exit_ip as usize;
                        if exit_ip < len && idx == block.end {
                            if !block.successors.contains(&exit_ip) {
                                block.successors.push(exit_ip);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Add fall-through edges: any block whose last instruction is not an unconditional
    // terminator (JUMP, HALT, RETURN) should fall through to the next block (end + 1).
    for block in &mut blocks {
        let last_instr = code[block.end];
        let op = last_instr.op();
        if op == OP_HALT || op == OP_RETURN {
            // Unconditional terminators have no fall-through
        } else if meta::branch_kind(op) == 1 {
            // Absolute branch: only unconditional JUMP has no fall-through
            // Conditional branches (JUMP_IF_FALSE, JUMP_IF_NIL, FOR_ITER_NEXT, FOR_RANGE_NEXT) fall through
            if op != crate::runtime::vm::ops::consts::OP_JUMP {
                let next = block.end + 1;
                if next < len && !block.successors.contains(&next) {
                    block.successors.push(next);
                }
            }
        } else {
            let next = block.end + 1;
            if next < len && !block.successors.contains(&next) {
                block.successors.push(next);
            }
        }
    }

    blocks
}

/// Compute live variables using backward data flow.
///
/// Returns a Vec where `live_vars[i]` is the set of local slots that are
/// live (used before being redefined) at instruction index `i`.
pub(crate) fn live_variables(code: &[Instr], blocks: &[BasicBlock]) -> Vec<BTreeSet<u32>> {
    let len = code.len();
    if len == 0 {
        return Vec::new();
    }

    let num_blocks = blocks.len();

    // Use[k] = variables used in block k
    // Def[k] = variables defined in block k
    let mut use_set: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); num_blocks];
    let mut def_set: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); num_blocks];

    for (bi, block) in blocks.iter().enumerate() {
        for instr in &code[block.start..=block.end] {
            let op = instr.op();
            // Use metadata for slot reads/writes
            for s in slot_indices(meta::reads(op), *instr) {
                use_set[bi].insert(s);
            }
            for s in slot_indices(meta::writes(op), *instr) {
                def_set[bi].insert(s);
            }
            // OP_FOR_NUMBER_LOOP is a packed-operand opt-out — special case
            // (see docs/tasks/opcode-slot-meta.md)
            if op == crate::bytecode::OP_FOR_NUMBER_LOOP {
                let var_slot = instr.a() >> 16;
                use_set[bi].insert(var_slot);
                def_set[bi].insert(var_slot);
            }
        }
    }

    // Backward data flow: live[b] = use[b] union (live[succ] - def[b]) for each successor
    let mut live_out: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); num_blocks];
    let mut live_in: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); num_blocks];

    let mut worklist: Vec<usize> = (0..num_blocks).collect();

    while let Some(bi) = worklist.pop() {
        // live_out[b] = union of live_in[s] for all successors s
        let mut new_out: BTreeSet<u32> = BTreeSet::new();
        for succ_idx in &blocks[bi].successors {
            // Find block containing succ_idx
            for (si, succ_block) in blocks.iter().enumerate() {
                if succ_block.start == *succ_idx {
                    for var in &live_in[si] {
                        new_out.insert(*var);
                    }
                }
            }
        }

        // live_in[b] = use[b] union (live_out[b] - def[b])
        let mut new_in: BTreeSet<u32> = use_set[bi].clone();
        for var in &new_out {
            if !def_set[bi].contains(var) {
                new_in.insert(*var);
            }
        }

        if new_out == live_out[bi] && new_in == live_in[bi] {
            continue;
        }

        live_out[bi] = new_out;
        live_in[bi] = new_in;

        // Add predecessors to worklist
        for (pi, pred) in blocks.iter().enumerate() {
            if pred.successors.contains(&blocks[bi].start) {
                if !worklist.contains(&pi) {
                    worklist.push(pi);
                }
            }
        }
    }

    // Build per-instruction live vars.
    // For each block, scan backward: live[idx] = instr_use[idx] union (live[idx+1] - instr_def[idx])
    // The live-out of the last instruction in a block = union of live_in of successor blocks.
    let mut result: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); len];

    for block in blocks.iter() {
        // live_out of this block = union of live_in of all successor blocks
        let mut block_live_out: BTreeSet<u32> = BTreeSet::new();
        for succ_idx in &block.successors {
            for (si, succ_block) in blocks.iter().enumerate() {
                if succ_block.start == *succ_idx {
                    for var in &live_in[si] {
                        block_live_out.insert(*var);
                    }
                }
            }
        }

        // Scan backward within the block, computing per-instruction use/def
        let mut live_after: BTreeSet<u32> = block_live_out;
        for idx in (block.start..=block.end).rev() {
            let instr = &code[idx];
            let op = instr.op();

            // Compute per-instruction use and def
            // Compute per-instruction use and def from metadata
            let mut instr_use: BTreeSet<u32> = BTreeSet::new();
            let mut instr_def: BTreeSet<u32> = BTreeSet::new();
            for s in slot_indices(meta::reads(op), *instr) {
                instr_use.insert(s);
            }
            for s in slot_indices(meta::writes(op), *instr) {
                instr_def.insert(s);
            }
            // OP_FOR_NUMBER_LOOP is a packed-operand opt-out — special case
            if op == crate::bytecode::OP_FOR_NUMBER_LOOP {
                let var_slot = instr.a() >> 16;
                instr_use.insert(var_slot);
                instr_def.insert(var_slot);
            }

            // live at idx = instr_use union (live_after - instr_def)
            let mut live_at: BTreeSet<u32> = instr_use;
            for var in &live_after {
                if !instr_def.contains(var) {
                    live_at.insert(*var);
                }
            }
            result[idx] = live_at;
            live_after = result[idx].clone();
        }
    }

    result
}
