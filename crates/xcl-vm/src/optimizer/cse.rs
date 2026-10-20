//! Common Subexpression Elimination (CSE): removes redundant computations.
//!
//! Detects repeated expressions within basic blocks and computes them once,
//! replacing duplicates with references to the first computation.
//!
//! Soundness rules:
//! - Only caches pure expressions whose destination is not an operand.
//! - Invalidates cache entries when any operand slot is redefined.
//! - Does not cache in-place store-back ops (ADD_LOCAL_STORE, etc.) or
//!   constant-in-place ops (ADD_INT_LOCAL, INC_LOCAL, etc.).


// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{
    vec::Vec,
};

use crate::bytecode::Instr;
use crate::runtime::vm::ops::consts::{
    OP_ADD, OP_ADD_FIELD_LOCAL_STORE, OP_ADD_FLOAT_LOCAL, OP_ADD_INT_LOCAL, OP_ADD_LOCAL_LOCAL,
    OP_ADD_LOCAL_LOCAL_STORE, OP_ADD_LOCAL_STORE, OP_DEC_LOCAL, OP_DIV, OP_DIV_FLOAT_LOCAL,
    OP_DIV_LOCAL_LOCAL, OP_DIV_LOCAL_LOCAL_STORE, OP_DIV_LOCAL_STORE, OP_HALT, OP_INC_LOCAL,
    OP_INC_LOCAL_INT, OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_LOCAL,
    OP_LOAD_LOCAL_STORE_LOCAL, OP_MUL, OP_MUL_FLOAT_LOCAL, OP_MUL_INT_LOCAL,
    OP_MUL_LOCAL_LOCAL, OP_MUL_LOCAL_LOCAL_STORE, OP_MUL_LOCAL_STORE, OP_RETURN, OP_STORE_LOCAL,
    OP_SUB, OP_SUB_FLOAT_LOCAL, OP_SUB_INT_LOCAL, OP_SUB_LOCAL_LOCAL, OP_SUB_LOCAL_LOCAL_STORE,
    OP_SUB_LOCAL_STORE, OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_WHILE_LT_INT,
    OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::meta;

use super::dataflow::{build_basic_blocks, BasicBlock};

use crate::HashMap;

/// Returns the set of local slots **defined** by an instruction.
///
/// Uses the slot metadata from `meta::writes()` as the single source of truth.
fn defined_slots(instr: Instr) -> Vec<u32> {
    let op = instr.op();
    // OP_FOR_NUMBER_LOOP is a packed-operand opt-out — special case
    // (see docs/tasks/opcode-slot-meta.md)
    if op == OP_FOR_NUMBER_LOOP {
        return vec![instr.a() >> 16];
    }
    let mut slots = Vec::new();
    let mask = meta::writes(op);
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

/// Normalize commutative operands: for ADD/MUL return (min, max), for SUB/DIV keep order.
fn normalize_key(op: u8, left: u32, right: u32) -> (u32, u32) {
    match op {
        OP_ADD | OP_MUL | OP_ADD_LOCAL_LOCAL | OP_ADD_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL_STORE => {
            if left < right {
                (left, right)
            } else {
                (right, left)
            }
        }
        _ => (left, right),
    }
}

/// Local CSE: eliminates common subexpressions within each basic block.
///
/// Strategy:
/// 1. For each basic block, scan for repeated binary expressions
/// 2. When a duplicate is found, replace it with a LOAD_LOCAL to the result slot
/// 3. Handle both stack-based (LOAD_LOCAL; LOAD_LOCAL; OP) and fused opcodes
pub(crate) fn eliminate_common_subexpressions(code: &mut Vec<Instr>, wide: &mut [i64]) {
    let blocks = build_basic_blocks(code, wide);
    if blocks.len() < 2 {
        return;
    }

    for block in &blocks {
        let body_size = block.end - block.start + 1;
        if body_size < 4 {
            continue;
        }

        cse_in_block(code, wide, block);
    }
}

/// Apply CSE within a single basic block.
fn cse_in_block(code: &mut Vec<Instr>, wide: &mut [i64], block: &BasicBlock) {
    let start = block.start;
    let mut end = block.end;
    if start >= end {
        return;
    }

    // Map from expression key (op, operand1, operand2) to (first_dest_slot)
    let mut expr_map: HashMap<(u8, u32, u32), u32> = HashMap::new();

    let mut i = start;
    while i <= end && i < code.len() {
        let instr = code[i];
        let op = instr.op();

        // --- 1. Check for stack-based pattern: LOAD_LOCAL a; LOAD_LOCAL b; OP; STORE_LOCAL dest ---
        if op == OP_LOAD_LOCAL && i + 3 <= end {
            let a = instr.a();
            let next = code[i + 1];
            let next2 = code[i + 2];
            let next3 = code[i + 3];

            if next.op() == OP_LOAD_LOCAL {
                let b = next.a();
                let bin_op = next2.op();
                if (bin_op == OP_ADD || bin_op == OP_SUB || bin_op == OP_MUL || bin_op == OP_DIV)
                    && next3.op() == OP_STORE_LOCAL
                {
                    let dest = next3.a();
                    let (lo, hi) = normalize_key(bin_op, a, b);
                    let key = (bin_op, lo, hi);

                    // Only cache if dest is not one of the operands
                    if dest != a && dest != b {
                        if let Some(&first_dest) = expr_map.get(&key) {
                            if first_dest != dest {
                                // Replace 4-instruction sequence with LOAD_LOCAL_STORE_LOCAL
                                code[i] = Instr::a2(OP_LOAD_LOCAL_STORE_LOCAL, first_dest, dest as u16);
                                let range = (i + 1)..=(i + 3);
                                code.drain(range);
                                patch_jumps_after(code, wide, i + 1, 3);
                                end -= 3;
                            }
                            // If dest == first_dest, leave alone (no self-copy)
                        } else {
                            expr_map.insert(key, dest);
                        }
                    }
                    // Invalidate cache entries whose operands intersect defined slots
                    invalidate_cache(&mut expr_map, &defined_slots(instr));
                    // Also invalidate for the remaining instructions in the pattern
                    if i < end && i + 1 < code.len() {
                        // After potential drain, code may have shifted; only invalidate if not drained
                        if code.get(i).map(|x| x.op() == OP_LOAD_LOCAL).unwrap_or(false) {
                            // Pattern was NOT replaced; invalidate for next instructions
                            // (LOAD_LOCAL doesn't define anything, so skip)
                        }
                    }
                    i += 1;
                    continue;
                }
            }
        }

        // --- 2. Three-operand fused: ADD_LOCAL_LOCAL_STORE (dest=a, src1=c, src2=b) ---
        if op == OP_ADD_LOCAL_LOCAL_STORE
            || op == OP_SUB_LOCAL_LOCAL_STORE
            || op == OP_MUL_LOCAL_LOCAL_STORE
            || op == OP_DIV_LOCAL_LOCAL_STORE
        {
            let dest = instr.a();
            // Semantics: locals[a] = locals[c] op locals[b]
            let left = instr.c() as u32;
            let right = instr.b() as u32;
            let (lo, hi) = normalize_key(op, left, right);
            let key = (op, lo, hi);

            // Only cache if dest is not one of the operands
            if dest != left && dest != right {
                if let Some(&first_dest) = expr_map.get(&key) {
                    if first_dest != dest {
                        code[i] = Instr::a2(OP_LOAD_LOCAL_STORE_LOCAL, first_dest, dest as u16);
                    }
                } else {
                    expr_map.insert(key, dest);
                }
            }

            // Invalidate cache entries whose operands intersect defined slots
            let defs = defined_slots(instr);
            invalidate_cache(&mut expr_map, &defs);
            i += 1;
            continue;
        }

        // --- 3. Stack-based binary + STORE: ADD_LOCAL_LOCAL; STORE_LOCAL ---
        if (op == OP_ADD_LOCAL_LOCAL || op == OP_SUB_LOCAL_LOCAL || op == OP_MUL_LOCAL_LOCAL || op == OP_DIV_LOCAL_LOCAL)
            && i < end
        {
            let next = code[i + 1];
            if next.op() == OP_STORE_LOCAL {
                let left = instr.a();
                let right = instr.b() as u32;
                let dest = next.a();
                let (lo, hi) = normalize_key(op, left, right);
                let key = (op, lo, hi);

                // Only cache if dest is not one of the operands
                if dest != left && dest != right {
                    if let Some(&first_dest) = expr_map.get(&key) {
                        if first_dest != dest {
                            // Replace with LOAD_LOCAL_STORE_LOCAL + remove STORE_LOCAL
                            code[i] = Instr::a2(OP_LOAD_LOCAL_STORE_LOCAL, first_dest, dest as u16);
                            let range = (i + 1)..=(i + 1);
                            code.drain(range);
                            patch_jumps_after(code, wide, i + 1, 1);
                            end -= 1;
                        }
                    } else {
                        expr_map.insert(key, dest);
                    }
                }

                // Invalidate: STORE_LOCAL defines dest
                invalidate_cache(&mut expr_map, &[dest]);
                i += 2;
                continue;
            }
        }

        // --- 4. In-place store-back ops: DO NOT CACHE ---
        // ADD_LOCAL_STORE, SUB_LOCAL_STORE, MUL_LOCAL_STORE, DIV_LOCAL_STORE
        // These mutate operand a, so caching is unsafe.
        if op == OP_ADD_LOCAL_STORE
            || op == OP_SUB_LOCAL_STORE
            || op == OP_MUL_LOCAL_STORE
            || op == OP_DIV_LOCAL_STORE
            || op == OP_ADD_FIELD_LOCAL_STORE
        {
            // Still invalidate because they define slot a
            let defs = defined_slots(instr);
            invalidate_cache(&mut expr_map, &defs);
            i += 1;
            continue;
        }

        // --- 5. Constant-operand in-place ops: DO NOT CACHE ---
        // ADD_INT_LOCAL, SUB_INT_LOCAL, MUL_INT_LOCAL, ADD_FLOAT_LOCAL, etc.
        if op == OP_ADD_INT_LOCAL
            || op == OP_SUB_INT_LOCAL
            || op == OP_MUL_INT_LOCAL
            || op == OP_ADD_FLOAT_LOCAL
            || op == OP_SUB_FLOAT_LOCAL
            || op == OP_MUL_FLOAT_LOCAL
            || op == OP_DIV_FLOAT_LOCAL
            || op == OP_INC_LOCAL
            || op == OP_DEC_LOCAL
            || op == OP_INC_LOCAL_INT
        {
            let defs = defined_slots(instr);
            invalidate_cache(&mut expr_map, &defs);
            i += 1;
            continue;
        }

        // --- 6. Other defining instructions: invalidate and skip ---
        let defs = defined_slots(instr);
        if !defs.is_empty() {
            invalidate_cache(&mut expr_map, &defs);
        }

        // --- 7. Reset map on control flow boundaries ---
        match op {
            OP_JUMP
            | OP_JUMP_IF_FALSE
            | OP_JUMP_IF_NIL
            | OP_RETURN
            | OP_HALT => {
                expr_map.clear();
            }
            _ => {}
        }

        i += 1;
    }
}

/// Invalidate cache entries whose operand slots intersect the given defined slots,
/// and any entry whose `first_dest` is defined.
fn invalidate_cache(expr_map: &mut HashMap<(u8, u32, u32), u32>, defined: &[u32]) {
    if defined.is_empty() {
        return;
    }
    let keys_to_remove: Vec<(u8, u32, u32)> = expr_map
        .iter()
        .filter(|&(&key, &dest)| {
            // Remove if dest is defined
            if defined.contains(&dest) {
                return true;
            }
            // Remove if any operand slot is defined
            let (_, op1, op2) = key;
            if defined.contains(&op1) || defined.contains(&op2) {
                return true;
            }
            false
        })
        .map(|(&key, _)| key)
        .collect();
    for key in keys_to_remove {
        expr_map.remove(&key);
    }
}

/// Patch jump targets after removing instructions.
fn patch_jumps_after(code: &mut [Instr], wide: &mut [i64], after: usize, count: usize) {
    let after_u32 = after as u32;
    let count_u32 = count as u32;
    for instr in code.iter_mut() {
        let op = instr.op();
        match op {
            OP_JUMP
            | OP_JUMP_IF_FALSE
            | OP_JUMP_IF_NIL
            | OP_FOR_ITER_NEXT
            | OP_FOR_RANGE_NEXT => {
                if instr.a() > after_u32 {
                    *instr =
                        Instr::make(op, instr.a() - count_u32, instr.b(), instr.c());
                }
            }
            OP_WHILE_LT_INT
            | OP_WHILE_LT_LOCAL
            | OP_FOR_NUMBER_LOOP => {
                let w_idx = instr.c() as usize;
                if let Some(exit_ip) = wide.get_mut(w_idx)
                    && *exit_ip > after as i64
                {
                    *exit_ip -= count as i64;
                }
            }
            _ => {}
        }
    }
}
