//! The `util` library — the only plugin domain that ships today.
//!
//! `xcl-std` is one crate: `xcl-std/std/util/` is a folder chain, and this file
//! is the library at the bottom of it. It holds the two things the language
//! itself assumes are present: `print` (output) and `input`.
//!
//! `#[xcl_plugin]` on [`Util`] declares the registry anchors, the plugin
//! descriptor, and the installable `std` feature in one place. The modules
//! then only attach methods with `#[xcl_impl]` / `#[xcl_fn]`; nothing else
//! needs to know about `linkme` or the slices.
//!
//! # Usage
//!
//! ```rust
//! let mut vm = xcl_std::new_vm();
//! let result = xcl_std::evaluate("print \"hello\"", &mut vm);
//! ```

extern crate self as xcl_std;

mod input;
mod manifest;
mod print;

use xcl_macros::xcl_plugin;
use xcl_vm::{VM, Value};

pub use manifest::{MANIFEST_ABI, manifest_toml};

pub use xcl_vm;
pub use xcl_vm::{FeatureDescriptor, OpcodeEntry, OpcodeFn};

/// The `std` plugin and its `util` domain.
///
/// The macro derives `Xcl`, declares the distributed-slice anchors
/// (`NATIVE_REGISTRY`, `TYPE_REGISTRY`, …), registers the plugin in
/// `PLUGIN_REGISTRY`, and registers one default installable feature in
/// `FEATURE_REGISTRY` named after the crate (`xcl-std` → `std`). The marker
/// type name gives the domain (`Util` → `util`).
#[xcl_plugin(default, summary = "standard library — print, input")]
pub struct Util;

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

// ── Host API ─────────────────────────────────────────────────────────────────

/// Create a new VM with all registered types and builtins.
pub fn new_vm() -> VM {
    let mut vm = VM::new();
    register_all(&mut vm);
    vm
}

/// Evaluate XCL source code using the provided VM.
pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    xcl_vm::evaluate(source, vm)
}

/// Evaluates `source`, returning every syntax error instead of the first.
pub fn evaluate_recover(
    name: &str,
    source: &str,
    vm: &mut VM,
) -> core::result::Result<Value, Vec<xcl_vm::ParseError>> {
    xcl_vm::evaluate_recover(name, source, vm)
}

/// Evaluates an already-parsed AST.
///
/// Hosts use this after assembling a program from several files, such as
/// resolving `binding = module.symbol` imports.
pub fn evaluate_ast(ast: &[xcl_vm::AstNode], vm: &mut VM) -> Value {
    let bytecode = xcl_vm::bytecode::compile_hot(ast, vm.hot_table());
    vm.run(&bytecode)
}

/// Evaluate XCL source code in a fresh VM.
pub fn evaluate_new(source: &str) -> Value {
    let mut vm = new_vm();
    evaluate(source, &mut vm)
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
/// xcl_std::cold_module! {
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

// ── Tests ────────────────────────────────────────────────────────────────────

/// Macro-level capability tests.
///
/// These prove that `#[xcl(caps = "...")]` and `#[xcl_fn(flags = "...")]`
/// actually reach the registry as bits, rather than being parsed and dropped.
#[cfg(test)]
mod caps_macro_tests {
    use xcl_macros::{Xcl, xcl_fn};
    use xcl_vm::Cap;

    #[derive(Xcl)]
    #[xcl(kind = "Record", caps = "Len, Contains")]
    #[allow(dead_code)]
    struct CappedRecord {
        id: i64,
    }

    #[xcl_fn(name = "macro_test.pure_fn", flags = "PURE | DETERMINISTIC")]
    fn pure_fn(value: xcl_vm::Value) -> xcl_vm::Value {
        value
    }

    #[xcl_fn(name = "macro_test.effectful_fn")]
    fn effectful_fn(value: xcl_vm::Value) -> xcl_vm::Value {
        value
    }

    #[test]
    fn derive_records_declared_capabilities() {
        let desc = super::TYPE_REGISTRY
            .iter()
            .find(|d| d.name == "CappedRecord")
            .expect("CappedRecord is registered");
        assert!(desc.caps.has(Cap::Len));
        assert!(desc.caps.has(Cap::Contains));
        assert!(!desc.caps.has(Cap::Numeric));
    }

    #[test]
    fn native_flags_become_capability_bits() {
        let pure_entry = super::NATIVE_REGISTRY
            .iter()
            .find(|e| e.method == "pure_fn")
            .expect("pure_fn is registered");
        assert!(pure_entry.caps.has(Cap::Pure));
        assert!(pure_entry.caps.has(Cap::Deterministic));

        let plain_entry = super::NATIVE_REGISTRY
            .iter()
            .find(|e| e.method == "effectful_fn")
            .expect("effectful_fn is registered");
        assert!(plain_entry.caps.is_empty());
    }
}

#[cfg(test)]
mod registry_tests {
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

#[cfg(test)]
mod plugin_tests {
    use super::{evaluate_new, new_vm};

    #[test]
    fn a_hot_native_compiles_to_one_call_instruction() {
        let vm = new_vm();
        for spelling in ["print", "util.print", "std.util.print"] {
            assert!(
                vm.hot_table().slot(spelling).is_some(),
                "{spelling} should have a hot slot"
            );
        }

        let src = "print \"hi\"";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);
        let bytecode = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        assert!(
            bytecode
                .instructions
                .iter()
                .any(|i| i.op() == xcl_vm::bytecode::OP_CALL_HOT),
            "a registered hot native must not need a name lookup"
        );
    }

    /// The hot encoding must be a drop-in replacement for the named one: same
    /// instruction count, same result, one word different.
    #[test]
    fn the_hot_encoding_differs_by_exactly_one_word() {
        let mut vm = new_vm();
        let src = "util.write \"\"";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);

        let hot = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        let named = xcl_vm::bytecode::compile(&ast);

        assert_eq!(
            hot.instructions.len(),
            named.instructions.len(),
            "the hot form must not add or remove instructions"
        );
        let differing: Vec<(u64, u64)> = hot
            .instructions
            .iter()
            .zip(named.instructions.iter())
            .filter(|(a, b)| a.0 != b.0)
            .map(|(a, b)| (a.0, b.0))
            .collect();
        assert_eq!(differing.len(), 1, "only the call site should change");
        let (hot_word, named_word) = differing[0];
        assert_eq!(hot_word as u8, xcl_vm::bytecode::OP_CALL_HOT);
        assert_eq!(named_word as u8, xcl_vm::bytecode::OP_CALL_NAME);

        assert_eq!(vm.run(&hot).as_i48(), 0);
        assert_eq!(vm.run(&named).as_i48(), 0);
    }

    #[test]
    fn hot_and_named_calls_reach_the_same_function() {
        // One VM, one AST, two compilations: the hot encoding and the name
        // encoding must agree on the result.
        let mut vm = new_vm();
        let src = "util.write \"\"";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);

        let hot = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        let named = xcl_vm::bytecode::compile(&ast);

        assert_eq!(vm.run(&hot).as_i48(), 0);
        assert_eq!(vm.run(&named).as_i48(), 0);
    }

    #[test]
    fn the_vm_knows_the_std_plugin() {
        let vm = new_vm();
        assert!(vm.plugins().iter().any(|p| p.name == "std"));
        assert_eq!(vm.plugin_of("util"), Some("std"));
        assert_eq!(vm.plugin_of("std.util"), Some("std"));
        assert_eq!(vm.plugin_of("math"), None);
    }

    #[test]
    fn util_is_the_only_std_domain() {
        let vm = new_vm();
        assert_eq!(vm.plugin_of("util"), Some("std"));
        for gone in ["result", "metadata", "rule", "defer"] {
            assert_eq!(vm.plugin_of(gone), None, "{gone} must be gone");
        }
    }

    #[test]
    fn print_and_input_are_registered() {
        let vm = new_vm();
        for name in ["print", "util.print", "util.input", "util.readline"] {
            assert!(
                vm.hot_table().slot(name).is_some(),
                "{name} should be callable"
            );
        }
        assert_eq!(evaluate_new("util.write \"\"").as_i48(), 0);
    }
}
