//! Registerable opcode extension system.
//!
//! Allows crates outside `xcl-vm` to declare and register custom opcodes
//! using the `#[xcl_opcode(...)]` macro. Extension opcodes use codes `128..=255`.
//! Core opcodes remain below 128.
//!
//! # Architecture
//!
//! - [`OPCODE_REGISTRY`] is a [`linkme::distributed_slice`] that collects
//!   [`OpcodeEntry`] instances at link time.
//! - Each entry has a code, name, and handler function.
//! - The VM populates an [`opcode_table`] from the registry during [`VM::register_all`].
//! - Unhandled opcodes in the dispatch loop fall through to the registry lookup.
//!
//! # Example
//!
//! ```ignore
//! #[xcl_opcode(name = "OP_XCL_TEST", code = 200)]
//! fn op_xcl_test(
//!     vm: &mut ::xcl_vm::VM,
//!     top: usize,
//!     instr: ::xcl_vm::Instr,
//! ) -> ::xcl_vm::OpcodeResult {
//!     vm.push_value(::xcl_vm::Value::from_i48(42));
//!     ::xcl_vm::OpcodeResult::Next
//! }
//! ```

use crate::Instr;
use crate::runtime::vm::VM;

/// Function signature for opcode handlers.
pub type OpcodeFn = fn(&mut VM, usize, Instr) -> OpcodeResult;

/// Result of an opcode handler execution.
pub enum OpcodeResult {
    /// Advance to the next instruction.
    Next,
    /// Jump to the specified instruction address.
    Jump(u32),
    /// Return from the current frame.
    Return,
}

/// A registered opcode entry.
pub struct OpcodeEntry {
    /// The opcode code (must be in range 128..=255).
    pub code: u8,
    /// Human-readable name for the opcode.
    pub name: &'static str,
    /// Handler function.
    pub handler: OpcodeFn,
}

/// Distributed slice collecting all registered extension opcodes.
#[linkme::distributed_slice]
pub static OPCODE_REGISTRY: [OpcodeEntry] = [..];

/// Looks up the registered name for an extension opcode.
///
/// Returns `None` if the opcode is not registered.
pub fn registered_opcode_name(op: u8) -> Option<&'static str> {
    OPCODE_REGISTRY
        .iter()
        .find(|e| e.code == op)
        .map(|e| e.name)
}
