//! # XCL Virtual Machine
//!
//! Core VM for the XCL scripting language: lexer, parser, bytecode compiler, and runtime.
//!
//! ## Overview
//!
//! XCL compiles source code through four stages:
//!
//! 1. **Lexer** — tokenizes source text into a stream of tokens
//! 2. **Parser** — builds an AST from tokens
//! 3. **Bytecode compiler** — translates the AST into bytecode instructions
//! 4. **VM runtime** — executes bytecode on a stack-based virtual machine
//!
//! ## Quick start
//!
//! ```rust
//! use xcl_vm::{VM, Value, evaluate};
//!
//! let mut vm = VM::new();
//! let result = evaluate(r#"42 + 1"#, &mut vm);
//! assert_eq!(result.as_i48(), 43);
//! ```
//!
//! For the standard library (I/O, `Result`, `print`, etc.), use [`xcl_std`](../xcl_std/index.html) instead.

pub mod ast;
pub mod bytecode;
pub mod errors;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod value;
pub mod vm;
pub mod native;
pub mod args;
pub mod wellknown;

pub use bytecode::{Bytecode, Instr};
pub use value::{
    Value, FuncValue, HandleKind, RawPtr,
    NativeEntry, NamedParam, NativeKind, BuiltinImpl,
    TypeDescriptor, TypeKind, TypeField, TypeVariant,
    DomainCtx, NativeArg, extract_arg,
    SUBTYPE_NONE, SUBTYPE_RESULT, SUBTYPE_ERROR, SUBTYPE_HANDLE, SUBTYPE_USER_BASE,
    VARIANT_OK, VARIANT_ERR, VARIANT_DEFAULT,
};
pub use vm::{VM, DomainSlot};
pub use native::{NATIVE_REGISTRY, TYPE_REGISTRY, IMPL_REGISTRY};
pub use args::split_named_args;
pub use wellknown::WellKnown;
pub use span::Span;
pub use errors::{Group, is_known_kind};

/// Evaluate XCL source code using the provided VM.
///
/// This function runs the full pipeline: lexing, parsing, bytecode compilation,
/// and execution. The VM's bump arena is reset before each call.
///
/// # Example
///
/// ```
/// use xcl_vm::{VM, evaluate};
///
/// let mut vm = VM::new();
/// let result = evaluate(r#"10 * 3"#, &mut vm);
/// assert_eq!(result.as_i48(), 30);
/// ```
pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    let tokens = lexer::lex(source, "<input>");
    let ast = parser::parse(&tokens, source);
    let bytecode = bytecode::compile(&ast);
    vm.run(&bytecode)
}