//! Core Virtual Machine and compiler infrastructure for the XCL scripting language.
//!
//! The `xcl-vm` crate provides the complete language pipeline for XCL: a high-speed
//! regular-expression lexer, a Pratt/recursive-descent parser, a bytecode compiler with
//! peephole optimization, and a stack-based virtual machine runtime with NaN-boxed values.
//!
//! # Overview
//!
//! XCL is an expression-oriented, embeddable scripting language designed for automation,
//! configuration, and CLI workflows. `xcl-vm` implements the core VM engine without
//! hard-coding heavyweight standard libraries.
//!
//! Higher-level functionality (such as filesystem I/O, process execution, networking, and
//! standard library functions) is provided via plugins in `xcl-plugins`.
//!
//! # Compilation and Execution Pipeline
//!
//! Source code passes through four distinct phases:
//!
//! 1. **Lexical Analysis ([`lexer`])**
//!    Transforms raw UTF-8 source text into a linear token stream ([`TokenWithText`](lexer::TokenWithText))
//!    using [`logos`]. Source locations are tracked via [`Span`].
//!
//! 2. **Parsing ([`parser`] & [`ast`])**
//!    Converts the token stream into an Abstract Syntax Tree ([`AstNode`](ast::AstNode)).
//!    Statements and expressions are parsed using recursive descent and Pratt operator-precedence climbing.
//!
//! 3. **Bytecode Compilation ([`bytecode`])**
//!    Translates AST nodes into 64-bit packed instructions ([`Instr`]).
//!    Applies peephole optimization to fuse common arithmetic and loop patterns into single superinstructions.
//!
//! 4. **Runtime Execution ([`vm`] & [`value`])**
//!    Executes bytecode on a stack-based virtual machine ([`VM`]) operating on 64-bit
//!    NaN-boxed [`Value`] words.
//!
//! # Representation and Memory Architecture
//!
//! * **NaN-Boxed Values ([`Value`])**:
//!   All runtime values fit into an 8-byte machine word. Primitives (integers up to 48 bits, floats,
//!   booleans, and nil) require zero heap allocation. Heap objects (strings, arrays, maps, closures,
//!   and OS handles) store their pointer addresses in the lower 48 bits of a quiet NaN payload.
//!
//! * **Bump Allocation ([`bumpalo::Bump`])**:
//!   Temporary heap allocations generated during execution are serviced by the VM's internal
//!   bump arena ([`VM::arena`](vm::VM::arena)), ensuring microsecond allocation speeds and instant
//!   batch deallocation.
//!
//! * **Compact 64-bit Instructions ([`Instr`])**:
//!   Bytecode instructions pack an 8-bit opcode, 32-bit primary operand `a`, 16-bit operand `b`,
//!   and 8-bit operand `c` into a single 64-bit word for optimal CPU cache utilization.
//!
//! # Extensibility and Native Registries
//!
//! Native Rust functions and custom types can be registered into compile-time distributed
//! slices using [`linkme`]:
//!
//! * [`NATIVE_REGISTRY`] — Static slice of native functions and methods.
//! * [`TYPE_REGISTRY`] — Static slice of custom type definitions.
//! * [`IMPL_REGISTRY`] — Static slice of trait implementation blocks.
//!
//! Calling [`VM::register_all`](vm::VM::register_all) loads all registered extensions into the VM.
//!
//! # Examples
//!
//! ### Basic Expression Evaluation
//!
//! ```
//! use xcl_vm::{VM, evaluate};
//!
//! let mut vm = VM::new();
//! let result = evaluate(r#"40 + 2"#, &mut vm);
//! assert_eq!(result.as_i48(), 42);
//! ```
//!
//! ### Variable Assignments and Arithmetic
//!
//! ```
//! use xcl_vm::{VM, evaluate};
//!
//! let mut vm = VM::new();
//! let result = evaluate(
//!     r#"
//!     $base = 10;
//!     $height = 5;
//!     ($base * $height) / 2
//!     "#,
//!     &mut vm,
//! );
//! assert_eq!(result.as_i48(), 25);
//! ```
//!
//! ### Conditional Expressions
//!
//! ```
//! use xcl_vm::{VM, evaluate};
//!
//! let mut vm = VM::new();
//! let result = evaluate(
//!     r#"
//!     $score = 85;
//!     if $score >= 60 { "pass" } else { "fail" }
//!     "#,
//!     &mut vm,
//! );
//! assert_eq!(result.as_str(), Some("pass"));
//! ```
//!
//! ### Manual Pipeline Execution
//!
//! You can also drive each stage of the compilation pipeline individually:
//!
//! ```
//! use xcl_vm::{VM, lexer, parser, bytecode};
//!
//! let source = "$total = 10 * 3; $total";
//! let tokens = lexer::lex(source, "calc.xcl");
//! let ast = parser::parse(&tokens, source);
//! let code = bytecode::compile(&ast);
//!
//! let mut vm = VM::new();
//! let output = vm.run(&code);
//! assert_eq!(output.as_i48(), 30);
//! ```

// Strict clippy lints — no dead code, no duplicate code, no default allows
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::allow_attributes)]
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
#![allow(clippy::should_panic_without_expect)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_borrowed_reference)]
#![allow(clippy::single_match_else)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::use_self)]
#![allow(clippy::many_single_char_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::inline_always)]
#![allow(clippy::empty_line_after_doc_comments)]
#![allow(clippy::too_long_first_doc_paragraph)]
#![allow(clippy::duplicated_attributes)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::unnecessary_semicolon)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::derive_partial_eq_without_eq)]
#![allow(clippy::needless_continue)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::ref_as_ptr)]
#![allow(clippy::ptr_cast_constness)]
#![allow(clippy::elidable_lifetime_names)]
#![allow(clippy::or_fun_call)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::explicit_iter_loop)]
#![allow(clippy::assigning_clones)]
#![allow(clippy::branches_sharing_code)]
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate alloc;

/// Argument parsing utilities for named and positional arguments.
pub mod args;
/// Abstract syntax tree nodes representing parsed XCL code.
pub mod ast;
/// Bytecode compiler and instruction set for the XCL VM.
pub mod bytecode;
/// Error types and error grouping for XCL execution.
pub mod errors;
/// Lexer that tokenizes XCL source code into tokens.
pub mod lexer;
/// Native function registry and built-in function infrastructure.
pub mod native;
/// Parser that builds an AST from tokenized XCL source.
pub mod parser;
/// Source location spans for error reporting and debugging.
pub mod span;
/// Value types, heap objects, and native function bindings.
pub mod value;
/// Virtual machine runtime for executing XCL bytecode.
pub mod vm;
/// Well-known types and their metadata.
pub mod wellknown;

pub use args::split_named_args;
pub use bytecode::{Bytecode, Instr};
pub use errors::{Group, is_known_kind};
pub use native::{IMPL_REGISTRY, NATIVE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY};
pub use span::Span;
pub use value::{
    BuiltinImpl, DomainCtx, FuncValue, GlobalOverrideGuard, HandleKind, ImplDescriptor, ImplMethod,
    IntoValue, NamedParam, NativeArg, NativeEntry, NativeKind, RawPtr, SUBTYPE_ERROR, SUBTYPE_HANDLE,
    SUBTYPE_NONE, SUBTYPE_RESULT, SUBTYPE_USER_BASE, TraitDescriptor, TraitMethod, TypeDescriptor,
    TypeField, TypeKind, TypeVariant, VARIANT_DEFAULT, VARIANT_ERR, VARIANT_OK,
    Value, extract_arg, XclType, XclObject,
};
pub use vm::{DomainSlot, VM};
pub use wellknown::WellKnown;

/// Evaluates XCL source code using the provided virtual machine.
///
/// Runs the complete execution pipeline:
/// 1. Tokenizes `source` using [`lexer::lex`].
/// 2. Builds an AST using [`parser::parse`].
/// 3. Compiles the AST into bytecode using [`bytecode::compile`].
/// 4. Executes the bytecode on `vm` via [`VM::run`].
///
/// # Arguments
///
/// * `source` - The XCL script or expression to evaluate.
/// * `vm` - Mutable reference to the [`VM`] runtime instance.
///
/// # Returns
///
/// The evaluated [`Value`] result.
///
/// # Examples
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
