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
    STD_OPCODE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY, register_all, registered_opcode_name,
};
pub use result::Result;
pub use rule::Rule;
pub use xcl_vm;

use xcl_vm::{VM, Value};

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
