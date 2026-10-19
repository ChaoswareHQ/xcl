//! Opcode constants for the XCL bytecode instruction set.
//!
//! Each opcode is an 8-bit value that identifies a specific virtual machine instruction.
//! Opcodes are grouped by category: control flow, arithmetic, comparison, locals/globals,
//! fused superinstructions, and specialized loop opcodes.
//!
//! Migrated core opcodes are re-exported from the VM macro-generated module.

pub use crate::runtime::vm::ops::consts::*;

/// Calls a function or closure. Operand `a` = argument count.
pub const OP_CALL: u8 = 32;
/// Accesses a field `target.field` using interned string index `a`.
pub const OP_GET_FIELD: u8 = 38;
/// Initializes iteration over an iterable collection.
pub const OP_FOR_ITER_START: u8 = 42;
/// Advances to the next item in an iterator.
pub const OP_FOR_ITER_NEXT: u8 = 43;
/// Initializes a numeric range loop.
pub const OP_FOR_RANGE_START: u8 = 44;
/// Advances to the next integer in a numeric range loop.
pub const OP_FOR_RANGE_NEXT: u8 = 45;

/// Dynamic type test: checks if top of stack matches type index `a`.
pub const OP_TYPE_TEST: u8 = 52;

/// Type cast: casts value on top of stack to type index `a`.
pub const OP_TYPE_CAST: u8 = 53;

/// Fused instruction: loads two local variables sequentially (`locals[a]`, `locals[b]`).
pub const OP_LOAD_LOCAL_LOAD_LOCAL: u8 = 60;
/// Fused instruction: adds two local variables (`locals[a] + locals[b]`).
pub const OP_ADD_LOCAL_LOCAL: u8 = 61;
/// Fused instruction: subtracts two local variables (`locals[a] - locals[b]`).
pub const OP_SUB_LOCAL_LOCAL: u8 = 62;
/// Fused instruction: tests if `locals[a] < locals[b]`.
pub const OP_LT_LOCAL_LOCAL: u8 = 63;
/// Fused instruction: loads a local and pushes an integer constant.
pub const OP_LOAD_LOCAL_PUSH_INT: u8 = 64;
/// Fused instruction: copies `locals[a]` to `locals[b]`.
pub const OP_LOAD_LOCAL_STORE_LOCAL: u8 = 65;

/// Calls a domain or native method on receiver with `b` arguments.
///
/// Stack layout: receiver, arg1, arg2, ... -> result.
/// Operand `a` = method name string index in `string_table`, `b` = argument count.
pub const OP_CALL_METHOD: u8 = 66;

/// Superinstruction: increments local variable in-place (`$x = $x + 1`).
pub const OP_INC_LOCAL: u8 = 80;
/// Superinstruction: decrements local variable in-place (`$x = $x - 1`).
pub const OP_DEC_LOCAL: u8 = 81;
/// Superinstruction: multiplies two locals (`$a * $b`).
pub const OP_MUL_LOCAL_LOCAL: u8 = 82;
/// Superinstruction: divides two locals (`$a / $b`).
pub const OP_DIV_LOCAL_LOCAL: u8 = 83;
/// Superinstruction: adds local to integer constant and stores (`locals[a] = locals[b] + c`).
pub const OP_ADD_INT_STORE_LOCAL: u8 = 84;
/// Superinstruction: adds integer constant to local in-place (`locals[a] = locals[a] + b`).
pub const OP_INC_LOCAL_INT: u8 = 85;

/// Single-opcode numeric for-loop iterator.
///
/// Operand `a` = local slot index, `b` = backward jump offset, `c` = step increment.
pub const OP_FOR_NUMBER_LOOP: u8 = 90;

/// Fused: load local, add integer constant, store back (`$x = $x + c`).
pub const OP_ADD_INT_LOCAL: u8 = 91;
/// Fused: load two locals, add, store to first (`$a = $a + $b`).
pub const OP_ADD_LOCAL_STORE: u8 = 92;
/// Fused: load local, sub integer constant, store back (`$x = $x - c`).
pub const OP_SUB_INT_LOCAL: u8 = 93;
/// Fused: load local, mul integer constant, store back (`$x = $x * c`).
pub const OP_MUL_INT_LOCAL: u8 = 94;

/// Fused while-loop condition and conditional jump (`while $a < b`).
pub const OP_WHILE_LT_INT: u8 = 95;
/// Fused: load local, mul local, store back (`$a = $a * $b`).
pub const OP_MUL_LOCAL_STORE: u8 = 96;
/// Fused: load local, div local, store back (`$a = $a / $b`).
pub const OP_DIV_LOCAL_STORE: u8 = 97;
/// Fused: load local, sub local, store back (`$a = $a - $b`).
pub const OP_SUB_LOCAL_STORE: u8 = 98;
/// Fused: load local, add local, store to third (`$c = $a + $b`).
pub const OP_ADD_LOCAL_LOCAL_STORE: u8 = 99;
/// Fused: load local, mul local, store to third (`$c = $a * $b`).
pub const OP_MUL_LOCAL_LOCAL_STORE: u8 = 100;
/// Fused: load local, sub local, store to third (`$c = $a - $b`).
pub const OP_SUB_LOCAL_LOCAL_STORE: u8 = 101;
/// Fused: load local, div local, store to third (`$c = $a / $b`).
pub const OP_DIV_LOCAL_LOCAL_STORE: u8 = 102;
/// Fused while-loop condition and conditional jump comparing two locals (`while $a < $b`).
pub const OP_WHILE_LT_LOCAL: u8 = 103;
/// Fused array/map index load into local: `locals[a] = locals[c][locals[b]]`.
pub const OP_GET_INDEX_LOCAL_LOCAL_STORE: u8 = 105;
/// Fused map field load into local: `locals[a] = locals[c].(string_table[b])`.
pub const OP_GET_FIELD_LOCAL_STORE: u8 = 106;
/// Fused modulo local-local store: `locals[a] = locals[c] % locals[b]`.
pub const OP_MOD_LOCAL_LOCAL_STORE: u8 = 107;
/// Loads a constant from `wide_operands[b]` into `locals[a]` (`c = 0` for int, `c = 1` for float).
pub const OP_LOAD_CONST_LOCAL: u8 = 108;
/// Appends `locals[b]` in-place to the array in `locals[a]`.
pub const OP_ARRAY_PUSH_LOCAL: u8 = 109;

/// Fused: load local, add float constant from wide_operands[b], store back (`$x = $x + c`).
pub const OP_ADD_FLOAT_LOCAL: u8 = 110;
/// Fused: load local, sub float constant from wide_operands[b], store back (`$x = $x - c`).
pub const OP_SUB_FLOAT_LOCAL: u8 = 111;
/// Fused: load local, mul float constant from wide_operands[b], store back (`$x = $x * c`).
pub const OP_MUL_FLOAT_LOCAL: u8 = 112;
/// Fused: load local, div float constant from wide_operands[b], store back (`$x = $x / c`).
pub const OP_DIV_FLOAT_LOCAL: u8 = 113;

/// Fused: modulo and equality test (`locals[a] % b == c`).
/// Pops a local, computes modulo, and compares with the equality constant.
/// Operand `a` = local slot, `b` = modulo divisor, `c` = equality value.
pub const OP_MOD_EQ_INT: u8 = 114;
