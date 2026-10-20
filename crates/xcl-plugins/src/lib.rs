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

mod wellknown;

#[cfg(test)]
mod opcode_test;

pub use args::split_named_args;
pub use errors::{Group, is_known_kind};
pub use registry::{
    IMPL_REGISTRY, NATIVE_REGISTRY, OPCODE_REGISTRY, OpcodeEntry, OpcodeFn, TRAIT_REGISTRY,
    TYPE_REGISTRY, register_all, registered_opcode_name,
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
    xcl_vm::evaluate(source, vm)
}

/// Evaluate XCL source code in a fresh VM.
pub fn evaluate_new(source: &str) -> Value {
    let mut vm = new_vm();
    evaluate(source, &mut vm)
}
