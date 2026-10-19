//! Data flow analysis: basic block construction, reaching definitions, and live variable analysis.
//!
//! Provides the foundation for dead store elimination and constant propagation.

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::bytecode::{
    Instr, OP_ADD_LOCAL_LOCAL, OP_ADD_LOCAL_LOCAL_STORE, OP_ADD_LOCAL_STORE, OP_ADD_INT_LOCAL,
    OP_ADD_FLOAT_LOCAL, OP_DEC_LOCAL, OP_DIV_LOCAL_LOCAL, OP_DIV_LOCAL_STORE, OP_DIV_FLOAT_LOCAL,
    OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_HALT, OP_INC_LOCAL,
    OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_CONST_LOCAL,
    OP_LOAD_LOCAL, OP_LOAD_LOCAL_PUSH_INT, OP_LOAD_LOCAL_STORE_LOCAL, OP_LT_LOCAL_LOCAL,
    OP_MOD_EQ_INT, OP_MUL_LOCAL_LOCAL, OP_MUL_LOCAL_STORE, OP_MUL_INT_LOCAL, OP_MUL_FLOAT_LOCAL,
    OP_STORE_LOCAL, OP_SUB_LOCAL_LOCAL, OP_SUB_LOCAL_STORE, OP_SUB_INT_LOCAL, OP_SUB_FLOAT_LOCAL,
    OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL,
};

#[cfg(not(feature = "std"))]
use alloc::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "std")]
use std::collections::BTreeSet;

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
pub(crate) fn build_basic_blocks(code: &[Instr]) -> Vec<BasicBlock> {
    let len = code.len();
    if len == 0 {
        return Vec::new();
    }

    let mut is_leader = vec![false; len];
    is_leader[0] = true;

    // First pass: mark all jump targets and fall-through targets
    for (i, instr) in code.iter().enumerate() {
        match instr.op() {
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL => {
                let target = instr.a() as usize;
                if target < len {
                    is_leader[target] = true;
                }
                // Fall-through
                if i + 1 < len {
                    is_leader[i + 1] = true;
                }
            }
            OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                let target = instr.a() as usize;
                if target < len {
                    is_leader[target] = true;
                }
                if i + 1 < len {
                    is_leader[i + 1] = true;
                }
            }
            OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
                // The wide operand holds the exit target
                // We can't resolve it here without wide_operands, so mark fall-through
                if i + 1 < len {
                    is_leader[i + 1] = true;
                }
            }
            OP_HALT => {
                // No successors
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
        for idx in block.start..=block.end {
            let instr = code[idx];
            match instr.op() {
                OP_STORE_LOCAL => {
                    block.defs.push(instr.a());
                }
                OP_LOAD_LOCAL => {
                    block.uses.push(instr.a());
                }
                OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL => {
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
                OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                    let target = instr.a() as usize;
                    if target < len && !block.successors.contains(&target) {
                        block.successors.push(target);
                    }
                    if idx + 1 < len && idx == block.end {
                        let ft = idx + 1;
                        if !block.successors.contains(&ft) {
                            block.successors.push(ft);
                        }
                    }
                }
                OP_HALT => {
                    // No successors
                }
                _ => {}
            }
        }
    }

    blocks
}

/// Compute reaching definitions using worklist algorithm.
///
/// Returns a Vec where `reaching_defs[i]` is the set of (slot, def_index) pairs
/// that reach instruction index `i`.
/// Used by constant propagation (Step 3).
#[allow(dead_code)]
fn reaching_definitions(
    code: &[Instr],
    blocks: &[BasicBlock],
) -> Vec<Vec<(u32, usize)>> {
    let len = code.len();
    if len == 0 {
        return Vec::new();
    }

    // Map instruction index to block index
    let instr_to_block: Vec<usize> = (0..len)
        .map(|i| {
            blocks
                .iter()
                .position(|b| i >= b.start && i <= b.end)
                .unwrap_or(0)
        })
        .collect();

    let num_blocks = blocks.len();

    // gen[k] = definitions generated in block k
    // kill[k] = definitions killed (redefined) in block k
    let mut gen_set: Vec<Vec<(u32, usize)>> = vec![Vec::new(); num_blocks];
    let mut kill: Vec<Vec<u32>> = vec![Vec::new(); num_blocks];

    for (bi, block) in blocks.iter().enumerate() {
        for idx in block.start..=block.end {
            let instr = code[idx];
            if instr.op() == OP_STORE_LOCAL {
                let slot = instr.a();
                // Kill previous defs of this slot
                if !kill[bi].contains(&slot) {
                    kill[bi].push(slot);
                }
                // Generate new def
                gen_set[bi].push((slot, idx));
            }
        }
    }

    // Initialize in[k] and out[k]
    let mut in_defs: Vec<Vec<(u32, usize)>> = vec![Vec::new(); num_blocks];
    let mut out_defs: Vec<Vec<(u32, usize)>> = vec![Vec::new(); num_blocks];

    // Worklist: start with all blocks
    let mut worklist: Vec<usize> = (0..num_blocks).collect();

    while let Some(bi) = worklist.pop() {
        // in[b] = union of out[s] for all predecessors s
        // Compute in[b] from successors of predecessor blocks
        let mut new_in: Vec<(u32, usize)> = Vec::new();

        // Find predecessors: blocks whose successors include this block's start
        for (pi, pred) in blocks.iter().enumerate() {
            if pred.successors.contains(&blocks[bi].start) {
                for def in &out_defs[pi] {
                    if !new_in.contains(def) {
                        new_in.push(*def);
                    }
                }
            }
        }

        // out[b] = gen[b] union (in[b] - kill[b])
        let mut new_out: Vec<(u32, usize)> = Vec::new();
        // Add surviving defs from in
        let kill_set: BTreeSet<u32> = kill[bi].iter().copied().collect();
        for def in &new_in {
            if !kill_set.contains(&def.0) {
                if !new_out.contains(def) {
                    new_out.push(*def);
                }
            }
        }
        // Add generated defs
        for def in &gen_set[bi] {
            if !new_out.contains(def) {
                new_out.push(*def);
            }
        }

        // Check for convergence
        if new_in == in_defs[bi] && new_out == out_defs[bi] {
            continue;
        }

        in_defs[bi] = new_in;
        out_defs[bi] = new_out;

        // Add successors to worklist
        for succ in &blocks[bi].successors {
            if let Some(sbi) = instr_to_block.get(*succ).copied() {
                if !worklist.contains(&sbi) {
                    worklist.push(sbi);
                }
            }
        }
    }

    // Build per-instruction reaching defs
    let mut result: Vec<Vec<(u32, usize)>> = vec![Vec::new(); len];
    for (bi, block) in blocks.iter().enumerate() {
        // First instruction gets in_defs[b]
        if block.start < len {
            result[block.start].clone_from(&in_defs[bi]);
        }
        // Subsequent instructions get in_defs plus defs from earlier in block
        for idx in (block.start + 1)..=block.end {
            if idx < len {
                result[idx].clone_from(&in_defs[bi]);
                for i in block.start..idx {
                    let instr = code[i];
                    if instr.op() == OP_STORE_LOCAL {
                        let slot = instr.a();
                        // Remove killed defs of this slot
                        result[idx].retain(|d| d.0 != slot);
                        result[idx].push((slot, i));
                    }
                }
            }
        }
    }

    result
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
        for idx in block.start..=block.end {
            let instr = code[idx];
            let op = instr.op();
            match op {
                // Simple load/store
                OP_LOAD_LOCAL => {
                    use_set[bi].insert(instr.a());
                }
                OP_STORE_LOCAL => {
                    def_set[bi].insert(instr.a());
                }
                // Fused load+store: LOAD_CONST_LOCAL reads wide, writes slot a
                OP_LOAD_CONST_LOCAL => {
                    def_set[bi].insert(instr.a());
                }
                // LOAD_LOCAL_STORE_LOCAL: reads slot b, writes slot a
                OP_LOAD_LOCAL_STORE_LOCAL => {
                    use_set[bi].insert(instr.b() as u32);
                    def_set[bi].insert(instr.a());
                }
                // LOAD_LOCAL_PUSH_INT: reads slot a
                OP_LOAD_LOCAL_PUSH_INT => {
                    use_set[bi].insert(instr.a());
                }
                // Binary fused ops: ADD_LOCAL_LOCAL, SUB_LOCAL_LOCAL, etc. â€” reads a and b
                OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL
                | OP_DIV_LOCAL_LOCAL | OP_LT_LOCAL_LOCAL => {
                    use_set[bi].insert(instr.a());
                    use_set[bi].insert(instr.b() as u32);
                }
                // Store-back fused ops: ADD_LOCAL_STORE, SUB_LOCAL_STORE, etc.
                // Reads slots a and b, writes slot a
                OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE
                | OP_DIV_LOCAL_STORE => {
                    use_set[bi].insert(instr.a());
                    use_set[bi].insert(instr.b() as u32);
                    def_set[bi].insert(instr.a());
                }
                // Three-operand fused: ADD_LOCAL_LOCAL_STORE â€” reads a and b, writes c
                OP_ADD_LOCAL_LOCAL_STORE => {
                    use_set[bi].insert(instr.a());
                    use_set[bi].insert(instr.b() as u32);
                    def_set[bi].insert(instr.c() as u32);
                }
                // Constant-operand fused ops: ADD_INT_LOCAL, SUB_INT_LOCAL, etc.
                // Reads slot a, writes slot a
                OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL
                | OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL | OP_DIV_FLOAT_LOCAL =>
                {
                    use_set[bi].insert(instr.a());
                    def_set[bi].insert(instr.a());
                }
                // MOD_EQ_INT: reads slot a, pushes result (no local write)
                OP_MOD_EQ_INT => {
                    use_set[bi].insert(instr.a());
                }
                // INC/DEC: reads and writes slot a
                OP_INC_LOCAL | OP_DEC_LOCAL => {
                    use_set[bi].insert(instr.a());
                    def_set[bi].insert(instr.a());
                }
                _ => {}
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

    // Build per-instruction live vars
    let mut result: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); len];
    for (bi, block) in blocks.iter().enumerate() {
        for idx in block.start..=block.end {
            if idx < len {
                // Live at this point = live_in plus uses from later in block
                // Simplified: use live_in for the whole block
                for var in &live_in[bi] {
                    result[idx].insert(*var);
                }
            }
        }
    }

    result
}