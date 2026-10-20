//! Dead store elimination: removes STORE instructions whose values are never read.
//!
//! Uses live variable analysis from [`super::dataflow`] to identify dead stores.


// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{
    vec::Vec,
};

use crate::bytecode::Instr;
use crate::runtime::vm::ops::consts::{
    OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_JUMP,
    OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_GLOBAL, OP_STORE_LOCAL,
    OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::meta;


use super::dataflow::{build_basic_blocks, live_variables};

/// Eliminate dead stores from bytecode using data flow analysis.
///
/// A store is dead if the stored variable is never loaded before being
/// redefined. Also eliminates dead LOAD_GLOBAL instructions whose result is never used.
pub fn eliminate_dead_stores(code: &mut Vec<Instr>, wide: &mut [i64]) {
    if code.len() <= 1 {
        return;
    }

    let blocks = build_basic_blocks(code.as_slice(), wide);
    let live = live_variables(code, &blocks);

    // Mark instructions for removal
    let mut remove: Vec<bool> = vec![false; code.len()];

    for (i, instr) in code.iter().enumerate() {
        match instr.op() {
            OP_STORE_LOCAL => {
                let slot = instr.a();
                // Check if this slot is live after this instruction
                // Look at the next instruction's live set
                let is_live = if i + 1 < code.len() {
                    live[i + 1].contains(&slot)
                } else {
                    false
                };
                if !is_live {
                    // Conservative: also check if any later instruction reads this slot
                    // This handles cases where liveness analysis misses cross-block usage
                    let mut found_use = false;
                    for later in code.iter().skip(i + 1) {
                        let op = later.op();
                        // Use metadata for slot reads
                        let mask = meta::reads(op);
                        if mask & 1 != 0 && later.a() == slot {
                            found_use = true;
                            break;
                        }
                        if mask & 2 != 0 && later.b() as u32 == slot {
                            found_use = true;
                            break;
                        }
                        if mask & 4 != 0 && later.c() as u32 == slot {
                            found_use = true;
                            break;
                        }
                        // OP_FOR_NUMBER_LOOP is a packed-operand opt-out — special case
                        // (see docs/tasks/opcode-slot-meta.md)
                        if op == OP_FOR_NUMBER_LOOP {
                            let var_slot = later.a() >> 16;
                            if var_slot == slot {
                                found_use = true;
                                break;
                            }
                        }
                    }
                    if found_use {
                        continue;
                    }
                }
                if !is_live {
                    remove[i] = true;
                }
            }
            OP_LOAD_GLOBAL => {
                let _ = instr;
                // Check if the result is used: look ahead for the next instruction
                // that consumes the stack value
                if i + 1 < code.len() {
                    let next = code[i + 1];
                    // If next is STORE_LOCAL to a dead slot, this load is dead
                    if next.op() == OP_STORE_LOCAL {
                        let slot = next.a();
                        let is_live = if i + 2 < code.len() {
                            live[i + 2].contains(&slot)
                        } else {
                            false
                        };
                        if !is_live {
                            remove[i] = true;
                            remove[i + 1] = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Remove marked instructions in reverse order to preserve indices
    for i in (0..code.len()).rev() {
        if remove[i] {
            code.remove(i);
            // Patch jump targets
            patch_jump_targets(code, wide, i, 1);
        }
    }
}

/// Patch jump targets after removing `count` instructions at position `pos`.
fn patch_jump_targets(code: &mut [Instr], wide: &mut [i64], pos: usize, count: usize) {
    let pos_u32 = pos as u32;
    let count_u32 = count as u32;
    for instr in code.iter_mut() {
        let op = instr.op();
        match op {
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL | OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                if instr.a() > pos_u32 {
                    *instr = Instr::make(op, instr.a() - count_u32, instr.b(), instr.c());
                }
            }
            OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
                let w_idx = instr.c() as usize;
                if let Some(exit_ip) = wide.get_mut(w_idx)
                    && *exit_ip > pos as i64
                {
                    *exit_ip -= count as i64;
                }
            }
            _ => {}
        }
    }
}
