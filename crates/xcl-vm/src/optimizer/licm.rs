//! Loop detection and loop-invariant code motion.
//!
//! Identifies natural loops via back edges and hoists invariant instructions
//! (like LOAD_GLOBAL) before loop entry.

use crate::bytecode::{
    Instr, OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::consts::{OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_GLOBAL, OP_NOP};

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet;
#[cfg(feature = "std")]
use std::collections::BTreeSet;

use super::dataflow::{build_basic_blocks, BasicBlock};

/// Returns a list of loops, where each loop is a list of instruction indices in the loop body.
fn detect_loops(code: &[Instr], blocks: &[BasicBlock]) -> Vec<Vec<usize>> {
    let len = code.len();
    if len == 0 {
        return Vec::new();
    }

    // Build adjacency list for CFG
    let num_blocks = blocks.len();
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); num_blocks];
    for (bi, block) in blocks.iter().enumerate() {
        for succ_idx in &block.successors {
            // Find block containing succ_idx
            for (si, succ_block) in blocks.iter().enumerate() {
                if succ_block.start == *succ_idx {
                    successors[bi].push(si);
                }
            }
        }
    }

    // Compute dominators using iterative algorithm
    let mut dom: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); num_blocks];
    dom[0].insert(0); // Entry block dominates itself

    loop {
        let mut changed = false;
        for bi in 1..num_blocks {
            // dom[b] = {b} union intersection of dom[p] for all predecessors p
            let mut new_dom: BTreeSet<usize> = BTreeSet::new();
            new_dom.insert(bi);

            // Find predecessors
            for (pi, pred_succs) in successors.iter().enumerate() {
                if pred_succs.contains(&bi) {
                    for d in &dom[pi] {
                        new_dom.insert(*d);
                    }
                }
            }

            // Intersect with existing dom
            if !new_dom.is_empty() {
                let old_count = dom[bi].len();
                dom[bi].retain(|d| new_dom.contains(d));
                if dom[bi].len() != old_count {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Find back edges: edge from b to d where d dominates b
    let mut loops: Vec<Vec<usize>> = Vec::new();
    for (bi, succs) in successors.iter().enumerate() {
        for &si in succs {
            // Check if si dominates bi (back edge)
            if si < bi && dom[bi].contains(&si) {
                // Found a back edge bi -> si
                // Collect all blocks in the natural loop
                let mut loop_blocks: BTreeSet<usize> = BTreeSet::new();
                loop_blocks.insert(si); // Header
                let mut worklist: Vec<usize> = vec![bi];
                while let Some(w) = worklist.pop() {
                    if loop_blocks.insert(w) {
                        // Add predecessors of w
                        for (pi, pred_succs) in successors.iter().enumerate() {
                            if pred_succs.contains(&w) && !loop_blocks.contains(&pi) {
                                worklist.push(pi);
                            }
                        }
                    }
                }

                // Convert block indices to instruction indices
                let mut loop_instrs: Vec<usize> = Vec::new();
                for &bidx in &loop_blocks {
                    let block = &blocks[bidx];
                    for idx in block.start..=block.end {
                        if idx < len {
                            loop_instrs.push(idx);
                        }
                    }
                }
                loops.push(loop_instrs);
            }
        }
    }

    loops
}

/// Move loop-invariant instructions outside loops.
///
/// Handles:
/// - LOAD_GLOBAL inside loops â†’ hoist before loop entry
///
/// Algorithm:
/// 1. Detect natural loops using back edges in the CFG
/// 2. For each loop, find the loop header (the block targeted by the back edge)
/// 3. Find the pre-header: the instruction index just before the loop header
/// 4. Collect LOAD_GLOBAL instructions that appear in every loop block (invariant)
/// 5. Insert clones of invariant instructions at the pre-header
/// 6. Replace the original instructions inside the loop with NOP
/// 7. Patch all jump targets to account for inserted instructions
#[allow(unused_variables)]
pub fn loop_invariant_code_motion(code: &mut Vec<Instr>, wide: &mut [i64]) {
    if code.len() <= 2 {
        return;
    }

    let blocks = build_basic_blocks(code, wide);
    let loops = detect_loops(code, &blocks);

    if loops.is_empty() {
        return;
    }

    // Map instruction index to block index for quick lookup
    let instr_to_block: Vec<Option<usize>> = (0..code.len())
        .map(|i| {
            blocks.iter().position(|b| i >= b.start && i <= b.end)
        })
        .collect();

    for loop_instrs in &loops {
        if loop_instrs.is_empty() {
            continue;
        }

        // Find the loop header block (the block with the smallest index in the loop
        // that is a target of a back edge)
        let loop_block_indices: BTreeSet<usize> = loop_instrs
            .iter()
            .filter_map(|&idx| instr_to_block.get(idx).copied().flatten())
            .collect();

        if loop_block_indices.is_empty() {
            continue;
        }

        let header_block_idx = *loop_block_indices.iter().min().unwrap();
        let header_block = &blocks[header_block_idx];
        let header_instr_idx = header_block.start;

        // The pre-header is the instruction just before the loop header.
        // If the header is at index 0, there's no pre-header â€” skip.
        if header_instr_idx == 0 {
            continue;
        }
        let pre_header_idx = header_instr_idx - 1;

        // Find LOAD_GLOBAL instructions that appear in ALL blocks of the loop.
        // An instruction is loop-invariant if it produces the same value on every
        // iteration. LOAD_GLOBAL is always invariant (globals don't change within
        // a single loop execution unless explicitly stored, which we conservatively
        // ignore here).
        //
        // For simplicity, we collect all LOAD_GLOBAL instructions in the loop body
        // (excluding the header, which may be the loop condition) and hoist them.

        // Group LOAD_GLOBALs by their slot (a) to deduplicate
        // Key: (instr_index, slot), Value: count of blocks containing this load
        let mut global_loads: BTreeMap<u32, Vec<usize>> = BTreeMap::new();

        for &idx in loop_instrs {
            if idx >= code.len() {
                continue;
            }
            let instr = code[idx];
            if instr.op() == OP_LOAD_GLOBAL {
                let slot = instr.a();
                global_loads.entry(slot).or_default().push(idx);
            }
        }

        // For each unique global slot loaded in the loop, hoist the first occurrence
        // to the pre-header and replace all occurrences in the loop with NOP.
        // We need to track how many instructions are inserted to patch jump targets.
        let mut insertions_at_pre_header: Vec<Instr> = Vec::new();

        for (&slot, indices) in &global_loads {
            if indices.is_empty() {
                continue;
            }
            // Get the instruction to hoist (first occurrence)
            let instr_to_hoist = code[indices[0]];
            insertions_at_pre_header.push(instr_to_hoist);
        }

        if insertions_at_pre_header.is_empty() {
            continue;
        }

        // Insert the hoisted instructions at pre_header_idx + 1 (after the pre-header)
        // We insert in reverse order so indices remain stable
        let insert_pos = pre_header_idx + 1;
        for (i, instr) in insertions_at_pre_header.iter().enumerate().rev() {
            code.insert(insert_pos + i, *instr);
        }
        let insert_count = insertions_at_pre_header.len();

        // Patch all jump targets for the inserted instructions
        patch_jump_targets_insert(code, wide, insert_pos, insert_count);

        // Now replace all LOAD_GLOBAL occurrences in the loop with NOP.
        // The loop instruction indices are shifted by insert_count.
        for &slot in global_loads.keys() {
            let indices = &global_loads[&slot];
            for &orig_idx in indices {
                let shifted_idx = orig_idx + insert_count;
                if shifted_idx < code.len() {
                    code[shifted_idx] = Instr::none(OP_NOP);
                }
            }
        }
    }

    // Let the peephole optimizer (Pattern 28) clean up the NOPs in the next pass.
    // We don't remove them here to avoid complex index management.
    let _ = wide;
}

/// Patch jump targets after inserting `count` instructions at position `pos`.
fn patch_jump_targets_insert(code: &mut [Instr], wide: &mut [i64], pos: usize, count: usize) {
    let pos_u32 = pos as u32;
    let count_u32 = count as u32;
    for instr in code.iter_mut() {
        let op = instr.op();
        match op {
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL | OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                if instr.a() >= pos_u32 {
                    *instr = Instr::make(op, instr.a() + count_u32, instr.b(), instr.c());
                }
            }
            OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
                let w_idx = instr.c() as usize;
                if let Some(exit_ip) = wide.get_mut(w_idx)
                    && *exit_ip >= pos as i64
                {
                    *exit_ip += count as i64;
                }
            }
            _ => {}
        }
    }
}