//! Loop detection and loop-invariant code motion.
//!
//! Identifies natural loops via back edges and hoists invariant instructions
//! (like LOAD_GLOBAL) before loop entry.

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{boxed::Box, format, vec::Vec};

use crate::bytecode::Instr;
use crate::bytecode::peephole::{collect_jump_targets, drain_and_patch};
use crate::runtime::vm::ops::consts::{
    OP_CALL_HOT, OP_LOAD_GLOBAL, OP_LOAD_LOCAL, OP_NOP, OP_PUSH_FALSE, OP_PUSH_FLOAT, OP_PUSH_INT,
    OP_PUSH_NIL, OP_PUSH_STR, OP_PUSH_TRUE, OP_STORE_LOCAL,
};
use crate::runtime::vm::ops::meta;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet;
#[cfg(feature = "std")]
use std::collections::BTreeSet;

use super::dataflow::{BasicBlock, build_basic_blocks, slot_indices};

/// Upper bound on hoists per compilation unit.
///
/// Each hoist changes instruction indices, so the loop structure is recomputed
/// afterwards. The bound keeps a loop-heavy program from turning the pass
/// quadratic.
const MAX_HOISTS: usize = 8;

/// Is this instruction a single-push, invariant word?
///
/// These are the only argument producers the hoister accepts: a literal, or a
/// read of a local the loop never writes.
fn invariant_push(instr: Instr, written: &BTreeSet<u32>) -> bool {
    match instr.op() {
        OP_LOAD_LOCAL => !written.contains(&instr.a()),
        OP_PUSH_NIL | OP_PUSH_TRUE | OP_PUSH_FALSE | OP_PUSH_INT | OP_PUSH_FLOAT | OP_PUSH_STR => {
            true
        }
        _ => false,
    }
}

/// Returns a list of loops, where each loop is a list of instruction indices in the loop body.
pub(super) fn detect_loops(code: &[Instr], blocks: &[BasicBlock]) -> Vec<Vec<usize>> {
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

    // Compute dominators using iterative algorithm.
    //
    // Non-entry blocks start at the full block set and shrink to the fixpoint:
    // each round intersects with `{b} ∪ ⋃ dom[p]`, so starting from the empty
    // set would pin every block at `{}` and hide every loop.
    let universe: BTreeSet<usize> = (0..num_blocks).collect();
    let mut dom: Vec<BTreeSet<usize>> = vec![universe; num_blocks];
    dom[0].clear();
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
/// - LOAD_GLOBAL inside loops → hoist before loop entry
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
        .map(|i| blocks.iter().position(|b| i >= b.start && i <= b.end))
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
        // If the header is at index 0, there's no pre-header — skip.
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
        let kind = meta::branch_kind(instr.op());
        match kind {
            1 => {
                // Absolute branch: target in operand a
                if instr.a() >= pos_u32 {
                    let op = instr.op();
                    *instr = Instr::make(op, instr.a() + count_u32, instr.b(), instr.c());
                }
            }
            2 => {
                // Wide branch: exit target in wide operand c
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

/// Hoists pure native calls out of loops.
///
/// A hot call whose slot the VM declared `PURE | DETERMINISTIC` and whose
/// arguments are loop-invariant produces the same value on every iteration.
/// Moving it to the pre-header turns N calls into one; the body reads the
/// saved result from a fresh local.
///
/// The pass is deliberately narrow:
/// * only `OP_CALL_HOT` qualifies, so nothing that can run script code is
///   reordered;
/// * the argument words must be the `n` instructions immediately before the
///   call, each a literal or a read of a local the loop never writes;
/// * no branch may enter the argument window, and none may enter the loop
///   header from outside, which keeps the jump patching valid.
pub fn hoist_pure_calls(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    locals: &mut Vec<Box<str>>,
    is_pure: impl Fn(u16) -> bool,
) {
    // Most programs have no hot calls at all; skip the loop analysis entirely.
    if !code.iter().any(|i| i.op() == OP_CALL_HOT && is_pure(i.b())) {
        return;
    }
    for _ in 0..MAX_HOISTS {
        if !hoist_first(code, wide, locals, &is_pure) {
            break;
        }
    }
}

/// Performs one hoist, or reports that nothing qualified.
fn hoist_first(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    locals: &mut Vec<Box<str>>,
    is_pure: &impl Fn(u16) -> bool,
) -> bool {
    let blocks = build_basic_blocks(code, wide);
    let loops = detect_loops(code, &blocks);
    let targets = collect_jump_targets(code, wide);

    for loop_instrs in &loops {
        let in_loop: BTreeSet<usize> = loop_instrs.iter().copied().collect();
        let Some(&header) = loop_instrs.iter().min() else {
            continue;
        };
        if header == 0 {
            continue;
        }

        // Locals written anywhere in the loop. `meta::writes` is declared per
        // opcode by the opcode macro, so fused stores are covered too.
        let mut written: BTreeSet<u32> = BTreeSet::new();
        for &idx in &in_loop {
            let instr = code[idx];
            for slot in slot_indices(meta::writes(instr.op()), instr) {
                written.insert(slot);
            }
        }

        // A branch from outside into the header would skip the hoisted value.
        // The compiler emits none today; bail rather than model it.
        if branches_into(code, wide, header, |ip| !in_loop.contains(&ip)) {
            continue;
        }

        let Some((call_idx, n)) = find_hoistable(code, &in_loop, &written, &targets, is_pure)
        else {
            continue;
        };

        // `[arguments..., call, store]` evaluated once before the loop.
        let mut hoisted: Vec<Instr> = code[call_idx - n..call_idx].to_vec();
        hoisted.push(code[call_idx]);
        let slot = locals.len() as u32;
        locals.push(format!("__hoist_{slot}").into());
        hoisted.push(Instr::a1(OP_STORE_LOCAL, slot));
        let count = hoisted.len();

        code.splice(header..header, hoisted);
        patch_jump_targets_insert(code, wide, header, count);

        // The call now sits after the inserted words; the arguments are right
        // before it and no longer needed.
        let new_call = call_idx + count;
        code[new_call] = Instr::a1(OP_LOAD_LOCAL, slot);
        if n > 0 {
            drain_and_patch(code, wide, (new_call - n)..=(new_call - 1));
        }
        return true;
    }
    false
}

/// First pure hot call in the loop whose argument words are all invariant.
fn find_hoistable(
    code: &[Instr],
    in_loop: &BTreeSet<usize>,
    written: &BTreeSet<u32>,
    targets: &crate::HashSet<usize>,
    is_pure: &impl Fn(u16) -> bool,
) -> Option<(usize, usize)> {
    for &idx in in_loop {
        let instr = code[idx];
        if instr.op() != OP_CALL_HOT || !is_pure(instr.b()) {
            continue;
        }
        let n = instr.a() as usize;
        if idx < n {
            continue;
        }
        let start = idx - n;
        // A branch into the window would enter the argument sequence midway; a
        // block boundary is fine, a jump target is not.
        let window_ok =
            (start..idx).all(|j| !targets.contains(&j) && invariant_push(code[j], written));
        if window_ok {
            return Some((idx, n));
        }
    }
    None
}

/// Does any instruction accepted by `filter` branch to `target`?
fn branches_into(
    code: &[Instr],
    wide: &[i64],
    target: usize,
    filter: impl Fn(usize) -> bool,
) -> bool {
    code.iter().enumerate().any(|(ip, instr)| {
        if !filter(ip) {
            return false;
        }
        match meta::branch_kind(instr.op()) {
            1 => instr.a() as usize == target,
            2 => wide.get(instr.c() as usize).copied().unwrap_or(-1) == target as i64,
            _ => false,
        }
    })
}
