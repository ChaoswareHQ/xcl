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
//! For the plugin library (I/O, `Result`, `print`, etc.), use [`xcl_plugins`](../xcl_plugins/index.html) instead.

// Strict clippy lints — no dead code, no duplicate code, no default allows
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
#![warn(clippy::allow_attributes)]
#![deny(dead_code)]
#![deny(unused_imports)]
#![deny(unused_variables)]
#![deny(unused_assignments)]
#![deny(unreachable_code)]
#![deny(unused_must_use)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(rust_2018_idioms)]
#![allow(missing_docs)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_safety_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::default_trait_access)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::unused_self)]
#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::default_numeric_fallback)]
#![allow(clippy::type_repetition_in_bounds)]
#![allow(clippy::missing_assert_message)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::missing_asserts_for_indexing)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::if_not_else)]
#![allow(clippy::manual_assert)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::comparison_chain)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::match_wildcard_for_single_variants)]
#![allow(clippy::single_match)]
#![allow(clippy::match_wild_err_arm)]
#![allow(clippy::from_over_into)]
#![allow(clippy::let_unit_value)]
#![allow(clippy::unit_arg)]
#![allow(clippy::new_without_default)]
#![allow(clippy::new_ret_no_self)]
#![allow(clippy::inherent_to_string_shadow_display)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_inline_in_public_items)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::blocks_in_conditions)]
#![allow(clippy::needless_raw_string_hashes)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_else)]
#![allow(clippy::manual_map)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::map_unwrap_or_else)]
#![allow(clippy::option_map_unwrap_or_else)]
#![allow(clippy::result_map_err_into_error)]
#![allow(clippy::should_panic_without_expect)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_borrowed_reference)]

#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate alloc;

/// Abstract syntax tree nodes representing parsed XCL code.
pub mod ast;
/// Bytecode compiler and instruction set for the XCL VM.
pub mod bytecode;
/// Error types and error grouping for XCL execution.
pub mod errors;
/// Lexer that tokenizes XCL source code into tokens.
pub mod lexer;
/// Parser that builds an AST from tokenized XCL source.
pub mod parser;
/// Source location spans for error reporting and debugging.
pub mod span;
/// Value types, heap objects, and native function bindings.
pub mod value;
/// Virtual machine runtime for executing XCL bytecode.
pub mod vm;
/// Native function registry and built-in function infrastructure.
pub mod native;
/// Argument parsing utilities for named and positional arguments.
pub mod args;
/// Well-known types and their metadata.
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