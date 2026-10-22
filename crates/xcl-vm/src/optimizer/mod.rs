//! Bytecode optimization passes: data flow analysis, dead store elimination,
//! constant propagation, loop-invariant code motion, and loop unrolling.
//!
//! # Pipeline
//!
//! Each pass takes `&mut Vec<Instr>` and `&mut [i64]` (wide operands) and
//! transforms the bytecode in place. The [`optimize`] function runs all passes
//! in the correct order.

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec::Vec;

mod constant_prop;
mod cse;
mod dataflow;
mod dead_store;
mod licm;
mod loop_rotate;
mod loop_unroll;

/// Run all data flow optimization passes on bytecode.
///
/// `locals` receives the slots the passes introduce (loop hoisting needs
/// somewhere to keep an invariant value), and `hot` decides which hot native
/// slots are pure enough to move.
pub fn optimize(
    code: &mut Vec<crate::bytecode::Instr>,
    wide: &mut [i64],
    pinned: u32,
    locals: &mut Vec<Box<str>>,
    hot: Option<&crate::runtime::vm::HotTable>,
) {
    // Dead store elimination
    dead_store::eliminate_dead_stores(code, wide, pinned);

    // Constant propagation
    constant_prop::constant_propagation(code, wide);

    // Loop-invariant code motion — hoists LOAD_GLOBAL before loops
    licm::loop_invariant_code_motion(code, wide);

    // Pure hot calls are loop-invariant too, and each one costs a dispatch.
    // This is the pass that turns `while $i < N { if is_ok $r { ... } }` from a
    // call per iteration into one call.
    if let Some(hot) = hot {
        licm::hoist_pure_calls(code, wide, locals, |slot| hot.is_pure(slot));
    }

    // Common subexpression elimination — removes redundant computations
    cse::eliminate_common_subexpressions(code, wide);

    // Loop unrolling — reduces branch overhead in tight loops
    loop_unroll::loop_unroll(code, wide);

    // Counted-loop rotation runs last: it fuses the increment with the test and
    // the back jump, which only helps once every other pass has settled the
    // loop's shape.
    loop_rotate::rotate_counted_loops(code, wide);

    // Fusing a condition load into its branch comes last: the hoisting above is
    // what creates most of the `LOAD_LOCAL; JUMP_IF_FALSE` pairs.
    loop {
        let changed = crate::bytecode::peephole::fuse_local_branch(code, wide)
            | crate::bytecode::peephole::fuse_conditional_increment(code, wide);
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests;
