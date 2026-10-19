//! Constant propagation: replaces LOAD_LOCAL with PUSH_INT when the slot holds a known constant.
//!
//! Stops propagation at function calls, global access, and loop back-edges.

use crate::bytecode::{
    Instr, OP_ADD_FLOAT_LOCAL, OP_ADD_INT_LOCAL, OP_ADD_LOCAL_LOCAL_STORE, OP_ADD_LOCAL_STORE,
    OP_CALL, OP_CALL_METHOD, OP_DEC_LOCAL, OP_DIV_FLOAT_LOCAL, OP_DIV_LOCAL_STORE,
    OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP, OP_FOR_RANGE_NEXT, OP_INC_LOCAL, OP_LOAD_CONST_LOCAL,
    OP_LOAD_GLOBAL, OP_LOAD_LOCAL, OP_LOAD_LOCAL_STORE_LOCAL, OP_MOD_EQ_INT, OP_MUL_FLOAT_LOCAL,
    OP_MUL_INT_LOCAL, OP_MUL_LOCAL_STORE, OP_PUSH_INT, OP_STORE_LOCAL, OP_SUB_FLOAT_LOCAL,
    OP_SUB_INT_LOCAL, OP_SUB_LOCAL_STORE, OP_WHILE_LT_INT, OP_WHILE_LT_LOCAL, OP_JUMP,
    OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL, OP_STORE_GLOBAL,
};

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet;
#[cfg(feature = "std")]
use std::collections::BTreeSet;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

/// Propagate constants through bytecode.
///
/// Tracks known constant values for each local slot and replaces LOAD_LOCAL
/// with PUSH_INT when the slot is known to hold a constant. Stops propagation
/// at function calls (side effects), global variable access (mutable), and loop back-edges.
pub fn constant_propagation(code: &mut Vec<Instr>, wide: &[i64]) {
    if code.len() <= 1 {
        return;
    }

    // Find all backward jump targets (loop headers) â€” these are targets of
    // jumps where the target index is less than the jump instruction index.
    // Constants must be cleared at these points because loop variables change.
    let mut loop_headers: BTreeSet<usize> = BTreeSet::new();
    for (i, instr) in code.iter().enumerate() {
        let op = instr.op();
        match op {
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL
            | OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                let target = instr.a() as usize;
                if target < i {
                    loop_headers.insert(target);
                }
            }
            OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
                // These opcodes jump back to themselves (the header is at i)
                // and exit forward. The header is the current instruction.
                loop_headers.insert(i);
            }
            _ => {}
        }
    }

    // Also check wide_operands for backward jump targets
    for instr in code.iter() {
        let op = instr.op();
        match op {
            OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
                let w_idx = instr.c() as usize;
                if let Some(&exit_ip) = wide.get(w_idx) {
                    let exit = exit_ip as usize;
                    // The loop header is at the current instruction index
                    // (already added above). The exit target is forward.
                    let _ = exit;
                }
            }
            _ => {}
        }
    }

    // Track known constants per local slot
    // key = slot index, value = (instr_index, constant_value)
    let mut constants: BTreeMap<u32, (usize, u32)> = BTreeMap::new();

    let mut i = 0;
    while i < code.len() {
        // Clear all constants at loop headers â€” values change each iteration
        if loop_headers.contains(&i) {
            constants.clear();
        }

        let instr = code[i];
        let op = instr.op();

        match op {
            // PUSH_INT k; STORE_LOCAL slot â†’ slot is known constant k
            OP_STORE_LOCAL => {
                let slot = instr.a();
                // Check if previous instruction was PUSH_INT
                if i > 0 {
                    let prev = code[i - 1];
                    if prev.op() == OP_PUSH_INT {
                        constants.insert(slot, (i - 1, prev.a()));
                    } else {
                        // Unknown value stored â€” clear constant
                        constants.remove(&slot);
                    }
                } else {
                    constants.remove(&slot);
                }
            }
            // LOAD_LOCAL slot â†’ replace with PUSH_INT if known constant
            OP_LOAD_LOCAL => {
                let slot = instr.a();
                if let Some((_, val)) = constants.get(&slot) {
                    code[i] = Instr::a1(OP_PUSH_INT, *val);
                }
            }
            // Function calls and global access invalidate all constants
            OP_CALL | OP_CALL_METHOD => {
                constants.clear();
            }
            OP_LOAD_GLOBAL | OP_STORE_GLOBAL => {
                // Global access may have side effects; be conservative
                // Don't clear all constants, but don't propagate through globals
            }
            // Fused opcodes that store to locals invalidate those slots
            OP_LOAD_CONST_LOCAL => {
                let slot = instr.a();
                let kind = instr.c();
                if kind == 0 {
                    // Inline int
                    constants.insert(slot, (i, instr.b() as u32));
                } else if kind == 6 {
                    // Wide int â€” can't inline, remove from constants
                    constants.remove(&slot);
                } else {
                    constants.remove(&slot);
                }
            }
            OP_INC_LOCAL | OP_DEC_LOCAL => {
                let slot = instr.a();
                if let Some((_, val)) = constants.get(&slot) {
                    let new_val = match op {
                        OP_INC_LOCAL => val.checked_add(1),
                        OP_DEC_LOCAL => val.checked_sub(1),
                        _ => None,
                    };
                    if let Some(v) = new_val {
                        constants.insert(slot, (i, v));
                    } else {
                        constants.remove(&slot);
                    }
                }
            }
            // Fused ops that modify slots invalidate those slots
            OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE
            | OP_DIV_LOCAL_STORE => {
                let slot = instr.a();
                constants.remove(&slot);
            }
            OP_ADD_LOCAL_LOCAL_STORE => {
                let slot = instr.c() as u32;
                constants.remove(&slot);
            }
            OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL => {
                let slot = instr.a();
                constants.remove(&slot);
            }
            // Float constant ops destroy integer constants in slot a
            OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL | OP_DIV_FLOAT_LOCAL =>
            {
                let slot = instr.a();
                constants.remove(&slot);
            }
            // MOD_EQ_INT reads slot a but doesn't write any local
            OP_MOD_EQ_INT => {
                // No constant tracking needed — just reads a local
            }
            OP_LOAD_LOCAL_STORE_LOCAL => {
                let dst = instr.a();
                let src = instr.b() as u32;
                // If src is a known constant, dst becomes that constant
                if let Some((_, val)) = constants.get(&src) {
                    constants.insert(dst, (i, *val));
                } else {
                    constants.remove(&dst);
                }
            }
            _ => {}
        }

        i += 1;
    }
}
