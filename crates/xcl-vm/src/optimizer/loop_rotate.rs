//! Rotation of canonical counted loops.
//!
//! `while $i < N { body; $i = $i + 1 }` compiles to a test at the top of the
//! body, the body itself, then an increment and an unconditional jump back:
//!
//! ```text
//! H:  WHILE_LT_INT  i, N, exit      ; runs every iteration
//! B:  body...
//!     INC_LOCAL     i
//!     JUMP          H
//! ```
//!
//! Rotating moves the test to the bottom and folds it into the increment:
//!
//! ```text
//! H:  WHILE_LT_INT  i, N, exit      ; entry check, runs once
//! B:  body...
//!     INC_LT_INT    B, N, i         ; increment, compare, jump in one dispatch
//! ```
//!
//! The entry test stays, so a loop that runs zero times and the first iteration
//! behave exactly as before. Every later iteration saves one dispatch (the
//! unconditional back jump) and the test that used to sit at the top.
//!
//! # Why the pattern is this narrow
//!
//! A `continue` compiles to a jump to the loop header. After rotation the test
//! lives at the bottom, so a `continue` would re-test without incrementing —
//! a different program. The pass therefore requires the back edge to be the
//! *only* branch to the header, which is exactly "no `continue`". `break`
//! targets the exit, which rotation does not move.

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec::Vec;

use crate::bytecode::Instr;
use crate::bytecode::peephole::drain_and_patch;
use crate::runtime::vm::ops::consts::{
    OP_INC_LOCAL, OP_INC_LT_INT, OP_INC_LT_INT_WIDE, OP_JUMP, OP_WHILE_LT_INT, OP_WHILE_LT_INT_WIDE,
};
use crate::runtime::vm::ops::meta;

use super::dataflow::{build_basic_blocks, slot_indices};
use super::licm::detect_loops;

/// Upper bound on rotations per compilation unit; each rewrite re-runs the loop
/// analysis, so this keeps a loop-heavy program from going quadratic.
const MAX_ROTATIONS: usize = 16;

/// Rotates every canonical counted loop it can prove safe.
pub fn rotate_counted_loops(code: &mut Vec<Instr>, wide: &mut [i64]) {
    for _ in 0..MAX_ROTATIONS {
        if !rotate_first(code, wide) {
            break;
        }
    }
}

fn rotate_first(code: &mut Vec<Instr>, wide: &mut [i64]) -> bool {
    let blocks = build_basic_blocks(code, wide);
    let loops = detect_loops(code, &blocks);

    for loop_instrs in &loops {
        let Some(&header) = loop_instrs.iter().min() else {
            continue;
        };
        let head = code[header];
        // `a` = counter slot, `b` = limit (or its wide index).
        let (latch_op, slot) = match head.op() {
            OP_WHILE_LT_INT => (OP_INC_LT_INT, head.a()),
            OP_WHILE_LT_INT_WIDE => (OP_INC_LT_INT_WIDE, head.a()),
            _ => continue,
        };
        let Ok(slot8) = u8::try_from(slot) else {
            continue;
        };

        // The latch is the loop's last instruction: `INC_LOCAL i; JUMP header`.
        //
        // Taking the last instruction rather than the only branch to the header
        // is what lets a loop with `continue` rotate: a `continue` also jumps to
        // the header, but the entry test stays in place, so it keeps its meaning.
        let Some(&latch) = loop_instrs.iter().max() else {
            continue;
        };
        if code[latch].op() != OP_JUMP || code[latch].a() as usize != header {
            continue;
        }

        if latch < header + 2 || code[latch - 1].op() != OP_INC_LOCAL {
            continue;
        }
        if code[latch - 1].a() != slot {
            continue;
        }
        // The counter may only be written by the increment being replaced; a
        // second write makes the loop's trip count depend on the body, and the
        // rotation would have to reason about it.
        let writes: usize = loop_instrs
            .iter()
            .filter(|&&i| slot_indices(meta::writes(code[i].op()), code[i]).contains(&slot))
            .count();
        if writes != 1 {
            continue;
        }

        // `INC_LT_* target=<body>, limit, slot`, replacing the increment and the
        // back jump. The body starts right after the entry test.
        code[latch - 1] = Instr::make(latch_op, (header + 1) as u32, head.b(), slot8);
        drain_and_patch(code, wide, latch..=latch);
        return true;
    }
    false
}
