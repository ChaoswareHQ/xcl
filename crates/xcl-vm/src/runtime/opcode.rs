//! Registerable opcode extension system.
//!
//! Allows crates outside `xcl-vm` to declare and register custom opcodes
//! using the `#[xcl_opcode(...)]` macro. Extension opcodes use codes `128..=255`.
//! Core opcodes remain below 128.
//!
//! # Architecture
//!
//! - The `xcl-plugins` crate owns the [`OpcodeEntry`] distributed slice.
//! - Each entry has a code, name, and handler function.
//! - The VM populates an opcode table from the registry during registration.
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
    /// Push the pending frame onto the frame stack.
    PushFrame,
    /// The current frame now runs different bytecode (tail call).
    ///
    /// The dispatch loop must re-read the frame's code pointer and start at
    /// instruction 0. A plain `Jump(0)` is not enough: the loop caches the
    /// instruction slice, and a tail call can switch to a different
    /// [`crate::Bytecode`].
    TailCall,
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

/// Returns the human-readable name for any core opcode.
///
/// Extension opcode names are resolved via `xcl_plugins::registered_opcode_name`.
pub fn opcode_name(op: u8) -> Option<&'static str> {
    crate::runtime::vm::ops::opcode_name_core(op)
}

/// Operand values for the `c` field of `OP_SPECIAL_VAR`.
pub mod special_var_kind {
    /// `$0` — script name.
    pub const SCRIPT_NAME: u8 = 0;
    /// `$1..$N` — positional argument (`a` holds the index).
    pub const POSITIONAL: u8 = 1;
    /// `$#` — argument count.
    pub const ARG_COUNT: u8 = 2;
    /// `$@` — argument array.
    pub const ARGS_ARRAY: u8 = 3;
    /// `$*` — arguments joined by spaces.
    pub const ARGS_JOINED: u8 = 4;
}
