//! The `util` library — the only plugin domain that ships today.
//!
//! `xcl-plugins` is a container: `xcl-plugins/std/util/` is a folder chain, and
//! this file is the library. It holds what the language itself assumes is
//! present: `print`, `input`, `defer`, `panic`, `metadata`, `Result`, and the
//! print/input policy gate.
//!
//! Every module attaches methods to the [`Util`] marker with `#[xcl_impl]`, and
//! registration is distributed (`linkme`), so this file only declares modules.
//! Adding a method means adding it to the matching module; nothing here changes.
//!
//! # Usage
//!
//! ```rust
//! let mut vm = xcl_plugins::new_vm();
//! let result = xcl_plugins::evaluate("print \"hello\"", &mut vm);
//! ```

extern crate self as xcl_plugins;

pub mod errors;
mod registry;

mod defer;
mod input;
mod metadata;
mod panic;
mod print;
mod result;
mod rule;

use xcl_macros::Xcl;

pub use errors::{Group, is_known_kind};
pub use metadata::Metadata;
pub(crate) use metadata::install as install_metadata;
pub use registry::{
    COLD_MODULE_REGISTRY, IMPL_REGISTRY, NATIVE_REGISTRY, OPCODE_REGISTRY, OpcodeEntry, OpcodeFn,
    PLUGIN_REGISTRY, STD_OPCODE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY, register_all,
    registered_opcode_name,
};
pub use result::Result;
pub use rule::Rule;
pub use xcl_vm;

use xcl_vm::{VM, Value};

/// The plugin this library is: `std`, publishing its domains under `std.*`.
///
/// The flat key stays `util` (from the marker type), so `util.print` and the
/// bare `print` are unchanged; `std.util.print` resolves through the alias.
#[::linkme::distributed_slice(PLUGIN_REGISTRY)]
#[allow(non_upper_case_globals)]
static STD: xcl_vm::PluginDescriptor = xcl_vm::PluginDescriptor {
    name: "std",
    summary: "The XCL standard library: print, input, defer, panic, metadata, Result, policy.",
    domains: &[
        xcl_vm::DomainAlias {
            path: "std.util",
            domain: "util",
        },
        xcl_vm::DomainAlias {
            path: "std.metadata",
            domain: "metadata",
        },
        xcl_vm::DomainAlias {
            path: "std.result",
            domain: "result",
        },
        xcl_vm::DomainAlias {
            path: "std.rule",
            domain: "rule",
        },
    ],
};

/// Marker type for the `util` domain.
///
/// It carries no data; it exists so `#[derive(Xcl)]` registers the domain and
/// `#[xcl_impl]` can attach methods to it from several modules.
#[derive(Xcl)]
#[xcl(domain = "util")]
pub struct Util;

/// Create a new VM with all registered types and builtins.
pub fn new_vm() -> VM {
    let mut vm = VM::new();
    register_all(&mut vm);
    vm
}

/// Evaluate XCL source code using the provided VM.
pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    // Pick up the `# key: value` header before the script can call
    // `util.metadata`.
    install_metadata(source);
    xcl_vm::evaluate(source, vm)
}

/// Evaluates `source`, returning every syntax error instead of the first.
///
/// The metadata header is installed exactly as [`evaluate`] does, so a host can
/// swap the two calls without changing behaviour beyond error reporting.
pub fn evaluate_recover(
    name: &str,
    source: &str,
    vm: &mut VM,
) -> core::result::Result<Value, Vec<xcl_vm::ParseError>> {
    install_metadata(source);
    xcl_vm::evaluate_recover(name, source, vm)
}

/// Evaluate XCL source code in a fresh VM.
pub fn evaluate_new(source: &str) -> Value {
    let mut vm = new_vm();
    evaluate(source, &mut vm)
}

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
mod plugin_tests {
    use super::{evaluate_new, new_vm};

    #[test]
    fn a_hot_native_compiles_to_one_call_instruction() {
        let vm = new_vm();
        for spelling in ["print", "util.print", "std.util.print", "result.is_ok"] {
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
        let src = "$r = util.ok 1\n$s = 0\nif result.is_ok $r { $s = 1 }\n$s";
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

        assert_eq!(vm.run(&hot).as_i48(), 1);
        assert_eq!(vm.run(&named).as_i48(), 1);
    }

    /// Where the loop header sits, for the hoisting tests below.
    fn loop_start(bc: &xcl_vm::Bytecode) -> usize {
        bc.instructions
            .iter()
            .position(|i| {
                i.op() == xcl_vm::bytecode::OP_WHILE_LT_INT
                    || i.op() == xcl_vm::bytecode::OP_WHILE_LT_INT_WIDE
            })
            .expect("the program has a loop")
    }

    fn first_hot_call(bc: &xcl_vm::Bytecode) -> usize {
        bc.instructions
            .iter()
            .position(|i| i.op() == xcl_vm::bytecode::OP_CALL_HOT)
            .expect("the program makes a hot call")
    }

    #[test]
    fn an_invariant_hot_call_runs_once_before_the_loop() {
        let mut vm = new_vm();
        let src = "$r = util.ok 1
$i = 0
$s = 0
while $i < 4 {
  if result.is_ok $r { $s = $s + 1 }
  $i = $i + 1
}
$s";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);
        let bc = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        assert!(
            first_hot_call(&bc) < loop_start(&bc),
            "a pure call whose arguments never change must be hoisted"
        );
        assert_eq!(vm.run(&bc).as_i48(), 4);
    }

    #[test]
    fn a_hot_call_with_a_changing_argument_stays_in_the_loop() {
        let mut vm = new_vm();
        let src = "$i = 0
$s = 0
while $i < 4 {
  if result.is_ok (util.ok $i) { $s = $s + 1 }
  $i = $i + 1
}
$s";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);
        let bc = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        assert!(
            first_hot_call(&bc) > loop_start(&bc),
            "an argument the loop writes makes the call variant"
        );
        assert_eq!(vm.run(&bc).as_i48(), 4);
    }

    #[test]
    fn a_side_effecting_hot_call_is_never_hoisted() {
        // `print` is hot but not pure, so it has to run every iteration.
        let mut vm = new_vm();
        let src = "$i = 0
while $i < 2 {
  print \"\"
  $i = $i + 1
}
$i";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);
        let bc = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        assert!(first_hot_call(&bc) > loop_start(&bc));
        assert_eq!(vm.run(&bc).as_i48(), 2);
    }

    #[test]
    fn hot_and_named_calls_reach_the_same_function() {
        // One VM, one AST, two compilations: the hot encoding and the name
        // encoding must agree on the result.
        let mut vm = new_vm();
        let src = "util.is_ok (util.ok 1)";
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);

        let hot = xcl_vm::bytecode::compile_hot(&ast, vm.hot_table());
        assert!(vm.run(&hot).as_bool());

        let named = xcl_vm::bytecode::compile(&ast);
        assert!(vm.run(&named).as_bool());
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
    fn the_plugin_owns_every_std_domain() {
        let vm = new_vm();
        for domain in ["util", "metadata", "result", "rule"] {
            assert_eq!(vm.plugin_of(domain), Some("std"), "{domain}");
            assert_eq!(vm.plugin_of(&format!("std.{domain}")), Some("std"));
        }
    }

    #[test]
    fn qualified_path_reaches_the_same_method() {
        let flat = evaluate_new("util.is_ok (util.ok 1)");
        let qualified = evaluate_new("std.util.is_ok (std.util.ok 2)");
        assert!(flat.as_bool(), "flat domain must work");
        assert!(qualified.as_bool(), "`std.util.*` must alias to `util.*`");

        // A dotted method under a qualified module path: `std.util` resolves to
        // `util`, leaving `metadata.all` as the method.
        let header = evaluate_new(
            "# name: demo
std.util.metadata.all",
        );
        assert!(
            header.as_map().is_some(),
            "metadata.all returns the header map"
        );
    }

    #[test]
    fn a_sibling_domain_qualifies_too() {
        let named = evaluate_new(
            "# name: demo
std.metadata.has \"name\"",
        );
        assert!(
            named.as_bool(),
            "`std.metadata.*` must alias to `metadata.*`"
        );
        let result = evaluate_new("std.result.is_ok (util.ok 3)");
        assert!(result.as_bool());
    }
}
