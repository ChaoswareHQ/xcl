//! XCL engine runtime — parse nothing, compile nothing, run bytecode.
//!
//! `xcl-rt` is the minimal library a product ships. It loads compiled bytecode
//! ([`Bytecode`]) and executes it on a stack-based [`VM`] operating on NaN-boxed
//! [`Value`] words. It does not contain a lexer, parser, compiler, or plugin
//! registry. Those live in `xcl-sdk` and `xcl-vm`.
//!
//! # Overview
//!
//! * **NaN-Boxed Values ([`Value`](runtime::value::Value))**:
//!   All runtime values fit into an 8-byte machine word.
//! * **Bump Allocation ([`bumpalo::Bump`])**:
//!   Temporary heap allocations are serviced by the VM's internal arena.
//! * **Compact 64-bit Instructions ([`Instr`](bytecode::Instr))**:
//!   Bytecode instructions pack an 8-bit opcode and operands into a single word.
//!
//! # Examples
//!
//! ```
//! use xcl_rt::{VM, Bytecode, Instr};
//! // A host compiles bytecode elsewhere and passes it here.
//! let mut vm = VM::new();
//! // vm.run(&bytecode) — executed when bytecode is available
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
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc;

// The registration macros expand to `::xcl_rt::…`, including inside this crate.
extern crate self as xcl_rt;

// Collections that resolve to `std` or `hashbrown` depending on the feature set.
#[cfg(not(feature = "std"))]
pub(crate) use hashbrown::HashMap;
#[cfg(feature = "std")]
pub(crate) use std::collections::HashMap;

/// Bytecode instruction set and the `Bytecode` container.
pub mod bytecode;
/// Distributed registry of natives, types, plugins, features, and opcodes.
pub mod registry;
/// VM execution engine: virtual machine, values, and error types.
pub mod runtime;
/// The standard library plugin (`std` / `util`): `print`, `input`.
pub mod stdlib;

pub use registry::{
    COLD_MODULE_REGISTRY, FEATURE_REGISTRY, IMPL_REGISTRY, NATIVE_REGISTRY, OPCODE_REGISTRY,
    PLUGIN_REGISTRY, STD_OPCODE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY, register_all,
    register_domains, registered_opcode_name,
};

/// Creates a VM with every linked native, type, trait, impl, and plugin
/// registered.
pub fn new_vm() -> VM {
    let mut vm = VM::new();
    register_all(&mut vm);
    vm
}

/// Installs only the standard library into an existing VM.
///
/// Use this when the VM was created directly with [`VM::new`] and the host wants
/// `print`/`input` without every other linked plugin.
pub fn install_std(vm: &mut VM) {
    register_domains(vm, &["util"], true);
}

pub use bytecode::{Bytecode, BytecodeError, ENGINE_ABI, Instr};
pub use runtime::{
    BuiltinImpl, Cap, CapSet, ColdModule, DomainAlias, DomainCtx, DomainSlot, FeatureDescriptor,
    FuncValue, GlobalOverrideGuard, HandleKind, HeapMap, HotNative, HotTable, ImplDescriptor,
    ImplMethod, IntoValue, NamedParam, NativeArg, NativeEntry, NativeKind, OpcodeEntry, OpcodeFn,
    OpcodeResult, PluginDescriptor, PrimKind, Primitive, RawPtr, SUBTYPE_ERROR, SUBTYPE_HANDLE,
    SUBTYPE_NONE, SUBTYPE_RANGE, SUBTYPE_RESULT, SUBTYPE_USER_BASE, TraitDescriptor, TraitMethod,
    TypeDescriptor, TypeField, TypeKind, TypeVariant, VARIANT_DEFAULT, VARIANT_ERR, VARIANT_OK, VM,
    Value, XclType, capability_of_name, extract_arg, is_primitive_of, opcode_name,
    primitive_of_name, primitive_of_value,
};
