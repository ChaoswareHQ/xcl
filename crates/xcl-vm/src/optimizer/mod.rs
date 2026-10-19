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
use alloc::{
    vec::Vec,
};


mod cse;
mod constant_prop;
mod dataflow;
mod dead_store;
mod licm;
mod loop_unroll;

/// Run all data flow optimization passes on bytecode.
pub fn optimize(code: &mut Vec<crate::bytecode::Instr>, wide: &mut [i64]) {
    // Dead store elimination
    dead_store::eliminate_dead_stores(code, wide);

    // Constant propagation
    constant_prop::constant_propagation(code, wide);

    // Loop-invariant code motion — hoists LOAD_GLOBAL before loops
    licm::loop_invariant_code_motion(code, wide);

    // Common subexpression elimination — removes redundant computations
    cse::eliminate_common_subexpressions(code, wide);

    // Loop unrolling — reduces branch overhead in tight loops
    loop_unroll::loop_unroll(code, wide);
}

#[cfg(test)]
mod tests;
