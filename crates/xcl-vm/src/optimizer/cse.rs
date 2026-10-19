//! Common Subexpression Elimination (CSE): removes redundant computations.
//!
//! Detects repeated expressions within basic blocks and computes them once,
//! replacing duplicates with references to the first computation.

use crate::bytecode::{
    Instr, OP_ADD, OP_ADD_INT_LOCAL, OP_ADD_LOCAL_LOCAL, OP_ADD_LOCAL_LOCAL_STORE,
    OP_ADD_LOCAL_STORE, OP_DIV, OP_DIV_LOCAL_LOCAL_STORE, OP_DIV_LOCAL_STORE, OP_LOAD_LOCAL,
    OP_LOAD_LOCAL_STORE_LOCAL, OP_MUL, OP_MUL_INT_LOCAL, OP_MUL_LOCAL_LOCAL,
    OP_MUL_LOCAL_LOCAL_STORE, OP_MUL_LOCAL_STORE, OP_PUSH_INT, OP_STORE_LOCAL, OP_SUB,
    OP_SUB_INT_LOCAL, OP_SUB_LOCAL_LOCAL, OP_SUB_LOCAL_LOCAL_STORE, OP_SUB_LOCAL_STORE,
};

use super::dataflow::{build_basic_blocks, BasicBlock};

use std::collections::HashMap;

/// Local CSE: eliminates common subexpressions within each basic block.
///
/// Strategy:
/// 1. For each basic block, scan for repeated binary expressions
/// 2. When a duplicate is found, replace it with a LOAD_LOCAL to the result slot
/// 3. Handle both stack-based (LOAD_LOCAL; LOAD_LOCAL; OP) and fused opcodes
pub(crate) fn eliminate_common_subexpressions(code: &mut Vec<Instr>, wide: &mut [i64]) {
    let blocks = build_basic_blocks(code);
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

    // Map from expression signature to (first_occurrence_index, result_slot)
    let mut expr_map: HashMap<(u8, u32, u32), (usize, u32)> = HashMap::new();

    let mut i = start;
    while i <= end && i < code.len() {
        let instr = code[i];
        let op = instr.op();

        // Detect binary expressions on two locals
        if (op == OP_ADD_LOCAL_LOCAL
            || op == OP_SUB_LOCAL_LOCAL
            || op == OP_MUL_LOCAL_LOCAL)
            && i < end
        {
            let next = code[i + 1];
            if next.op() == OP_STORE_LOCAL {
                let a = instr.a();
                let b = instr.b() as u32;
                let key = (op, a, b);
                let dest = next.a();

                if let Some(&(_, first_dest)) = expr_map.get(&key) {
                    // Duplicate found — replace this expression with LOAD_LOCAL of the first result
                    code[i] = Instr::a1(OP_LOAD_LOCAL, first_dest);
                    // Remove the redundant STORE_LOCAL
                    let range = (i + 1)..=(i + 1);
                    code.drain(range);
                    // Adjust block end and patch jumps
                    patch_jumps_after(code, wide, i + 1, 1);
                    end -= 1;
                } else {
                    expr_map.insert(key, (i, dest));
                }
                i += 2;
                continue;
            }
        }

        // Detect fused local-store patterns
        if op == OP_ADD_LOCAL_STORE
            || op == OP_SUB_LOCAL_STORE
            || op == OP_MUL_LOCAL_STORE
            || op == OP_DIV_LOCAL_STORE
        {
            let a = instr.a();
            let b = instr.b() as u32;
            let key = (op, a, b);

            if let Some(&(_, first_dest)) = expr_map.get(&key) {
                code[i] = Instr::a2(OP_LOAD_LOCAL_STORE_LOCAL, first_dest, a as u16);
                i += 1;
                continue;
            } else {
                expr_map.insert(key, (i, a));
            }
        }

        // Detect three-operand fused patterns
        if op == OP_ADD_LOCAL_LOCAL_STORE
            || op == OP_SUB_LOCAL_LOCAL_STORE
            || op == OP_MUL_LOCAL_LOCAL_STORE
            || op == OP_DIV_LOCAL_LOCAL_STORE
        {
            let dest = instr.a();
            let src1 = instr.b() as u32;
            let src2 = instr.c() as u32;
            let key = (op, src1, src2);

            if let Some(&(_, first_dest)) = expr_map.get(&key) {
                if first_dest != dest {
                    code[i] = Instr::a2(OP_LOAD_LOCAL_STORE_LOCAL, first_dest, dest as u16);
                }
                i += 1;
                continue;
            } else {
                expr_map.insert(key, (i, dest));
            }
        }

        // Detect constant-operand patterns
        if op == OP_ADD_INT_LOCAL || op == OP_SUB_INT_LOCAL || op == OP_MUL_INT_LOCAL {
            let slot = instr.a();
            let constant = instr.b() as u32;
            let key = (op, slot, constant);

            if let Some(&(_, first_dest)) = expr_map.get(&key) {
                code[i] = Instr::a2(OP_LOAD_LOCAL_STORE_LOCAL, first_dest, slot as u16);
                i += 1;
                continue;
            } else {
                expr_map.insert(key, (i, slot));
            }
        }

        // Track stack-based binary expressions: LOAD_LOCAL a; LOAD_LOCAL b; OP; STORE_LOCAL dest
        if op == OP_LOAD_LOCAL && i + 3 <= end {
            let a = instr.a();
            let next = code[i + 1];
            let next2 = code[i + 2];
            let next3 = code[i + 3];

            if next.op() == OP_LOAD_LOCAL {
                let b = next.a();
                let bin_op = next2.op();
                if (bin_op == OP_ADD || bin_op == OP_SUB || bin_op == OP_MUL || bin_op == OP_DIV)
                    && next3.op() == crate::bytecode::OP_STORE_LOCAL
                {
                    let dest = next3.a();
                    let key = (bin_op, a, b);

                    if let Some(&(_, first_dest)) = expr_map.get(&key) {
                        // Replace the 4-instruction sequence with a single LOAD_LOCAL
                        code[i] = Instr::a1(OP_LOAD_LOCAL, first_dest);
                        let range = (i + 1)..=(i + 3);
                        code.drain(range);
                        patch_jumps_after(code, wide, i + 1, 3);
                        end -= 3;
                        i += 1;
                        continue;
                    } else {
                        expr_map.insert(key, (i, dest));
                    }
                }
            }
        }

        // Track PUSH_INT; STORE_LOCAL as a constant expression
        if op == OP_PUSH_INT && i < end {
            let next = code[i + 1];
            if next.op() == crate::bytecode::OP_STORE_LOCAL {
                let constant = instr.a();
                let dest = next.a();
                let key = (OP_PUSH_INT, 0, constant);
                expr_map.insert(key, (i, dest));
            }
        }

        // Reset map on control flow boundaries
        match op {
            crate::bytecode::OP_JUMP
            | crate::bytecode::OP_JUMP_IF_FALSE
            | crate::bytecode::OP_JUMP_IF_NIL
            | crate::bytecode::OP_RETURN
            | crate::bytecode::OP_HALT => {
                expr_map.clear();
            }
            _ => {}
        }

        i += 1;
    }
}

/// Patch jump targets after removing instructions.
fn patch_jumps_after(code: &mut [Instr], wide: &mut [i64], after: usize, count: usize) {
    let after_u32 = after as u32;
    let count_u32 = count as u32;
    for instr in code.iter_mut() {
        let op = instr.op();
        match op {
            crate::bytecode::OP_JUMP
            | crate::bytecode::OP_JUMP_IF_FALSE
            | crate::bytecode::OP_JUMP_IF_NIL
            | crate::bytecode::OP_FOR_ITER_NEXT
            | crate::bytecode::OP_FOR_RANGE_NEXT => {
                if instr.a() > after_u32 {
                    *instr =
                        Instr::make(op, instr.a() - count_u32, instr.b(), instr.c());
                }
            }
            crate::bytecode::OP_WHILE_LT_INT
            | crate::bytecode::OP_WHILE_LT_LOCAL
            | crate::bytecode::OP_FOR_NUMBER_LOOP => {
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
