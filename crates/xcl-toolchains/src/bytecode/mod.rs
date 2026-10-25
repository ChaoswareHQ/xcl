//! Bytecode compiler: AST to instructions, plus the peephole optimizer.
//!
//! The instruction format, the `Bytecode` container, and the `.xclb` encoding
//! live in the engine ([`xcl_rt::bytecode`]); this module re-exports them so
//! callers can keep treating `bytecode` as one namespace, and adds the
//! compilation entry points.

pub use self::codegen::{compile, compile_hot, compile_unoptimized};
pub use xcl_rt::bytecode::*;

pub mod codegen;
pub(crate) mod peephole;
