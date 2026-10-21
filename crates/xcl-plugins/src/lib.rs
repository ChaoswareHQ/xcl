//! XCL Plugin Library
//!
//! This crate provides the foundation for XCL plugins using `#[derive(Xcl)]` and `impl` blocks.
//!
//! # Usage
//!
//! ```rust
//! use xcl_vm::VM;
//!
//! let mut vm = xcl_plugins::new_vm();
//! let result = xcl_plugins::evaluate("print \"hello\"", &mut vm);
//! ```

extern crate self as xcl_plugins;

pub mod args;
#[cfg(feature = "encoding")]
mod encoding;
pub mod errors;
#[cfg(feature = "fs")]
mod fs;
#[cfg(feature = "io")]
mod io;
#[cfg(feature = "math")]
mod math;
#[cfg(feature = "net")]
mod net;
mod registry;
pub mod util;

mod wellknown;

#[cfg(test)]
mod opcode_test;

pub use args::split_named_args;
pub use errors::{Group, is_known_kind};
pub use registry::{
    COLD_MODULE_REGISTRY, IMPL_REGISTRY, NATIVE_REGISTRY, OPCODE_REGISTRY, OpcodeEntry, OpcodeFn,
    STD_OPCODE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY, register_all, registered_opcode_name,
};
pub use wellknown::WellKnown;
pub use xcl_vm;

use xcl_vm::{VM, Value};

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
    util::install_metadata(source);
    xcl_vm::evaluate(source, vm)
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
