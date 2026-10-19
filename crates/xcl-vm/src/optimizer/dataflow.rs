//! Data flow analysis: basic block construction, reaching definitions, and live variable analysis.
//!
//! Provides the foundation for dead store elimination and constant propagation.

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::bytecode::{
    Instr, OP_ADD_INT_STORE_LOCAL, OP_ADD_LOCAL_LOCAL, OP_ADD_LOCAL_LOCAL_STORE,
    OP_ADD_LOCAL_STORE, OP_ADD_INT_LOCAL, OP_ADD_FLOAT_LOCAL, OP_ARRAY_PUSH_LOCAL, OP_DEC_LOCAL,
    OP_DIV_LOCAL_LOCAL, OP_DIV_LOCAL_LOCAL_STORE, OP_DIV_LOCAL_STORE, OP_DIV_FLOAT_LOCAL,
    OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_GET_FIELD_LOCAL_STORE,
    OP_GET_INDEX_LOCAL_LOCAL_STORE, OP_INC_LOCAL, OP_INC_LOCAL_INT, OP_LOAD_CONST_LOCAL,
    OP_LOAD_LOCAL_LOAD_LOCAL, OP_LOAD_LOCAL_PUSH_INT, OP_LOAD_LOCAL_STORE_LOCAL, OP_LT_LOCAL_LOCAL,
    OP_MOD_EQ_INT, OP_MOD_LOCAL_LOCAL_STORE, OP_MUL_LOCAL_LOCAL, OP_MUL_LOCAL_LOCAL_STORE,
    OP_MUL_LOCAL_STORE, OP_MUL_INT_LOCAL, OP_MUL_FLOAT_LOCAL, OP_SUB_LOCAL_LOCAL,
    OP_SUB_LOCAL_LOCAL_STORE, OP_SUB_LOCAL_STORE, OP_SUB_INT_LOCAL, OP_SUB_FLOAT_LOCAL,
    OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::consts::{
    OP_HALT, OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_LOCAL, OP_RETURN, OP_STORE_LOCAL,
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
pub(crate) fn build_basic_blocks(code: &[Instr], wide: &[i64]) -> Vec<BasicBlock> {
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
        for (idx, instr) in code[block.start..=block.end].iter().enumerate() {
            let idx = block.start + idx;
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
                OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
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
                OP_HALT => {
                    // No successors
                }
                _ => {}
            }
        }
    }

    // Add fall-through edges: any block whose last instruction is not an unconditional
    // terminator (JUMP, HALT, RETURN) should fall through to the next block (end + 1).
    for block in &mut blocks {
        let last_instr = code[block.end];
        match last_instr.op() {
            OP_JUMP | OP_HALT | OP_RETURN => {
                // Unconditional terminators have no fall-through
            }
            _ => {
                let next = block.end + 1;
                if next < len && !block.successors.contains(&next) {
                    block.successors.push(next);
                }
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
                OP_ADD_LOCAL_LOCAL_STORE
                | OP_MUL_LOCAL_LOCAL_STORE
                | OP_SUB_LOCAL_LOCAL_STORE
                | OP_DIV_LOCAL_LOCAL_STORE
                | OP_MOD_LOCAL_LOCAL_STORE => {
                    use_set[bi].insert(instr.b() as u32);
                    use_set[bi].insert(instr.c() as u32);
                    def_set[bi].insert(instr.a());
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
                // INC_LOCAL_INT: reads and writes slot a (operand a = slot, b = const)
                OP_INC_LOCAL_INT => {
                    use_set[bi].insert(instr.a());
                    def_set[bi].insert(instr.a());
                }
                // ADD_INT_STORE_LOCAL: reads slot b, writes slot a (operand a = dest, b = src, c = const)
                OP_ADD_INT_STORE_LOCAL => {
                    use_set[bi].insert(instr.b() as u32);
                    def_set[bi].insert(instr.a());
                }
                // LOAD_LOCAL_LOAD_LOCAL: reads slot a and slot b
                OP_LOAD_LOCAL_LOAD_LOCAL => {
                    use_set[bi].insert(instr.a());
                    use_set[bi].insert(instr.b() as u32);
                }
                // WHILE_LT_INT: reads slot a (operand a = slot, b = limit, c = exit_wide_idx)
                OP_WHILE_LT_INT => {
                    use_set[bi].insert(instr.a());
                }
                // WHILE_LT_LOCAL: reads slot a and slot b (operand a = left, b = right, c = exit_wide_idx)
                OP_WHILE_LT_LOCAL => {
                    use_set[bi].insert(instr.a());
                    use_set[bi].insert(instr.b() as u32);
                }
                // GET_INDEX_LOCAL_LOCAL_STORE: reads slot b (index) and slot c (array), writes slot a
                OP_GET_INDEX_LOCAL_LOCAL_STORE => {
                    use_set[bi].insert(instr.b() as u32);
                    use_set[bi].insert(instr.c() as u32);
                    def_set[bi].insert(instr.a());
                }
                // GET_FIELD_LOCAL_STORE: reads slot c (object), writes slot a
                OP_GET_FIELD_LOCAL_STORE => {
                    use_set[bi].insert(instr.c() as u32);
                    def_set[bi].insert(instr.a());
                }
                // ARRAY_PUSH_LOCAL: reads slot a (array) and slot b (value), writes slot a
                OP_ARRAY_PUSH_LOCAL => {
                    use_set[bi].insert(instr.a());
                    use_set[bi].insert(instr.b() as u32);
                    def_set[bi].insert(instr.a());
                }
                // FOR_NUMBER_LOOP: reads and writes the loop-var slot packed in a (var_slot = a >> 16)
                OP_FOR_NUMBER_LOOP => {
                    let var_slot = instr.a() >> 16;
                    use_set[bi].insert(var_slot);
                    def_set[bi].insert(var_slot);
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
            let mut instr_use: BTreeSet<u32> = BTreeSet::new();
            let mut instr_def: BTreeSet<u32> = BTreeSet::new();
            match op {
                OP_LOAD_LOCAL => { instr_use.insert(instr.a()); }
                OP_STORE_LOCAL => { instr_def.insert(instr.a()); }
                OP_LOAD_CONST_LOCAL => { instr_def.insert(instr.a()); }
                OP_LOAD_LOCAL_STORE_LOCAL => {
                    instr_use.insert(instr.b() as u32);
                    instr_def.insert(instr.a());
                }
                OP_LOAD_LOCAL_PUSH_INT => { instr_use.insert(instr.a()); }
                OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL
                | OP_DIV_LOCAL_LOCAL | OP_LT_LOCAL_LOCAL => {
                    instr_use.insert(instr.a());
                    instr_use.insert(instr.b() as u32);
                }
                OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE
                | OP_DIV_LOCAL_STORE => {
                    instr_use.insert(instr.a());
                    instr_use.insert(instr.b() as u32);
                    instr_def.insert(instr.a());
                }
                OP_ADD_LOCAL_LOCAL_STORE | OP_MUL_LOCAL_LOCAL_STORE
                | OP_SUB_LOCAL_LOCAL_STORE | OP_DIV_LOCAL_LOCAL_STORE
                | OP_MOD_LOCAL_LOCAL_STORE => {
                    instr_use.insert(instr.b() as u32);
                    instr_use.insert(instr.c() as u32);
                    instr_def.insert(instr.a());
                }
                OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL
                | OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL | OP_DIV_FLOAT_LOCAL => {
                    instr_use.insert(instr.a());
                    instr_def.insert(instr.a());
                }
                OP_MOD_EQ_INT => { instr_use.insert(instr.a()); }
                OP_INC_LOCAL | OP_DEC_LOCAL => {
                    instr_use.insert(instr.a());
                    instr_def.insert(instr.a());
                }
                OP_INC_LOCAL_INT => {
                    instr_use.insert(instr.a());
                    instr_def.insert(instr.a());
                }
                OP_ADD_INT_STORE_LOCAL => {
                    instr_use.insert(instr.b() as u32);
                    instr_def.insert(instr.a());
                }
                OP_LOAD_LOCAL_LOAD_LOCAL => {
                    instr_use.insert(instr.a());
                    instr_use.insert(instr.b() as u32);
                }
                OP_WHILE_LT_INT => { instr_use.insert(instr.a()); }
                OP_WHILE_LT_LOCAL => {
                    instr_use.insert(instr.a());
                    instr_use.insert(instr.b() as u32);
                }
                OP_GET_INDEX_LOCAL_LOCAL_STORE => {
                    instr_use.insert(instr.b() as u32);
                    instr_use.insert(instr.c() as u32);
                    instr_def.insert(instr.a());
                }
                OP_GET_FIELD_LOCAL_STORE => {
                    instr_use.insert(instr.c() as u32);
                    instr_def.insert(instr.a());
                }
                OP_ARRAY_PUSH_LOCAL => {
                    instr_use.insert(instr.a());
                    instr_use.insert(instr.b() as u32);
                    instr_def.insert(instr.a());
                }
                OP_FOR_NUMBER_LOOP => {
                    let var_slot = instr.a() >> 16;
                    instr_use.insert(var_slot);
                    instr_def.insert(var_slot);
                }
                _ => {}
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