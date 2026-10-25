//! XCL toolchain — the frontend: source in, bytecode out.
//!
//! This crate is the development half of XCL. It owns the lexer, parser, AST,
//! bytecode compiler, optimizer, linter, import resolver, `plugin.toml` reader,
//! disassembler, and the host API the `xcl` launcher drives. Execution belongs
//! to [`xcl_rt`], which this crate links; a product that ships precompiled
//! `.xclb` files does not need this library at all.
//!
//! ```text
//! source ──toolchain──► Bytecode ──engine──► output
//!    ▲                      │
//!    └────── .xclb ─────────┘
//! ```
//!
//! # Feature flags
//!
//! * `std` (default) — forwards to the engine; no_std builds are not supported.
//! * `host` (default) — file imports, `plugin.toml`, the C ABI, and mimalloc.
//!   Disable it for `wasm32` or for a Rust host that only wants the compiler.
//!
//! # Example
//!
//! ```
//! use xcl_toolchain::{VM, evaluate};
//!
//! let mut vm = VM::new();
//! let result = evaluate(r#"10 * 3"#, &mut vm);
//! assert_eq!(result.as_i48(), 30);
//! ```

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

/// Bytecode codegen and peephole optimizer.
pub mod bytecode;

pub(crate) mod debug;

pub(crate) use std::collections::{HashMap, HashSet};
/// Lexer, parser, AST nodes, and source spans.
pub mod compiler;
/// Bytecode disassembler.
pub mod decompile;
/// Static analysis: diagnostics, lints, and the report format.
pub mod lint;
/// TOML projection of the compiled registry.
pub mod manifest;
/// Bytecode optimization passes: dead store elimination, constant propagation,
/// LICM, loop rotation and unrolling.
pub mod optimizer;

/// Host API and C ABI (`xcl_toolchain_*`), including file imports and
/// `plugin.toml`.
#[cfg(feature = "host")]
pub mod host;

#[cfg(feature = "host")]
mod imports;
#[cfg(feature = "host")]
mod plugin_config;

#[cfg(test)]
mod vm_tests;

/// Re-export the engine runtime under the module path the compiler uses.
pub mod runtime {
    pub use xcl_rt::runtime::*;
}

pub use compiler::AstNode;
pub use compiler::lexer;
pub use compiler::parser;
pub use compiler::span::Span;
pub use compiler::{PARSE_ERROR_CODES, ParseError, ParseOutput, parse_recover};
pub use xcl_rt::{
    BuiltinImpl, Bytecode, Cap, CapSet, ColdModule, DomainAlias, DomainCtx, DomainSlot,
    FeatureDescriptor, FuncValue, GlobalOverrideGuard, HandleKind, HeapMap, HotNative, HotTable,
    ImplDescriptor, ImplMethod, Instr, IntoValue, NamedParam, NativeArg, NativeEntry, NativeKind,
    OpcodeEntry, OpcodeFn, OpcodeResult, PluginDescriptor, PrimKind, Primitive, RawPtr,
    SUBTYPE_ERROR, SUBTYPE_HANDLE, SUBTYPE_NONE, SUBTYPE_RANGE, SUBTYPE_RESULT, SUBTYPE_USER_BASE,
    TraitDescriptor, TraitMethod, TypeDescriptor, TypeField, TypeKind, TypeVariant,
    VARIANT_DEFAULT, VARIANT_ERR, VARIANT_OK, VM, Value, XclType, capability_of_name, extract_arg,
    is_primitive_of, opcode_name, primitive_of_name, primitive_of_value,
};

/// Evaluates XCL source code using the provided virtual machine.
///
/// Runs the complete pipeline: lex, parse, compile with the VM's hot slots, run.
///
/// Compiling and running against the same VM is what makes the hot-slot binding
/// sound: the slots are numbered by the VM, so the bytecode is not portable to
/// another one. Hosts that cache bytecode across VMs should call
/// [`bytecode::compile`] and [`VM::run`] themselves.
///
/// # Examples
///
/// ```
/// use xcl_toolchain::{VM, evaluate};
///
/// let mut vm = VM::new();
/// let result = evaluate(r#"10 * 3"#, &mut vm);
/// assert_eq!(result.as_i48(), 30);
/// ```
pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    let tokens = compiler::lexer::lex(source, "<input>");
    let ast = compiler::parser::parse(&tokens, source);
    let bytecode = bytecode::compile_hot(&ast, vm.hot_table());
    vm.run(&bytecode)
}

/// Evaluates `source` as if it came from `name`, reporting **every** syntax
/// error instead of stopping at the first one.
///
/// # Errors
///
/// Returns every [`ParseError`] the parser recovered from, in source order. The
/// program is not executed when any error is present.
///
/// # Examples
///
/// ```
/// use xcl_toolchain::{evaluate_recover, VM};
///
/// let mut vm = VM::new();
/// let errors = evaluate_recover("script.xcl", ")", &mut vm).unwrap_err();
/// assert!(!errors.is_empty());
/// assert_eq!(errors[0].span.file, "script.xcl");
/// ```
pub fn evaluate_recover(
    name: &str,
    source: &str,
    vm: &mut VM,
) -> Result<Value, Vec<compiler::parser::ParseError>> {
    let tokens = compiler::lexer::lex(source, name);
    let out = compiler::parser::parse_recover(&tokens, source);
    if !out.errors.is_empty() {
        return Err(out.errors);
    }
    let bytecode = bytecode::compile_hot(&out.ast, vm.hot_table());
    Ok(vm.run(&bytecode))
}

/// Compiles an already-parsed program against `vm`'s hot slots and runs it.
///
/// This is the entry point for hosts that resolve imports themselves: the AST is
/// assembled across files, then compiled once with the same VM that will run it.
pub fn evaluate_ast(ast: &[AstNode], vm: &mut VM) -> Value {
    let bytecode = bytecode::compile_hot(ast, vm.hot_table());
    vm.run(&bytecode)
}
