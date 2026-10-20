//! Distributed static registry for native functions, types, and implementations.
//!
//! # Overview
//!
//! XCL allows native Rust functions and types to be exposed to the scripting
//! language with zero central boilerplate using [`linkme::distributed_slice`].
//!
//! Any crate or plugin can define a native function or type and register it
//! into one of the distributed slices:
//!
//! * [`NATIVE_REGISTRY`] - Static list of native functions.
//! * [`TYPE_REGISTRY`] - Static list of custom types.
//! * [`TRAIT_REGISTRY`] - Static list of trait interface definitions.
//! * [`IMPL_REGISTRY`] - Static list of trait implementation blocks.
//! * [`OPCODE_REGISTRY`] - Static list of extension opcodes.
//!
//! When a [`xcl_vm::VM`] starts up, calling [`register_all`] scans these slices
//! and populates the VM's internal dispatch tables.

use xcl_vm::{ImplDescriptor, NativeEntry, TraitDescriptor, TypeDescriptor, VM};

pub use xcl_vm::{OpcodeEntry, OpcodeFn};

use linkme::distributed_slice;

/// Compile-time distributed registry of native XCL functions.
#[distributed_slice]
pub static NATIVE_REGISTRY: [NativeEntry] = [..];

/// Compile-time distributed registry of custom XCL types.
#[distributed_slice]
pub static TYPE_REGISTRY: [TypeDescriptor] = [..];

/// Compile-time distributed registry of trait interface definitions.
#[distributed_slice]
pub static TRAIT_REGISTRY: [TraitDescriptor] = [..];

/// Compile-time distributed registry of trait implementation blocks.
#[distributed_slice]
pub static IMPL_REGISTRY: [ImplDescriptor] = [..];

/// Compile-time distributed registry of extension opcodes.
#[distributed_slice]
pub static OPCODE_REGISTRY: [xcl_vm::OpcodeEntry] = [..];

/// Looks up the registered name for an extension opcode.
pub fn registered_opcode_name(op: u8) -> Option<&'static str> {
    OPCODE_REGISTRY
        .iter()
        .find(|e| e.code == op)
        .map(|e| e.name)
}

/// Registers all plugin natives, types, traits, impls, and opcodes into the VM.
pub fn register_all(vm: &mut VM) {
    vm.register_opcodes(&xcl_plugins::OPCODE_REGISTRY);
    vm.register_natives(&NATIVE_REGISTRY);
    vm.register_types(&TYPE_REGISTRY);
    vm.register_traits(&TRAIT_REGISTRY);
    vm.register_impls(&IMPL_REGISTRY);
}
