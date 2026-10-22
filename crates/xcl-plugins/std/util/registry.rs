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

/// Compile-time distributed registry of extension opcodes defined by the
/// standard plugin crate.
///
/// This slice is registered **before** [`OPCODE_REGISTRY`], so std owns its codes
/// before any third-party plugin is loaded. `#[opcode]` routes here automatically
/// when the macro expands inside `xcl-plugins` (it reads `CARGO_PKG_NAME`), so no
/// author has to think about it.
#[distributed_slice]
pub static STD_OPCODE_REGISTRY: [xcl_vm::OpcodeEntry] = [..];

/// Compile-time distributed registry of extension opcodes defined by plugins.
#[distributed_slice]
pub static OPCODE_REGISTRY: [xcl_vm::OpcodeEntry] = [..];

/// Compile-time distributed registry of cold plugin modules.
///
/// A cold module is reached through `OP_EXT` (`a` = module id, `c` = local
/// opcode) and exists for plugins whose instruction set does not fit in the 128
/// hot bytes. Prefer the `cold_module!` macro to writing entries by hand.
#[distributed_slice]
pub static COLD_MODULE_REGISTRY: [xcl_vm::ColdModule] = [..];

/// Looks up the registered name for an extension opcode, std first.
pub fn registered_opcode_name(op: u8) -> Option<&'static str> {
    STD_OPCODE_REGISTRY
        .iter()
        .chain(OPCODE_REGISTRY.iter())
        .find(|e| e.code == op)
        .map(|e| e.name)
}

/// Verifies that the module-space opcode codes are unique across every linked
/// crate, with std loaded first.
///
/// `128..=255` is a single byte space shared by all plugins: two instructions
/// with the same code cannot be distinguished at dispatch time, and the emitted
/// bytecode bakes the code in at compile time. A collision is therefore always a
/// build bug, and it is reported with both owners named.
///
/// # Panics
///
/// Panics when a code is outside `128..=255`, or when two entries claim the same
/// code.
fn validate_opcode_space(std: &[OpcodeEntry], plugins: &[OpcodeEntry]) {
    let mut owner: [Option<&'static str>; 256] = [None; 256];
    for (origin, entries) in [("std", std), ("plugin", plugins)] {
        for entry in entries {
            if !xcl_vm::runtime::opcode::layout::is_module_slot(entry.code) {
                panic!(
                    "{} opcode `{}` uses code {} — plugin opcodes must be in 128..=255; \
                     codes below 128 belong to the core (and 127 is OP_EXT)",
                    origin, entry.name, entry.code
                );
            }
            if let Some(previous) = owner[entry.code as usize] {
                panic!(
                    "opcode code {} is claimed by both `{}` and `{}` ({}); \
                     module-space codes are a single shared byte range, so they must be unique",
                    entry.code, previous, entry.name, origin
                );
            }
            owner[entry.code as usize] = Some(entry.name);
        }
    }
}

/// Registers all plugin natives, types, traits, impls, and opcodes into the VM.
///
/// Order is part of the contract: std registers first, so its opcodes and
/// natives exist before any third-party plugin is loaded.
pub fn register_all(vm: &mut VM) {
    validate_opcode_space(&STD_OPCODE_REGISTRY, &OPCODE_REGISTRY);
    vm.register_opcodes(&STD_OPCODE_REGISTRY);
    vm.register_opcodes(&OPCODE_REGISTRY);
    vm.register_cold_modules(&COLD_MODULE_REGISTRY);
    vm.register_natives(&NATIVE_REGISTRY);
    vm.register_types(&TYPE_REGISTRY);
    vm.register_traits(&TRAIT_REGISTRY);
    vm.register_impls(&IMPL_REGISTRY);
}

/// Declares a cold plugin module and registers it in [`COLD_MODULE_REGISTRY`].
///
/// The local opcode is the number carried in `OP_EXT` operand `c`.
///
/// ```ignore
/// fn op_open(vm: &mut xcl_vm::VM, _top: usize, _i: xcl_vm::Instr) -> xcl_vm::OpcodeResult {
///     xcl_vm::OpcodeResult::Next
/// }
///
/// xcl_plugins::cold_module! {
///     SQLITE = { id: 7, name: "sqlite", ops: {
///         0 => ("OP_SQLITE_OPEN", op_open),
///         1 => ("OP_SQLITE_STEP", op_step),
///     }}
/// }
/// ```
#[macro_export]
macro_rules! cold_module {
    ($static_name:ident = { id: $id:expr, name: $name:expr, ops: { $($code:expr => ($op_name:expr, $handler:path)),* $(,)? } }) => {
        #[::linkme::distributed_slice($crate::COLD_MODULE_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static $static_name: $crate::xcl_vm::ColdModule = $crate::xcl_vm::ColdModule {
            id: $id,
            name: $name,
            ops: &[$(
                $crate::xcl_vm::OpcodeEntry {
                    code: $code,
                    name: $op_name,
                    handler: $handler,
                }
            ),*],
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn handler(_vm: &mut VM, _top: usize, _instr: xcl_vm::Instr) -> xcl_vm::OpcodeResult {
        xcl_vm::OpcodeResult::Next
    }

    fn entry(code: u8, name: &'static str) -> OpcodeEntry {
        OpcodeEntry {
            code,
            name,
            handler,
        }
    }

    fn panic_message(err: Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = err.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = err.downcast_ref::<String>() {
            s.clone()
        } else {
            String::new()
        }
    }

    #[test]
    fn disjoint_codes_are_accepted() {
        let std = [entry(128, "OP_STD_A"), entry(129, "OP_STD_B")];
        let plugins = [entry(200, "OP_PLUGIN_A")];
        validate_opcode_space(&std, &plugins);
    }

    #[test]
    fn a_plugin_cannot_claim_a_std_code() {
        let std = [entry(200, "OP_STD_OWNS_200")];
        let plugins = [entry(200, "OP_PLUGIN_WANTS_200")];
        let err = catch_unwind(AssertUnwindSafe(|| validate_opcode_space(&std, &plugins)))
            .expect_err("a duplicate code must panic");
        let msg = panic_message(err);
        assert!(msg.contains("OP_STD_OWNS_200"), "{msg}");
        assert!(msg.contains("OP_PLUGIN_WANTS_200"), "{msg}");
        assert!(msg.contains("200"), "{msg}");
    }

    #[test]
    fn two_plugins_cannot_share_a_code() {
        let plugins = [entry(201, "OP_ONE"), entry(201, "OP_TWO")];
        let err = catch_unwind(AssertUnwindSafe(|| validate_opcode_space(&[], &plugins)))
            .expect_err("a duplicate code must panic");
        assert!(panic_message(err).contains("201"));
    }

    #[test]
    fn core_codes_are_rejected() {
        for code in [0u8, 49, 126, 127] {
            let std = [entry(code, "OP_BAD")];
            let err = catch_unwind(AssertUnwindSafe(|| validate_opcode_space(&std, &[])))
                .expect_err("a core code must panic");
            assert!(panic_message(err).contains(&code.to_string()));
        }
    }
}
