//! Distributed static registry for native functions, types, and implementations.
//!
//! # Overview
//!
//! XCL allows native Rust functions and types to be exposed to the scripting
//! language with zero central boilerplate using [`linkme::distributed_slice`].
//!
//! Any crate or plugin can define a native function or type and register it
//! into one of the three distributed slices:
//!
//! * [`NATIVE_REGISTRY`] - Static list of native functions (pure functions or context-aware methods).
//! * [`TYPE_REGISTRY`] - Static list of custom types and record/sum definitions.
//! * [`IMPL_REGISTRY`] - Static list of trait implementation blocks for types.
//!
//! When a [`crate::vm::VM`] starts up, calling [`crate::vm::VM::register_all`]
//! scans these slices and populates the VM's internal dispatch tables.
//!
//! # Examples
//!
//! Inspecting registered native entries:
//!
//! ```
//! use xcl_vm::native::NATIVE_REGISTRY;
//!
//! // Iterate over all statically registered native functions:
//! for entry in NATIVE_REGISTRY.iter() {
//!     let domain = entry.domain.unwrap_or("global");
//!     println!("Found native method: {}.{}", domain, entry.method);
//! }
//! ```

pub use crate::value::{
    ImplDescriptor, ImplMethod, NamedParam, NativeEntry, NativeKind, TypeDescriptor, TypeField,
    TypeKind, TypeVariant,
};

use linkme::distributed_slice;

/// Compile-time distributed registry of native XCL functions.
///
/// Native functions decorated with registration macros (or explicitly added via
/// `#[linkme::distributed_slice(NATIVE_REGISTRY)]`) are placed into this slice
/// by the linker.
///
/// The VM loads these entries during [`crate::vm::VM::register_all_natives`].
///
/// # Examples
///
/// ```
/// use xcl_vm::native::NATIVE_REGISTRY;
///
/// let native_count = NATIVE_REGISTRY.len();
/// println!("Total registered native functions: {}", native_count);
/// ```
#[distributed_slice]
pub static NATIVE_REGISTRY: [NativeEntry] = [..];

/// Compile-time distributed registry of custom XCL types.
///
/// Records type metadata, field definitions, and variant structures registered
/// by core domains or third-party plugins.
///
/// The VM loads these entries during [`crate::vm::VM::register_all_types`].
///
/// # Examples
///
/// ```
/// use xcl_vm::native::TYPE_REGISTRY;
///
/// for ty in TYPE_REGISTRY.iter() {
///     println!("Registered type: {}", ty.name);
/// }
/// ```
#[distributed_slice]
pub static TYPE_REGISTRY: [TypeDescriptor] = [..];

/// Compile-time distributed registry of trait implementation blocks.
///
/// Maps methods to types and traits, allowing XCL to dispatch trait methods
/// on native or script types.
///
/// # Examples
///
/// ```
/// use xcl_vm::native::IMPL_REGISTRY;
///
/// for imp in IMPL_REGISTRY.iter() {
///     println!("Type {} implements trait {}", imp.type_name, imp.trait_name);
/// }
/// ```
#[distributed_slice]
pub static IMPL_REGISTRY: [ImplDescriptor] = [..];
