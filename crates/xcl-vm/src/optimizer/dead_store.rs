//! Dead store elimination: removes STORE instructions whose values are never read.
//!
//! Uses live variable analysis from [`super::dataflow`] to identify dead stores.


// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{
    vec::Vec,
};

use crate::bytecode::{
    Instr, OP_ADD_FLOAT_LOCAL, OP_ADD_INT_LOCAL, OP_ADD_LOCAL_LOCAL, OP_ADD_LOCAL_LOCAL_STORE,
    OP_ADD_LOCAL_STORE, OP_ARRAY_PUSH_LOCAL, OP_CALL_METHOD, OP_DEC_LOCAL, OP_DIV_FLOAT_LOCAL,
    OP_DIV_LOCAL_LOCAL, OP_DIV_LOCAL_LOCAL_STORE, OP_DIV_LOCAL_STORE, OP_FOR_ITER_NEXT,
    OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_GET_FIELD_LOCAL_STORE,
    OP_GET_INDEX_LOCAL_LOCAL_STORE, OP_ADD_FIELD_LOCAL_STORE,
    OP_INC_LOCAL, OP_INC_LOCAL_INT, OP_LOAD_LOCAL_LOAD_LOCAL, OP_LOAD_LOCAL_PUSH_INT,
    OP_LOAD_LOCAL_STORE_LOCAL, OP_LT_LOCAL_LOCAL, OP_MOD_LOCAL_LOCAL_STORE, OP_MUL_FLOAT_LOCAL,
    OP_MUL_INT_LOCAL, OP_MUL_LOCAL_LOCAL, OP_MUL_LOCAL_LOCAL_STORE, OP_MUL_LOCAL_STORE,
    OP_SUB_FLOAT_LOCAL, OP_SUB_INT_LOCAL, OP_SUB_LOCAL_LOCAL, OP_SUB_LOCAL_LOCAL_STORE,
    OP_SUB_LOCAL_STORE, OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL, OP_IF_LT_INT, OP_IF_EQ_INT,
};
use crate::runtime::vm::ops::consts::{
    OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_LOAD_GLOBAL, OP_LOAD_LOCAL, OP_STORE_LOCAL,
};


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
                        match op {
                            OP_LOAD_LOCAL => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE
                            | OP_DIV_LOCAL_STORE => {
                                if later.a() == slot || later.b() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_ADD_LOCAL_LOCAL_STORE | OP_MUL_LOCAL_LOCAL_STORE
                            | OP_SUB_LOCAL_LOCAL_STORE | OP_DIV_LOCAL_LOCAL_STORE
                            | OP_MOD_LOCAL_LOCAL_STORE => {
                                if later.b() as u32 == slot || later.c() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL
                            | OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL
                            | OP_DIV_FLOAT_LOCAL => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_INC_LOCAL | OP_DEC_LOCAL | OP_INC_LOCAL_INT => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_LOAD_LOCAL_STORE_LOCAL => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_LOAD_LOCAL_PUSH_INT => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL
                            | OP_DIV_LOCAL_LOCAL | OP_LT_LOCAL_LOCAL => {
                                if later.a() == slot || later.b() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_GET_INDEX_LOCAL_LOCAL_STORE => {
                                if later.b() as u32 == slot || later.c() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_GET_FIELD_LOCAL_STORE => {
                                if later.c() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            // reads slot a (accumulator) and slot c (object)
                            OP_ADD_FIELD_LOCAL_STORE => {
                                if later.a() == slot || later.c() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_ARRAY_PUSH_LOCAL => {
                                if later.a() == slot || later.b() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_IF_LT_INT | OP_IF_EQ_INT => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_WHILE_LT_INT => {
                                if later.a() == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_WHILE_LT_LOCAL => {
                                if later.a() == slot || later.b() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_FOR_NUMBER_LOOP => {
                                let var_slot = later.a() >> 16;
                                if var_slot == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_LOAD_LOCAL_LOAD_LOCAL => {
                                if later.a() == slot || later.b() as u32 == slot {
                                    found_use = true;
                                    break;
                                }
                            }
                            OP_CALL_METHOD => {
                                // OP_CALL_METHOD reads the receiver from the stack.
                                // Check if the immediately preceding instruction loads this slot.
                                // We can't easily track stack depth, so only protect if
                                // there's a LOAD_LOCAL of this slot right before CALL_METHOD.
                                // This is handled by the LOAD_LOCAL case above.
                            }
                            _ => {}
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
