//! Registerable opcode extension system.
//!
//! Allows crates outside `xcl-vm` to declare and register custom opcodes
//! using the `#[opcode(...)]` macro. Extension opcodes use codes `128..=255`.
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
//! #[opcode(name = "OP_XCL_TEST", code = 200)]
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

/// A cold plugin module reached through `OP_EXT`.
///
/// The hot space (`128..=255`) is one instruction per byte, which caps extensions
/// at 128 instructions in total. A plugin that needs more declares a cold module
/// instead: `OP_EXT` names the module in operand `a`, the module's own opcode
/// travels in `c`, and `b` stays a 16-bit operand.
///
/// Cold instructions cost one extra lookup over hot bytes, so they are for code
/// that runs rarely or that would otherwise not fit at all.
pub struct ColdModule {
    /// Module id, named by `OP_EXT` operand `a`. Must be unique across all crates.
    pub id: u32,
    /// Human-readable module name, used in diagnostics.
    pub name: &'static str,
    /// Instructions in this module. `OpcodeEntry::code` is the local opcode
    /// (`0..=255`, carried in `c`).
    pub ops: &'static [OpcodeEntry],
}

/// Returns the human-readable name for any core opcode.
///
/// Extension opcode names are resolved via `xcl_plugins::registered_opcode_name`.
pub fn opcode_name(op: u8) -> Option<&'static str> {
    crate::runtime::vm::ops::opcode_name_core(op)
}

/// Operand values for the `c` field of the fused field-update opcodes
/// (`OP_FIELD_INT`, `OP_FIELD_LOCAL`, `OP_FIELD_FIELD`).
pub mod field_op {
    /// `old + rhs`.
    pub const ADD: u8 = 0;
    /// `old - rhs`.
    pub const SUB: u8 = 1;
    /// `old * rhs`.
    pub const MUL: u8 = 2;
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
    /// `$self` — receiver of the current call.
    pub const SELF: u8 = 5;
    /// `$super` — parent record of `$self`.
    pub const SUPER: u8 = 6;
}

/// Layout of the 8-bit opcode space.
///
/// The space is partitioned so every code belongs to exactly one owner, and the
/// unassigned core codes are listed explicitly rather than left implicit.
///
/// ```text
///   0 ..= 126   core opcodes (see `runtime/vm/ops.rs`)
///        127    OP_EXT       cold modules: `a` = module id, `c` = local opcode
/// 128 ..= 255   hot extension opcodes, one byte per instruction
/// ```
///
/// The split is a single bit, so classification needs no comparison table:
///
/// ```text
///   op & 0b1000_0000 == 0   → core space (includes OP_EXT)
///   op & 0b1000_0000 != 0   → module space, index = op & 0b0111_1111
/// ```
///
/// Within the core range, codes are either:
/// * **defined** by a core handler,
/// * **[`layout::OP_EXT`]**, the escape reserved for cold plugin modules,
/// * **growth** — listed in [`layout::CORE_GROWTH`] and free for the next core
///   superinstruction.
///
/// # Module space (`128..=255`)
///
/// A module-space byte *is* the instruction: dispatch masks it with
/// [`layout::MODULE_INDEX_MASK`] and indexes the 128-entry `opcode_table`
/// directly, with no second lookup, and `a`/`b`/`c` stay available as the
/// handler's operands. The range is shared by every linked crate, so codes must
/// be unique across the whole program — `xcl_plugins::register_all` rejects
/// duplicates and names both owners.
///
/// **Std loads first.** `#[opcode]` routes an entry into
/// `STD_OPCODE_REGISTRY` when it expands inside `xcl-plugins` and into
/// `OPCODE_REGISTRY` otherwise; std registers first, so its codes are never
/// taken by a plugin. A plugin that collides with std fails at registration
/// instead of silently replacing an instruction.
///
/// `OP_EXT` is the cold tier: operand `a` names a module, `c` names the
/// instruction inside it, and `b` stays a 16-bit operand. Modules are declared
/// with `cold_module!` and registered by `xcl_plugins::register_all`. A cold
/// instruction costs one extra lookup over a hot byte, so it is for plugins whose
/// instruction set does not fit in 128 bytes — or that run rarely.
///
/// An unknown cold module or opcode is a no-op, matching an unclaimed hot byte.
///
/// `every_core_code_has_a_defined_owner` below enforces the inventory, so an
/// unaccounted code cannot appear silently.
pub mod layout {
    /// First code available to core opcodes.
    pub const CORE_FIRST: u8 = 0;
    /// Last code owned by the core range.
    pub const CORE_LAST: u8 = 127;
    /// Escape opcode. Operand `a` holds a 32-bit cold-module id.
    ///
    /// Kept at the last core code so the boundary reads naturally:
    /// everything below it is a real core instruction.
    pub const OP_EXT: u8 = 127;
    /// Selects the module space: set for codes `128..=255`.
    pub const MODULE_SPACE_MASK: u8 = 0b1000_0000;
    /// Module slot index inside the module space.
    pub const MODULE_INDEX_MASK: u8 = 0b0111_1111;
    /// First hot-plugin module slot.
    pub const MODULE_FIRST: u8 = 128;
    /// Number of hot-plugin module slots (`128..=255`).
    pub const MODULE_COUNT: usize = 128;

    /// Core codes with no opcode today: reserved growth space for new
    /// superinstructions. Keep in sync when adding an opcode.
    pub const CORE_GROWTH: &[u8] = &[88, 89];

    /// Returns `true` when `op` belongs to the core range.
    #[inline(always)]
    pub const fn is_core(op: u8) -> bool {
        op & MODULE_SPACE_MASK == 0
    }

    /// Returns `true` when `op` is a hot-plugin module slot.
    #[inline(always)]
    pub const fn is_module_slot(op: u8) -> bool {
        op & MODULE_SPACE_MASK != 0
    }

    /// Module slot index for a module-space code (`0..=127`).
    #[inline(always)]
    pub const fn module_index(op: u8) -> u8 {
        op & MODULE_INDEX_MASK
    }
}

#[cfg(test)]
mod layout_tests {
    use super::layout;
    use crate::runtime::vm::ops::opcode_name_core;

    #[test]
    fn opcode_space_is_partitioned() {
        assert_eq!(layout::CORE_FIRST, 0);
        assert_eq!(layout::CORE_LAST + 1, layout::MODULE_FIRST);
        assert_eq!(layout::MODULE_FIRST as usize + layout::MODULE_COUNT, 256);
        for op in 0..=u8::MAX {
            assert!(
                layout::is_core(op) ^ layout::is_module_slot(op),
                "opcode {op} belongs to no range or to both"
            );
        }
    }

    #[test]
    fn escape_code_is_the_only_owner_of_its_byte() {
        // `OP_EXT` is a real core opcode now: it carries the escape into cold
        // plugin modules. It must stay inside the core range and must not be
        // listed as growth space.
        assert!(layout::is_core(layout::OP_EXT));
        assert_eq!(opcode_name_core(layout::OP_EXT), Some("OP_EXT"));
        assert!(!layout::CORE_GROWTH.contains(&layout::OP_EXT));
    }

    #[test]
    fn module_index_is_a_single_mask() {
        for slot in 0..layout::MODULE_COUNT {
            let op = layout::MODULE_FIRST + slot as u8;
            assert!(layout::is_module_slot(op));
            assert_eq!(layout::module_index(op), slot as u8);
        }
        assert!(!layout::is_module_slot(layout::OP_EXT));
    }

    #[test]
    fn every_core_code_has_a_defined_owner() {
        // 0 = unaccounted, 1 = defined opcode, 2 = growth
        let mut owner = [0u8; 128];
        for op in layout::CORE_FIRST..=layout::CORE_LAST {
            if opcode_name_core(op).is_some() {
                owner[op as usize] = 1;
            }
        }
        for &op in layout::CORE_GROWTH {
            assert_eq!(
                owner[op as usize], 0,
                "growth code {op} is already a defined opcode; remove it from CORE_GROWTH"
            );
            owner[op as usize] = 2;
        }

        let missing: Vec<u8> = (layout::CORE_FIRST..=layout::CORE_LAST)
            .filter(|op| owner[*op as usize] == 0)
            .collect();
        assert!(
            missing.is_empty(),
            "unaccounted core opcode codes: {missing:?}"
        );
    }
}
