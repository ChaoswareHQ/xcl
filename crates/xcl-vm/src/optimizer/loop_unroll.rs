//! Loop unrolling: reduces branch overhead by duplicating loop bodies.
//!
//! Detects tight loops (small body, no function calls, simple induction variable)
//! and unrolls them 4x, adjusting the loop counter accordingly.


// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{
    vec::Vec,
};

use crate::bytecode::{
    Instr, OP_ADD_INT_LOCAL, OP_CALL, OP_CALL_METHOD, OP_DEC_LOCAL, OP_FOR_ITER_NEXT,
    OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_INC_LOCAL, OP_SUB_INT_LOCAL, OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::consts::{OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_GLOBAL};

use super::dataflow::{build_basic_blocks, BasicBlock};

/// Maximum loop body size to consider for unrolling.
const MAX_LOOP_BODY_SIZE: usize = 16;
/// Unroll factor: duplicate the loop body this many times.
const UNROLL_FACTOR: usize = 4;

/// Checks if an instruction is a function call or other side-effecting operation
/// that prevents safe unrolling.
#[inline]
fn is_side_effecting(instr: Instr) -> bool {
    matches!(
        instr.op(),
        OP_CALL
            | OP_CALL_METHOD
            | OP_LOAD_GLOBAL
            | OP_FOR_ITER_NEXT
            | OP_FOR_RANGE_NEXT
            | OP_FOR_NUMBER_LOOP
    )
}

/// Finds the induction variable in a loop body.
/// Returns (slot, increment) if the loop uses `$slot = $slot + increment`.
fn find_induction_variable(code: &[Instr], loop_start: usize, loop_end: usize) -> Option<(u32, i64)> {
    for idx in loop_start..=loop_end {
        let instr = code.get(idx)?;
        // Pattern: INC_LOCAL a (increment by 1)
        if instr.op() == OP_INC_LOCAL {
            return Some((instr.a(), 1));
        }
        // Pattern: DEC_LOCAL a (decrement by 1)
        if instr.op() == OP_DEC_LOCAL {
            return Some((instr.a(), -1));
        }
        // Pattern: ADD_INT_LOCAL a, k (increment by k)
        if instr.op() == OP_ADD_INT_LOCAL {
            return Some((instr.a(), instr.b() as i64));
        }
        // Pattern: SUB_INT_LOCAL a, k (decrement by k)
        if instr.op() == OP_SUB_INT_LOCAL {
            return Some((instr.a(), -(instr.b() as i64)));
        }
    }
    None
}

/// Unrolls tight loops to reduce branch overhead.
///
/// Strategy:
/// 1. Find loops with small bodies (≤ MAX_LOOP_BODY_SIZE instructions)
/// 2. Check the loop has no function calls or side effects
/// 3. Find the induction variable ($i = $i + 1)
/// 4. Duplicate the loop body UNROLL_FACTOR times
/// 5. Adjust the induction variable increment
pub(crate) fn loop_unroll(code: &mut Vec<Instr>, wide: &[i64]) {
    let blocks = build_basic_blocks(code, wide);
    if blocks.is_empty() {
        return;
    }

    // Find backward jumps to identify loop headers
    let mut loop_headers: Vec<usize> = Vec::new();
    for (idx, instr) in code.iter().enumerate() {
        let op = instr.op();
        if op == OP_JUMP_IF_FALSE || op == OP_JUMP_IF_NIL {
            let target = instr.a() as usize;
            if target < idx {
                loop_headers.push(target);
            }
        } else if op == OP_WHILE_LT_INT || op == OP_WHILE_LT_LOCAL {
            // These are loop headers themselves
            loop_headers.push(idx);
        }
    }

    // Deduplicate and sort
    loop_headers.sort_unstable();
    loop_headers.dedup();

    for header in loop_headers {
        if !try_unroll_loop(code, wide, &blocks, header) {
            continue;
        }
    }
}

/// Try to unroll a single loop starting at `header`.
fn try_unroll_loop(
    code: &mut Vec<Instr>,
    wide: &[i64],
    _blocks: &[BasicBlock],
    header: usize,
) -> bool {
    let _len = code.len();

    // Find the loop body: from header to the backward jump target
    let mut loop_end = header;
    let mut jump_back_target: Option<usize> = None;

    for (idx, instr) in code[header..].iter().enumerate() {
        let idx = header + idx;
        let op = instr.op();

        // Check for side effects
        if is_side_effecting(*instr) {
            return false;
        }

        // Find backward jump
        if op == OP_JUMP_IF_FALSE || op == OP_JUMP_IF_NIL {
            let target = instr.a() as usize;
            if target <= header {
                jump_back_target = Some(target);
                loop_end = idx;
                break;
            }
        } else if op == OP_WHILE_LT_INT || op == OP_WHILE_LT_LOCAL {
            // The while opcode IS the loop condition
            loop_end = idx;
            // Find the exit target from wide_operands
            let w_idx = instr.c() as usize;
            if let Some(&exit_ip) = wide.get(w_idx) {
                _ = exit_ip; // Used for patching
            }
            jump_back_target = Some(header);
            break;
        } else if op == OP_JUMP {
            let target = instr.a() as usize;
            if target < header {
                jump_back_target = Some(target);
                loop_end = idx;
                break;
            }
        }
    }

    let Some(back_target) = jump_back_target else {
        return false;
    };

    let body_start = if back_target <= header { header } else { back_target };
    let body_end = loop_end;
    let body_size = body_end - body_start + 1;

    // Only unroll small loops
    if body_size > MAX_LOOP_BODY_SIZE || body_size == 0 {
        return false;
    }

    // Find the induction variable
    let Some((slot, increment)) = find_induction_variable(code, body_start, body_end) else {
        return false;
    };

    // Calculate new increment for unrolled loop
    let new_increment = increment * UNROLL_FACTOR as i64;
    if new_increment == increment {
        return false;
    }

    // Collect the loop body instructions (excluding the jump at the end)
    let mut body_instrs: Vec<Instr> = Vec::new();
    body_instrs.extend(&code[body_start..body_end]);

    // Build the unrolled body: duplicate UNROLL_FACTOR times
    let mut unrolled_body: Vec<Instr> = Vec::new();
    for i in 0..UNROLL_FACTOR {
        if i > 0 {
            // Between copies, add the induction variable increment
            // We'll adjust the last increment to be the total
            for instr in &body_instrs {
                unrolled_body.push(*instr);
            }
        } else {
            for instr in &body_instrs {
                unrolled_body.push(*instr);
            }
        }
    }

    // Replace the induction variable increment in the last copy with the total increment
    // Find the INC/ADD_INT_LOCAL instruction in the last copy and adjust it
    let last_copy_start = (UNROLL_FACTOR - 1) * body_size;
    for instr in unrolled_body[last_copy_start..].iter_mut() {
        match instr.op() {
            OP_INC_LOCAL if instr.a() == slot => {
                // Replace INC_LOCAL with ADD_INT_LOCAL using the total increment
                let inc_val = new_increment.unsigned_abs();
                if u16::try_from(inc_val).is_ok() {
                    *instr = Instr::a2(
                        if new_increment > 0 {
                            OP_ADD_INT_LOCAL
                        } else {
                            OP_SUB_INT_LOCAL
                        },
                        slot,
                        inc_val as u16,
                    );
                }
                break;
            }
            OP_ADD_INT_LOCAL if instr.a() == slot => {
                let current_val = instr.b() as i64;
                let total = current_val * UNROLL_FACTOR as i64;
                if total > 0 && total <= u16::MAX as i64 {
                    *instr = Instr::a2(OP_ADD_INT_LOCAL, slot, total as u16);
                }
                break;
            }
            _ => {}
        }
    }

    // Replace the loop body in the code
    let old_range = body_start..=body_end;
    let old_count = old_range.end() - old_range.start() + 1;

    // Remove the old body
    code.drain(old_range);

    // Insert the unrolled body
    for (i, instr) in unrolled_body.into_iter().enumerate() {
        code.insert(body_start + i, instr);
    }

    // Patch jump targets
    let new_count = code.len() - body_start;
    let delta = new_count as i64 - old_count as i64;
    for instr in code.iter_mut() {
        let op = instr.op();
        match op {
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL => {
                let target = instr.a() as i64;
                if target > body_start as i64 {
                    let new_target = target + delta;
                    *instr = Instr::make(op, new_target as u32, instr.b(), instr.c());
                }
            }
            _ => {}
        }
    }

    true
}
