//! Opcode constants for the XCL bytecode instruction set.
//!
//! Each opcode is an 8-bit value that identifies a specific virtual machine instruction.
//! Opcodes are grouped by category: control flow, arithmetic, comparison, locals/globals,
//! fused superinstructions, and specialized loop opcodes.

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(feature = "std")]
use std::sync::Arc;

/// No-operation opcode.
pub const OP_NOP: u8 = 0;
/// Pushes `nil` (`none`) onto the operand stack.
pub const OP_PUSH_NIL: u8 = 1;
/// Pushes boolean `true` onto the operand stack.
pub const OP_PUSH_TRUE: u8 = 2;
/// Pushes boolean `false` onto the operand stack.
pub const OP_PUSH_FALSE: u8 = 3;
/// Pushes a small integer constant encoded directly in operand `a`.
pub const OP_PUSH_INT: u8 = 4;
/// Pushes a 64-bit integer constant indexed from `wide_operands` by operand `a`.
pub const OP_PUSH_INT_WIDE: u8 = 5;
/// Pushes a 64-bit float constant indexed from `wide_operands` by operand `a`.
pub const OP_PUSH_FLOAT: u8 = 6;
/// Pushes an interned string indexed from `string_table` by operand `a`.
pub const OP_PUSH_STR: u8 = 7;
/// Loads a local variable from slot index `a` onto the stack.
pub const OP_LOAD_LOCAL: u8 = 8;
/// Pops a value from the stack and stores it into local slot index `a`.
pub const OP_STORE_LOCAL: u8 = 9;
/// Loads a global variable from slot index `a` onto the stack.
pub const OP_LOAD_GLOBAL: u8 = 10;
/// Pops a value from the stack and stores it into global slot index `a`.
pub const OP_STORE_GLOBAL: u8 = 11;

/// Pops two values, adds them, and pushes the result.
pub const OP_ADD: u8 = 12;
/// Pops two values, subtracts `top` from `second`, and pushes the result.
pub const OP_SUB: u8 = 13;
/// Pops two values, multiplies them, and pushes the result.
pub const OP_MUL: u8 = 14;
/// Pops two values, divides `second` by `top`, and pushes the result.
pub const OP_DIV: u8 = 15;
/// Pops two values, computes modulo remainder, and pushes the result.
pub const OP_MOD: u8 = 16;
/// Negates the value on the top of the stack.
pub const OP_NEG: u8 = 17;

/// Compares two top stack values for equality (`==`).
pub const OP_EQ: u8 = 18;
/// Compares two top stack values for inequality (`!=`).
pub const OP_NEQ: u8 = 19;
/// Tests if second stack value is less than top stack value (`<`).
pub const OP_LT: u8 = 20;
/// Tests if second stack value is greater than top stack value (`>`).
pub const OP_GT: u8 = 21;
/// Tests if second stack value is less than or equal to top stack value (`<=`).
pub const OP_LE: u8 = 22;
/// Tests if second stack value is greater than or equal to top stack value (`>=`).
pub const OP_GE: u8 = 23;
/// Logical AND of top two stack values.
pub const OP_AND: u8 = 24;
/// Logical OR of top two stack values.
pub const OP_OR: u8 = 25;
/// Logical inversion of top stack value (`!`).
pub const OP_NOT: u8 = 26;
/// Tests membership (`in`).
pub const OP_IN: u8 = 27;

/// Unconditional jump to instruction pointer address `a`.
pub const OP_JUMP: u8 = 28;
/// Conditional jump to address `a` if top of stack is falsy.
pub const OP_JUMP_IF_FALSE: u8 = 29;
/// Conditional jump to address `a` if top of stack is nil.
pub const OP_JUMP_IF_NIL: u8 = 30;

/// Calls a function or closure. Operand `a` = argument count.
pub const OP_CALL: u8 = 32;
/// Returns from the current function call frame.
pub const OP_RETURN: u8 = 33;
/// Halts VM execution.
pub const OP_HALT: u8 = 34;

/// Constructs an array from `a` elements on the stack.
pub const OP_MAKE_ARRAY: u8 = 35;
/// Constructs a dictionary map from `a` key-value pairs on the stack.
pub const OP_MAKE_DICT: u8 = 36;
/// Retrieves an element by indexing `target[index]`.
pub const OP_GET_INDEX: u8 = 37;
/// Accesses a field `target.field` using interned string index `a`.
pub const OP_GET_FIELD: u8 = 38;
/// Pushes an element onto an array.
pub const OP_ARRAY_PUSH: u8 = 39;
/// Initializes iteration over an iterable collection.
pub const OP_FOR_ITER_START: u8 = 42;
/// Advances to the next item in an iterator.
pub const OP_FOR_ITER_NEXT: u8 = 43;
/// Initializes a numeric range loop.
pub const OP_FOR_RANGE_START: u8 = 44;
/// Advances to the next integer in a numeric range loop.
pub const OP_FOR_RANGE_NEXT: u8 = 45;

/// Loads a compiled lambda closure onto the stack. Operand `a` = closure index.
pub const OP_LOAD_LAMBDA: u8 = 46;

/// Null coalescing operator (`??`).
pub const OP_COALESCE: u8 = 47;

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

/// Bitwise left shift: pop two values, push `a << b`.
pub const OP_SHL: u8 = 115;
/// Bitwise right shift: pop two values, push `a >> b`.
pub const OP_SHR: u8 = 116;
/// Bitwise AND: pop two values, push `a & b`.
pub const OP_BITAND: u8 = 117;

/// Compiled XCL bytecode unit ready for execution by the VM runtime.
///
/// Stores immutable instruction sequences, constant tables, interned identifier strings,
/// and function metadata.
#[derive(Debug, Clone)]
pub struct Bytecode {
    /// Linear sequence of compiled 64-bit bytecode instructions.
    pub instructions: Arc<[super::Instr]>,
    /// Table of 64-bit wide numeric operands (large integers and IEEE-754 floats).
    pub wide_operands: Arc<[i64]>,
    /// Interned string literals and member/method names.
    pub string_table: Arc<[Box<str>]>,
    /// Pre-allocated string [`Value`](crate::runtime::value::Value) constants corresponding to `string_table`.
    pub string_values: Arc<[crate::runtime::value::Value]>,
    /// Local variable names mapped to slot indices.
    pub local_names: Arc<[Box<str>]>,
    /// Global variable names mapped to slot indices.
    pub global_names: Arc<[Box<str>]>,
    /// User-defined top-level function declarations.
    pub user_functions: Vec<(Box<str>, crate::runtime::value::FuncValue)>,
    /// Compiled nested closures and lambda bodies.
    pub closures: Vec<crate::runtime::value::FuncValue>,
}
