//! Distributed static registry for native functions, types, and implementations.
//!
//! # Overview
//!
//! XCL exposes native Rust code to the scripting language with zero central
//! boilerplate, using [`linkme::distributed_slice`].
//!
//! Any crate linked into the program can define a native function or type and
//! register it into one of the distributed slices:
//!
//! * [`NATIVE_REGISTRY`] — native functions.
//! * [`TYPE_REGISTRY`] — custom types.
//! * [`TRAIT_REGISTRY`] — trait interface definitions.
//! * [`IMPL_REGISTRY`] — trait implementation blocks.
//! * [`OPCODE_REGISTRY`] / [`STD_OPCODE_REGISTRY`] — extension opcodes.
//! * [`PLUGIN_REGISTRY`] — plugin descriptors.
//! * [`FEATURE_REGISTRY`] — installable features the host can select.
//! * [`COLD_MODULE_REGISTRY`] — cold plugin modules reached through `OP_EXT`.
//!
//! The slices live **here**, in the engine, and nowhere else. A plugin crate
//! only appends entries, so the backend owns the extension points the way a
//! base class library owns its types. [`register_all`] scans the slices and
//! populates a VM's dispatch tables; [`register_domains`] does the same for a
//! selected subset, which is what feature installation uses.

use linkme::distributed_slice;

use crate::{ImplDescriptor, NativeEntry, TraitDescriptor, TypeDescriptor, VM};

pub use crate::{FeatureDescriptor, OpcodeEntry, OpcodeFn};

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
/// standard library.
///
/// `#[opcode]` routes here automatically when it expands inside `xcl-rt`, so no
/// author has to think about it.
#[distributed_slice]
pub static STD_OPCODE_REGISTRY: [OpcodeEntry] = [..];

/// Compile-time distributed registry of extension opcodes defined by plugins.
#[distributed_slice]
pub static OPCODE_REGISTRY: [OpcodeEntry] = [..];

/// Compile-time distributed registry of plugins.
///
/// A plugin lists the domains it publishes; the VM uses the table to answer
/// "which plugin owns this domain?" and to resolve qualified paths such as
/// `std.util.print`.
#[distributed_slice]
pub static PLUGIN_REGISTRY: [crate::PluginDescriptor] = [..];

/// Compile-time distributed registry of cold plugin modules.
///
/// A cold module is reached through `OP_EXT` (`a` = module id, `c` = local
/// opcode) and exists for plugins whose instruction set does not fit in the 128
/// hot bytes. Prefer the [`cold_module!`] macro to writing entries by hand.
#[distributed_slice]
pub static COLD_MODULE_REGISTRY: [crate::ColdModule] = [..];

/// Compile-time distributed registry of optional runtime features.
///
/// Each entry names one installable piece of the compiled library. The runtime
/// exposes the list to the host, which selects a subset at run time. A plugin
/// crate adds a feature by declaring a static in this slice; no change to the
/// engine or the launcher is needed.
#[distributed_slice]
pub static FEATURE_REGISTRY: [FeatureDescriptor] = [..];

/// Looks up the registered name for an extension opcode, std first.
pub fn registered_opcode_name(op: u8) -> Option<&'static str> {
    STD_OPCODE_REGISTRY
        .iter()
        .chain(OPCODE_REGISTRY.iter())
        .find(|entry| entry.code == op)
        .map(|entry| entry.name)
}

/// Verifies that the module-space opcode codes are unique across every linked
/// crate, with the standard library loaded first.
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
            if !crate::runtime::opcode::layout::is_module_slot(entry.code) {
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

/// Registers all natives, types, traits, impls, plugins, and opcodes into the VM.
///
/// The standard library registers first, so its opcodes and natives exist
/// before any extension.
pub fn register_all(vm: &mut VM) {
    vm.register_plugins(&PLUGIN_REGISTRY);
    validate_opcode_space(&STD_OPCODE_REGISTRY, &OPCODE_REGISTRY);
    vm.register_opcodes(&STD_OPCODE_REGISTRY);
    vm.register_opcodes(&OPCODE_REGISTRY);
    vm.register_cold_modules(&COLD_MODULE_REGISTRY);
    vm.register_natives(&NATIVE_REGISTRY);
    vm.register_types(&TYPE_REGISTRY);
    vm.register_traits(&TRAIT_REGISTRY);
    vm.register_impls(&IMPL_REGISTRY);
}

/// Registers only the parts of the compiled registry that belong to `domains`.
///
/// This is the building block for feature installation. `include_globals`
/// controls entries without a domain (a global builtin, for example).
/// Opcodes and impls are always registered: an unused opcode is harmless, and
/// impl blocks may be shared by several domains.
pub fn register_domains(vm: &mut VM, domains: &[&str], include_globals: bool) {
    let selected = |domain: Option<&'static str>| match domain {
        Some(domain) => domains.contains(&domain),
        None => include_globals,
    };

    vm.register_plugins_filtered(&PLUGIN_REGISTRY, |plugin| {
        plugin
            .domains
            .iter()
            .any(|alias| domains.contains(&alias.domain))
    });
    validate_opcode_space(&STD_OPCODE_REGISTRY, &OPCODE_REGISTRY);
    vm.register_opcodes(&STD_OPCODE_REGISTRY);
    vm.register_opcodes(&OPCODE_REGISTRY);
    vm.register_cold_modules(&COLD_MODULE_REGISTRY);
    vm.register_natives_filtered(&NATIVE_REGISTRY, |entry| selected(entry.domain));
    vm.register_types_filtered(&TYPE_REGISTRY, |entry| selected(entry.domain));
    vm.register_traits_filtered(&TRAIT_REGISTRY, |entry| selected(entry.domain));
    vm.register_impls(&IMPL_REGISTRY);
}

/// Declares a cold plugin module and registers it in [`COLD_MODULE_REGISTRY`].
///
/// The local opcode is the number carried in `OP_EXT` operand `c`.
///
/// ```ignore
/// fn op_open(vm: &mut xcl_rt::VM, _top: usize, _i: xcl_rt::Instr) -> xcl_rt::OpcodeResult {
///     xcl_rt::OpcodeResult::Next
/// }
///
/// xcl_rt::cold_module! {
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
        static $static_name: $crate::ColdModule = $crate::ColdModule {
            id: $id,
            name: $name,
            ops: &[$(
                $crate::OpcodeEntry {
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

    fn handler(_vm: &mut VM, _top: usize, _instr: crate::Instr) -> crate::OpcodeResult {
        crate::OpcodeResult::Next
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
