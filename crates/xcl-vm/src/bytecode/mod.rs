//! Compact 64-bit bytecode instruction set, compiler, and peephole optimizer.
//!
//! # Overview
//!
//! The XCL bytecode system translates AST expressions into a high-density, cache-friendly
//! stream of 64-bit instructions executed by the [`crate::vm::VM`].
//!
//! # Instruction Encoding
//!
//! Every [`Instr`] is a single transparent 64-bit integer (`u64`) partitioned into four fields:
//!
//! ```text
//!  63        56 55              40 39                              8 7        0
//! +------------+------------------+---------------------------------+----------+
//! |  c (8-bit) |    b (16-bit)    |           a (32-bit)            | op (8-bit|
//! +------------+------------------+---------------------------------+----------+
//! ```
//!
//! * **`op` (bits 0..8)**: 8-bit opcode identifying the instruction.
//! * **`a` (bits 8..40)**: 32-bit primary operand (jump destination instruction pointer,
//!   string pool index, wide operand table index, or local slot index).
//! * **`b` (bits 40..56)**: 16-bit secondary operand (argument count, second local slot, or small immediate).
//! * **`c` (bits 56..64)**: 8-bit auxiliary operand or configuration flag.
//!
//! # Superinstructions & Peephole Optimization
//!
//! The compiler includes an integrated peephole optimizer that fuses common instruction
//! patterns into single superinstructions to eliminate stack traffic and branch penalties:
//!
//! * **Arithmetic fusion**: `LOAD_LOCAL $a; LOAD_LOCAL $b; ADD; STORE_LOCAL $c` is fused
//!   into [`OP_ADD_LOCAL_LOCAL_STORE`].
//! * **Loop specialization**: Single-opcode numeric range loops use [`OP_FOR_NUMBER_LOOP`].
//! * **Fused while branches**: [`OP_WHILE_LT_INT`] and [`OP_WHILE_LT_LOCAL`] execute loop
//!   comparisons and jumps in a single dispatch step.
//!
//! # Examples
//!
//! Compiling an AST into bytecode:
//!
//! ```
//! use xcl_vm::compiler::lexer::lex;
//! use xcl_vm::compiler::parser::parse;
//! use xcl_vm::bytecode::{compile, OP_ADD};
//!
//! let tokens = lex("20 + 30", "<eval>");
//! let ast = parse(&tokens, "20 + 30");
//! let code = compile(&ast);
//!
//! assert!(!code.instructions.is_empty());
//! ```

mod compiler;
mod instr;
mod opcodes;
mod peephole;

pub use instr::Instr;
pub use opcodes::{Bytecode, *};
pub use compiler::compile;
