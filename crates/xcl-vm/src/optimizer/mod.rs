//! Bytecode optimization passes: data flow analysis, dead store elimination,
//! constant propagation, and loop-invariant code motion.
//!
//! # Pipeline
//!
//! Each pass takes `&mut Vec<Instr>` and `&mut [i64]` (wide operands) and
//! transforms the bytecode in place. The [`optimize`] function runs all passes
//! in the correct order.

mod constant_prop;
mod dataflow;
mod dead_store;
mod licm;

/// Run all data flow optimization passes on bytecode.
pub fn optimize(code: &mut Vec<crate::bytecode::Instr>, wide: &mut [i64]) {
    // Dead store elimination
    dead_store::eliminate_dead_stores(code, wide);

    // Constant propagation
    constant_prop::constant_propagation(code, wide);

    // Loop-invariant code motion — hoists LOAD_GLOBAL before loops
    licm::loop_invariant_code_motion(code, wide);
}

#[cfg(test)]
mod tests;
