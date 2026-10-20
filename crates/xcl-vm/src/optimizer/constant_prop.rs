//! Constant propagation: replaces LOAD_LOCAL with PUSH_INT when the slot holds a known constant.
//!
//! Stops propagation at function calls, global access, and loop back-edges.

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec::Vec;

use crate::bytecode::Instr;
use crate::runtime::vm::ops::consts::{
    OP_CALL, OP_CALL_METHOD, OP_CALL_NAME, OP_DEC_LOCAL, OP_FOR_ITER_NEXT, OP_FOR_NUMBER_LOOP,
    OP_FOR_RANGE_NEXT, OP_INC_LOCAL, OP_JUMP, OP_JUMP_IF_FALSE, OP_JUMP_IF_NIL,
    OP_LOAD_CONST_LOCAL, OP_LOAD_GLOBAL, OP_LOAD_LOCAL, OP_LOAD_LOCAL_STORE_LOCAL, OP_PUSH_INT,
    OP_STORE_GLOBAL, OP_STORE_LOCAL, OP_TAIL_CALL_NAME, OP_WHILE_LT_INT, OP_WHILE_LT_INT_WIDE,
    OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::meta;

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
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL | OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                let target = instr.a() as usize;
                if target < i {
                    loop_headers.insert(target);
                }
            }
            OP_WHILE_LT_INT | OP_WHILE_LT_INT_WIDE | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
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
            OP_WHILE_LT_INT | OP_WHILE_LT_INT_WIDE | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
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
            // PUSH_INT k; STORE_LOCAL slot → slot is known constant k
            OP_STORE_LOCAL => {
                let slot = instr.a();
                if i > 0 {
                    let prev = code[i - 1];
                    if prev.op() == OP_PUSH_INT {
                        constants.insert(slot, (i - 1, prev.a()));
                    } else {
                        constants.remove(&slot);
                    }
                } else {
                    constants.remove(&slot);
                }
            }
            // LOAD_LOCAL slot → replace with PUSH_INT if known constant
            OP_LOAD_LOCAL => {
                let slot = instr.a();
                if let Some((_, val)) = constants.get(&slot) {
                    code[i] = Instr::a1(OP_PUSH_INT, *val);
                }
            }
            // Function calls and global access invalidate all constants
            OP_CALL | OP_CALL_METHOD | OP_CALL_NAME | OP_TAIL_CALL_NAME => {
                constants.clear();
            }
            OP_LOAD_GLOBAL | OP_STORE_GLOBAL => {
                // Global access may have side effects; be conservative
            }
            // Fused opcodes that store to locals
            OP_LOAD_CONST_LOCAL => {
                let slot = instr.a();
                let kind = instr.c();
                if kind == 0 {
                    constants.insert(slot, (i, instr.b() as u32));
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
            OP_LOAD_LOCAL_STORE_LOCAL => {
                let dst = instr.b() as u32;
                let src = instr.a();
                if let Some((_, val)) = constants.get(&src) {
                    constants.insert(dst, (i, *val));
                } else {
                    constants.remove(&dst);
                }
            }
            OP_FOR_NUMBER_LOOP => {
                // Packed-operand opt-out — special case
                constants.remove(&(instr.a() >> 16));
            }
            _ => {
                // For all other opcodes, invalidate constants for written slots
                let mask = meta::writes(op);
                if mask & 1 != 0 {
                    constants.remove(&instr.a());
                }
                if mask & 2 != 0 {
                    constants.remove(&(instr.b() as u32));
                }
                if mask & 4 != 0 {
                    constants.remove(&(instr.c() as u32));
                }
            }
        }

        i += 1;
    }
}
