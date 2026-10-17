//! Compact 64-bit bytecode instruction set, compiler, and peephole optimizer.
//!
//! # Overview
//!
//! The XCL bytecode system translates AST expressions into a high-density, cache-friendly
//! stream of 64-bit instructions executed by the [`crate::vm::VM`].
//!
//! # Instruction Encoding
//!
//! Every [`Instr`] is a single transparent 64-bit integer (`u64`) partitioned into four fields:
//!
//! ```text
//!  63        56 55              40 39                              8 7        0
//! +------------+------------------+---------------------------------+----------+
//! |  c (8-bit) |    b (16-bit)    |           a (32-bit)            | op (8-bit|
//! +------------+------------------+---------------------------------+----------+
//! ```
//!
//! * **`op` (bits 0..8)**: 8-bit opcode identifying the instruction.
//! * **`a` (bits 8..40)**: 32-bit primary operand (jump destination instruction pointer,
//!   string pool index, wide operand table index, or local slot index).
//! * **`b` (bits 40..56)**: 16-bit secondary operand (argument count, second local slot, or small immediate).
//! * **`c` (bits 56..64)**: 8-bit auxiliary operand or configuration flag.
//!
//! # Superinstructions & Peephole Optimization
//!
//! The compiler includes an integrated peephole optimizer that fuses common instruction
//! patterns into single superinstructions to eliminate stack traffic and branch penalties:
//!
//! * **Arithmetic fusion**: `LOAD_LOCAL $a; LOAD_LOCAL $b; ADD; STORE_LOCAL $c` is fused
//!   into [`OP_ADD_LOCAL_LOCAL_STORE`](crate::bytecode::OP_ADD_LOCAL_LOCAL_STORE).
//! * **Loop specialization**: Single-opcode numeric range loops use [`OP_FOR_NUMBER_LOOP`](crate::bytecode::OP_FOR_NUMBER_LOOP).
//! * **Fused while branches**: [`OP_WHILE_LT_INT`](crate::bytecode::OP_WHILE_LT_INT) and [`OP_WHILE_LT_LOCAL`](crate::bytecode::OP_WHILE_LT_LOCAL) execute loop
//!   comparisons and jumps in a single dispatch step.
//!
//! # Examples
//!
//! Compiling an AST into bytecode:
//!
//! ```
//! use xcl_vm::lexer::lex;
//! use xcl_vm::parser::parse;
//! use xcl_vm::bytecode::{compile, OP_ADD};
//!
//! let tokens = lex("20 + 30", "<eval>");
//! let ast = parse(&tokens, "20 + 30");
//! let code = compile(&ast);
//!
//! assert!(!code.instructions.is_empty());
//! ```

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
use alloc::vec;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::sync::Arc;

use crate::ast::{AstNode, BinaryOperator, TypeKind, UnaryOperator};
use crate::value::FuncValue;

/// A 64-bit packed virtual machine instruction.
///
/// Encodes an 8-bit opcode, a 32-bit primary operand `a`, a 16-bit secondary operand `b`,
/// and an 8-bit auxiliary operand `c` into a single machine word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Instr(pub u64);

impl Instr {
    /// Extracts the 8-bit opcode (`bits 0..8`).
    #[inline(always)]
    pub const fn op(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Extracts the 32-bit primary operand `a` (`bits 8..40`).
    #[inline(always)]
    pub const fn a(self) -> u32 {
        ((self.0 >> 8) & 0xFFFF_FFFF) as u32
    }

    /// Extracts the 16-bit secondary operand `b` (`bits 40..56`).
    #[inline(always)]
    pub const fn b(self) -> u16 {
        ((self.0 >> 40) & 0xFFFF) as u16
    }

    /// Extracts the 8-bit auxiliary operand `c` (`bits 56..64`).
    #[inline(always)]
    pub const fn c(self) -> u8 {
        ((self.0 >> 56) & 0xFF) as u8
    }

    /// Packs raw `op`, `a`, `b`, and `c` components into a 64-bit [`Instr`].
    #[inline(always)]
    pub const fn make(op: u8, a: u32, b: u16, c: u8) -> Self {
        Instr((op as u64) | ((a as u64) << 8) | ((b as u64) << 40) | ((c as u64) << 56))
    }

    /// Creates an instruction with no operands (`a = 0`, `b = 0`, `c = 0`).
    #[inline(always)]
    pub const fn none(op: u8) -> Self {
        Self::make(op, 0, 0, 0)
    }

    /// Creates an instruction with a single 32-bit operand `a` (`b = 0`, `c = 0`).
    #[inline(always)]
    pub const fn a1(op: u8, a: u32) -> Self {
        Self::make(op, a, 0, 0)
    }

    /// Creates an instruction with two operands `a` (32-bit) and `b` (16-bit) (`c = 0`).
    #[inline(always)]
    pub const fn a2(op: u8, a: u32, b: u16) -> Self {
        Self::make(op, a, b, 0)
    }
}

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
/// Computes the length of a string, array, or dictionary.
pub const OP_LEN: u8 = 41;

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

/// Specialized fast integer addition (bypasses type checking when types are proven integer).
pub const OP_ADD_INT: u8 = 70;
/// Specialized fast integer subtraction.
pub const OP_SUB_INT: u8 = 71;
/// Specialized fast integer multiplication.
pub const OP_MUL_INT: u8 = 72;
/// Specialized fast integer less-than comparison.
pub const OP_LT_INT: u8 = 73;
/// Specialized fast integer greater-than comparison.
pub const OP_GT_INT: u8 = 74;
/// Specialized fast integer equality comparison.
pub const OP_EQ_INT: u8 = 75;

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
/// Fused unary math intrinsic: `locals[a] = math_op(locals[b])` where `c` selects the function.
pub const OP_MATH_UNARY: u8 = 104;
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

/// Math intrinsic IDs for [`OP_MATH_UNARY`].
pub const MATH_SQRT: u8 = 0;
pub const MATH_SIN: u8 = 1;
pub const MATH_COS: u8 = 2;
pub const MATH_TAN: u8 = 3;
pub const MATH_EXP: u8 = 4;
pub const MATH_LOG: u8 = 5;
pub const MATH_ABS: u8 = 6;
pub const MATH_FLOOR: u8 = 7;
pub const MATH_CEIL: u8 = 8;
pub const MATH_ROUND: u8 = 9;
pub const MATH_TANH: u8 = 10;
pub const MATH_LOG2: u8 = 11;
pub const MATH_LOG10: u8 = 12;
pub const MATH_CBRT: u8 = 13;

#[inline]
fn math_unary_id(name: &str) -> Option<u8> {
    match name {
        "math.sqrt" => Some(MATH_SQRT),
        "math.sin" => Some(MATH_SIN),
        "math.cos" => Some(MATH_COS),
        "math.tan" => Some(MATH_TAN),
        "math.exp" => Some(MATH_EXP),
        "math.log" => Some(MATH_LOG),
        "math.abs" => Some(MATH_ABS),
        "math.floor" => Some(MATH_FLOOR),
        "math.ceil" => Some(MATH_CEIL),
        "math.round" => Some(MATH_ROUND),
        "math.tanh" => Some(MATH_TANH),
        "math.log2" => Some(MATH_LOG2),
        "math.log10" => Some(MATH_LOG10),
        "math.cbrt" => Some(MATH_CBRT),
        _ => None,
    }
}

/// Compiled XCL bytecode unit ready for execution by the VM runtime.
///
/// Stores immutable instruction sequences, constant tables, interned identifier strings,
/// and function metadata.
#[derive(Debug, Clone)]
pub struct Bytecode {
    /// Linear sequence of compiled 64-bit bytecode instructions.
    pub instructions: Arc<[Instr]>,
    /// Table of 64-bit wide numeric operands (large integers and IEEE-754 floats).
    pub wide_operands: Arc<[i64]>,
    /// Interned string literals and member/method names.
    pub string_table: Arc<[Box<str>]>,
    /// Pre-allocated string [`Value`] constants corresponding to `string_table`.
    pub string_values: Arc<[Value]>,
    /// Local variable names mapped to slot indices.
    pub local_names: Arc<[Box<str>]>,
    /// Global variable names mapped to slot indices.
    pub global_names: Arc<[Box<str>]>,
    /// User-defined top-level function declarations.
    pub user_functions: Vec<(Box<str>, FuncValue)>,
    /// Compiled nested closures and lambda bodies.
    pub closures: Vec<FuncValue>,
}

struct Compiler {
    code: Vec<Instr>,
    wide: Vec<i64>,
    strings: Vec<Box<str>>,
    locals: Vec<Box<str>>,
    globals: Vec<Box<str>>,
    functions: Vec<(Box<str>, FuncValue)>,
    closures: Vec<FuncValue>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            code: Vec::with_capacity(64),
            wide: Vec::new(),
            strings: Vec::new(),
            locals: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            closures: Vec::new(),
        }
    }

    #[inline(always)]
    fn emit(&mut self, i: Instr) {
        self.code.push(i);
    }
    #[inline(always)]
    fn ip(&self) -> u32 {
        self.code.len() as u32
    }

    fn patch_jump(&mut self, at: u32, target: u32) {
        let i = &mut self.code[at as usize];
        *i = Instr::make(i.op(), target, i.b(), i.c());
    }

    #[inline]
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| &**x == s) {
            return i as u32;
        }
        let i = self.strings.len() as u32;
        self.strings.push(s.into());
        i
    }

    #[inline]
    fn local(&mut self, name: &str) -> u32 {
        if let Some(i) = self.locals.iter().position(|x| &**x == name) {
            return i as u32;
        }
        let i = self.locals.len() as u32;
        self.locals.push(name.into());
        i
    }

    #[inline]
    fn global(&mut self, name: &str) -> u32 {
        if let Some(i) = self.globals.iter().position(|x| &**x == name) {
            return i as u32;
        }
        let i = self.globals.len() as u32;
        self.globals.push(name.into());
        i
    }

    /// Check if a name is a registered global (enum variant, type name, etc.)
    #[inline]
    fn is_global(&self, name: &str) -> bool {
        self.globals.iter().any(|x| &**x == name)
    }

    #[inline]
    fn wide(&mut self, v: i64) -> u32 {
        let i = self.wide.len() as u32;
        self.wide.push(v);
        i
    }

    #[inline]
    fn temp_slot(&mut self, depth: u32) -> u32 {
        let name = format!("__tmp_{depth}");
        self.local(&name)
    }

    fn can_emit_to_slot(&self, node: &AstNode) -> bool {
        match node {
            AstNode::Ident { name, .. } => !self.is_global(name),
            AstNode::Int { .. } | AstNode::Float { .. } => true,
            AstNode::UnaryOp {
                op: UnaryOperator::Neg,
                operand,
                ..
            } => self.can_emit_to_slot(operand),
            AstNode::BinaryOp {
                left,
                op:
                    BinaryOperator::Add
                    | BinaryOperator::Sub
                    | BinaryOperator::Mul
                    | BinaryOperator::Div
                    | BinaryOperator::Mod,
                right,
                ..
            } => self.can_emit_to_slot(left) && self.can_emit_to_slot(right),
            AstNode::Index { target, index, .. } => {
                self.can_emit_to_slot(target) && self.can_emit_to_slot(index)
            }
            AstNode::FieldAccess { target, .. } => self.can_emit_to_slot(target),
            AstNode::Call { callee, args, .. } => {
                args.len() == 1
                    && args[0].name.is_none()
                    && collect_dotted_name(callee)
                        .and_then(|n| math_unary_id(&n))
                        .is_some()
                    && self.can_emit_to_slot(&args[0].value)
            }
            _ => false,
        }
    }

    fn emit_expr_to_slot(&mut self, node: &AstNode, temp_depth: u32) -> u32 {
        match node {
            AstNode::Ident { name, .. } => self.local(name),
            AstNode::Int { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                let w = self.wide(*value);
                self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 0));
                dst
            }
            AstNode::Float { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                let w = self.wide(value.to_bits() as i64);
                self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 1));
                dst
            }
            AstNode::UnaryOp {
                op: UnaryOperator::Neg,
                operand,
                ..
            } => {
                if let AstNode::Int { value, .. } = operand.as_ref() {
                    let dst = self.temp_slot(temp_depth);
                    let w = self.wide(-*value);
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 0));
                    return dst;
                }
                if let AstNode::Float { value, .. } = operand.as_ref() {
                    let dst = self.temp_slot(temp_depth);
                    let w = self.wide((-*value).to_bits() as i64);
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 1));
                    return dst;
                }
                let zero_slot = self.temp_slot(temp_depth);
                let w = self.wide(0);
                self.emit(Instr::make(OP_LOAD_CONST_LOCAL, zero_slot, w as u16, 0));
                let src = self.emit_expr_to_slot(operand, temp_depth + 1);
                self.emit(Instr::make(
                    OP_SUB_LOCAL_LOCAL_STORE,
                    zero_slot,
                    src as u16,
                    zero_slot as u8,
                ));
                zero_slot
            }
            AstNode::Index { target, index, .. } => {
                let t_slot = self.emit_expr_to_slot(target, temp_depth);
                let i_slot = self.emit_expr_to_slot(index, temp_depth + 1);
                let dst = self.temp_slot(temp_depth);
                self.emit(Instr::make(
                    OP_GET_INDEX_LOCAL_LOCAL_STORE,
                    dst,
                    i_slot as u16,
                    t_slot as u8,
                ));
                dst
            }
            AstNode::FieldAccess { target, field, .. } => {
                let t_slot = self.emit_expr_to_slot(target, temp_depth);
                let field_idx = self.intern(field);
                let dst = self.temp_slot(temp_depth);
                self.emit(Instr::make(
                    OP_GET_FIELD_LOCAL_STORE,
                    dst,
                    field_idx as u16,
                    t_slot as u8,
                ));
                dst
            }
            AstNode::Call { callee, args, .. } => {
                let cname = collect_dotted_name(callee).unwrap_or_default();
                let math_id = math_unary_id(&cname).unwrap_or(MATH_SQRT);
                let src = self.emit_expr_to_slot(&args[0].value, temp_depth);
                let dst = self.temp_slot(temp_depth);
                self.emit(Instr::make(OP_MATH_UNARY, dst, src as u16, math_id));
                dst
            }
            AstNode::BinaryOp {
                left, op, right, ..
            } => {
                let l_slot = self.emit_expr_to_slot(left, temp_depth);
                let r_slot = self.emit_expr_to_slot(right, temp_depth + 1);
                let dst = self.temp_slot(temp_depth);
                let opc = match op {
                    BinaryOperator::Add => OP_ADD_LOCAL_LOCAL_STORE,
                    BinaryOperator::Sub => OP_SUB_LOCAL_LOCAL_STORE,
                    BinaryOperator::Mul => OP_MUL_LOCAL_LOCAL_STORE,
                    BinaryOperator::Div => OP_DIV_LOCAL_LOCAL_STORE,
                    BinaryOperator::Mod => OP_MOD_LOCAL_LOCAL_STORE,
                    _ => unreachable!(),
                };
                self.emit(Instr::make(opc, dst, r_slot as u16, l_slot as u8));
                dst
            }
            _ => unreachable!(),
        }
    }

    /// Try to fuse a nested binary expression into superinstructions.
    /// For example, `$zr = $zr*$zr - $zi*$zi + $cr` becomes:
    ///   MUL_LL_ST(temp1, zr, zr)
    ///   MUL_LL_ST(temp2, zi, zi)
    ///   SUB_LL_ST(temp1, temp1, temp2)
    ///   ADD_LL_ST(zr, temp1, cr)
    fn try_emit_binary_fused(&mut self, dest_slot: u32, node: &AstNode) -> bool {
        let AstNode::BinaryOp {
            left, op, right, ..
        } = node
        else {
            return false;
        };

        // Case 1: $dest = $dest op $b (simple self-assignment)
        if let AstNode::Ident { name, .. } = left.as_ref()
            && !self.is_global(name)
            && self.local(name) == dest_slot
        {
            if let AstNode::Ident { name: rn, .. } = right.as_ref()
                && !self.is_global(rn)
            {
                let rslot = self.local(rn);
                let opc = match op {
                    BinaryOperator::Add => Some(OP_ADD_LOCAL_STORE),
                    BinaryOperator::Sub => Some(OP_SUB_LOCAL_STORE),
                    BinaryOperator::Mul => Some(OP_MUL_LOCAL_STORE),
                    BinaryOperator::Div => Some(OP_DIV_LOCAL_STORE),
                    _ => None,
                };
                if let Some(opc) = opc {
                    self.emit(Instr::a2(opc, dest_slot, rslot as u16));
                    return true;
                }
            }
            if let AstNode::Int { value, .. } = right.as_ref() {
                if *value == 1 && matches!(op, BinaryOperator::Add) {
                    self.emit(Instr::a1(OP_INC_LOCAL, dest_slot));
                    return true;
                }
                if *value == 1 && matches!(op, BinaryOperator::Sub) {
                    self.emit(Instr::a1(OP_DEC_LOCAL, dest_slot));
                    return true;
                }
                if (0..=u16::MAX as i64).contains(value) {
                    let opc = match op {
                        BinaryOperator::Add => Some(OP_ADD_INT_LOCAL),
                        BinaryOperator::Sub => Some(OP_SUB_INT_LOCAL),
                        BinaryOperator::Mul => Some(OP_MUL_INT_LOCAL),
                        _ => None,
                    };
                    if let Some(opc) = opc {
                        self.emit(Instr::a2(opc, dest_slot, *value as u16));
                        return true;
                    }
                }
            }
        }

        // Case 2: $dest = $a op $b (both distinct locals)
        if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. }) =
            (left.as_ref(), right.as_ref())
            && !self.is_global(ln)
            && !self.is_global(rn)
        {
            let lslot = self.local(ln);
            let rslot = self.local(rn);
            if lslot < 256 {
                let c_src1 = (lslot & 0xFF) as u8;
                let b_src2 = (rslot & 0xFFFF) as u16;
                let opc = match op {
                    BinaryOperator::Add => Some(OP_ADD_LOCAL_LOCAL_STORE),
                    BinaryOperator::Sub => Some(OP_SUB_LOCAL_LOCAL_STORE),
                    BinaryOperator::Mul => Some(OP_MUL_LOCAL_LOCAL_STORE),
                    BinaryOperator::Div => Some(OP_DIV_LOCAL_LOCAL_STORE),
                    BinaryOperator::Mod => Some(OP_MOD_LOCAL_LOCAL_STORE),
                    _ => None,
                };
                if let Some(opc) = opc {
                    self.emit(Instr::make(opc, dest_slot, b_src2, c_src1));
                    return true;
                }
            }
        }

        // Case 3: Nested 3-address register compilation
        if matches!(
            op,
            BinaryOperator::Add
                | BinaryOperator::Sub
                | BinaryOperator::Mul
                | BinaryOperator::Div
                | BinaryOperator::Mod
        ) && self.locals.len() < 200
            && self.can_emit_to_slot(node)
        {
            if let AstNode::Ident { name: ln, .. } = left.as_ref()
                && !self.is_global(ln)
                && self.local(ln) == dest_slot
                && matches!(
                    op,
                    BinaryOperator::Add
                        | BinaryOperator::Sub
                        | BinaryOperator::Mul
                        | BinaryOperator::Div
                )
            {
                let r_slot = self.emit_expr_to_slot(right, 0);
                let opc = match op {
                    BinaryOperator::Add => OP_ADD_LOCAL_STORE,
                    BinaryOperator::Sub => OP_SUB_LOCAL_STORE,
                    BinaryOperator::Mul => OP_MUL_LOCAL_STORE,
                    BinaryOperator::Div => OP_DIV_LOCAL_STORE,
                    _ => unreachable!(),
                };
                self.emit(Instr::a2(opc, dest_slot, r_slot as u16));
                return true;
            }

            let l_slot = self.emit_expr_to_slot(left, 0);
            let r_slot = self.emit_expr_to_slot(right, 1);
            if l_slot < 256 {
                let opc = match op {
                    BinaryOperator::Add => OP_ADD_LOCAL_LOCAL_STORE,
                    BinaryOperator::Sub => OP_SUB_LOCAL_LOCAL_STORE,
                    BinaryOperator::Mul => OP_MUL_LOCAL_LOCAL_STORE,
                    BinaryOperator::Div => OP_DIV_LOCAL_LOCAL_STORE,
                    BinaryOperator::Mod => OP_MOD_LOCAL_LOCAL_STORE,
                    _ => unreachable!(),
                };
                self.emit(Instr::make(opc, dest_slot, r_slot as u16, l_slot as u8));
                return true;
            }
        }

        false
    }

    fn try_fuse_load_load(&mut self, second: u32) -> bool {
        if let Some(prev) = self.code.last().copied()
            && prev.op() == OP_LOAD_LOCAL
        {
            let a = prev.a();
            self.code.pop();
            self.emit(Instr::make(OP_LOAD_LOCAL_LOAD_LOCAL, a, second as u16, 0));
            return true;
        }
        false
    }

    fn try_fuse_add_local_local(&mut self) -> bool {
        // If the last instruction is already LL_LL, upgrade it to ADD_LOCAL_LOCAL
        if let Some(prev) = self.code.last().copied() {
            if prev.op() == OP_LOAD_LOCAL_LOAD_LOCAL {
                self.code.pop();
                self.emit(Instr::make(OP_ADD_LOCAL_LOCAL, prev.a(), prev.b(), 0));
                return true;
            }
        }
        // Fallback: two consecutive LOAD_LOCAL instructions
        if self.code.len() < 2 {
            return false;
        }
        let l2 = self.code[self.code.len() - 1].op();
        let l1 = self.code[self.code.len() - 2].op();
        if l1 == OP_LOAD_LOCAL && l2 == OP_LOAD_LOCAL {
            let a = self.code[self.code.len() - 2].a();
            let b_slot = self.code[self.code.len() - 1].a();
            self.code.truncate(self.code.len() - 2);
            self.emit(Instr::make(OP_ADD_LOCAL_LOCAL, a, b_slot as u16, 0));
            return true;
        }
        false
    }

    /// Check if the last instruction is LL_LL and upgrade it, or two LOAD_LOCALs
    fn try_fuse_local_local(&mut self, op: u8) -> bool {
        // If the last instruction is already LL_LL, upgrade it to the arithmetic opcode
        if let Some(prev) = self.code.last().copied() {
            if prev.op() == OP_LOAD_LOCAL_LOAD_LOCAL {
                self.code.pop();
                self.emit(Instr::make(op, prev.a(), prev.b(), 0));
                return true;
            }
        }
        // Fallback: two consecutive LOAD_LOCAL instructions
        if self.code.len() < 2 {
            return false;
        }
        let l2 = self.code[self.code.len() - 1].op();
        let l1 = self.code[self.code.len() - 2].op();
        if l1 == OP_LOAD_LOCAL && l2 == OP_LOAD_LOCAL {
            let a = self.code[self.code.len() - 2].a();
            let b_slot = self.code[self.code.len() - 1].a();
            self.code.truncate(self.code.len() - 2);
            self.emit(Instr::make(op, a, b_slot as u16, 0));
            return true;
        }
        false
    }

    fn compile(&mut self, ast: &[AstNode]) -> Bytecode {
        for n in ast {
            self.node(n);
        }
        self.emit(Instr::none(OP_HALT));

        // Run peephole optimizer to fuse common patterns
        let mut code: Vec<Instr> = core::mem::take(&mut self.code);
        peephole_optimize_once(&mut code, &self.wide);

        Bytecode {
            instructions: code.into(),
            wide_operands: core::mem::take(&mut self.wide).into(),
            string_table: core::mem::take(&mut self.strings).into(),
            local_names: core::mem::take(&mut self.locals).into(),
            global_names: core::mem::take(&mut self.globals).into(),
            user_functions: core::mem::take(&mut self.functions),
            closures: core::mem::take(&mut self.closures),
        }
    }

    fn node(&mut self, node: &AstNode) {
        match node {
            AstNode::Assign { name, value, .. } => {
                let slot = self.local(name);
                // Try recursive RHS fusion for nested binary expressions
                // e.g., $zr = $zr*$zr - $zi*$zi + $cr
                if let AstNode::BinaryOp { .. } = value.as_ref() {
                    if self.try_emit_binary_fused(slot, value.as_ref()) {
                        return;
                    }
                }
                // Try to fuse hot patterns: $x = $x + 1, $x = $x - 1, $x = $x + c, $a = $a + $b
                if let AstNode::BinaryOp {
                    left, op, right, ..
                } = value.as_ref()
                {
                    if let AstNode::Ident {
                        name: left_name, ..
                    } = left.as_ref()
                    {
                        if left_name == name {
                            // $x = $x op something
                            match op {
                                BinaryOperator::Add => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x + 1 → OP_INC_LOCAL
                                        self.emit(Instr::a1(OP_INC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x + c → OP_INC_LOCAL_INT
                                        self.emit(Instr::make(
                                            OP_INC_LOCAL_INT,
                                            slot,
                                            *c as u16,
                                            0,
                                        ));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x + $y → OP_ADD_LOCAL_STORE (direct local-to-local)
                                        let right_slot = self.local(right_name);
                                        self.emit(Instr::a2(
                                            OP_ADD_LOCAL_STORE,
                                            slot,
                                            right_slot as u16,
                                        ));
                                        return;
                                    }
                                }
                                BinaryOperator::Sub => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x - 1 → OP_DEC_LOCAL
                                        self.emit(Instr::a1(OP_DEC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x - $y → OP_SUB_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        self.emit(Instr::a2(
                                            OP_SUB_LOCAL_STORE,
                                            slot,
                                            right_slot as u16,
                                        ));
                                        return;
                                    }
                                }
                                BinaryOperator::Mul => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x * c → OP_MUL_INT_LOCAL
                                        self.emit(Instr::a2(OP_MUL_INT_LOCAL, slot, *c as u16));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x * $y → OP_MUL_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        self.emit(Instr::a2(
                                            OP_MUL_LOCAL_STORE,
                                            slot,
                                            right_slot as u16,
                                        ));
                                        return;
                                    }
                                }
                                BinaryOperator::Div => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x / c → handled by general path for now
                                        let _ = c;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // NEW: $z = $a op $b (both operands are distinct locals)
                    if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. }) =
                        (left.as_ref(), right.as_ref())
                    {
                        let lslot = self.local(ln);
                        let rslot = self.local(rn);
                        let c_src1 = (lslot & 0xFF) as u8;
                        let b_src2 = (rslot & 0xFFFF) as u16;
                        let opc = match op {
                            BinaryOperator::Add => Some(OP_ADD_LOCAL_LOCAL_STORE),
                            BinaryOperator::Sub => Some(OP_SUB_LOCAL_LOCAL_STORE),
                            BinaryOperator::Mul => Some(OP_MUL_LOCAL_LOCAL_STORE),
                            BinaryOperator::Div => Some(OP_DIV_LOCAL_LOCAL_STORE),
                            _ => None,
                        };
                        if let Some(opc) = opc {
                            self.emit(Instr::make(opc, slot, b_src2, c_src1));
                            return;
                        }
                    }
                }
                self.node(value);
                self.emit(Instr::a1(OP_STORE_LOCAL, slot));
            }
            AstNode::Return { value, .. } => {
                if let Some(v) = value {
                    self.node(v);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                self.emit(Instr::none(OP_RETURN));
            }
            AstNode::Break { .. } | AstNode::Continue { .. } => {}

            AstNode::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in then_branch {
                    self.node(s);
                }
                if let Some(e) = else_branch {
                    let j2 = self.ip();
                    self.emit(Instr::a1(OP_JUMP, 0));
                    let t = self.ip();
                    self.patch_jump(j, t);
                    for s in e {
                        self.node(s);
                    }
                    let end = self.ip();
                    self.patch_jump(j2, end);
                } else {
                    let end = self.ip();
                    self.patch_jump(j, end);
                }
            }

            AstNode::For {
                variable,
                iterable,
                body,
                ..
            } => {
                if let AstNode::Range { start, end, .. } = iterable.as_ref() {
                    // Check if start and end are integer constants — if so, use OP_FOR_NUMBER_LOOP
                    let start_val = match start.as_ref().map(|n| n.as_ref()) {
                        Some(AstNode::Int { value, .. }) => Some(*value),
                        Some(_) => None,
                        None => Some(0),
                    };
                    let end_val = match end.as_ref().map(|n| n.as_ref()) {
                        Some(AstNode::Int { value, .. }) => Some(*value),
                        Some(_) => None,
                        None => Some(0),
                    };

                    if let (Some(start_i), Some(end_i)) = (start_val, end_val) {
                        // Use single-opcode FOR_NUMBER_LOOP (like Luau's FORNLOOP)
                        let var = self.local(variable);
                        // Emit start value and end value as wide operands for the loop
                        let start_wide = self.wide(start_i);
                        let end_wide = self.wide(end_i);
                        // a = (var_slot << 16) | start_wide_idx, b = end_wide_idx
                        let a_combined = var << 16 | (start_wide & 0xFFFF);
                        self.emit(Instr::a2(OP_FOR_NUMBER_LOOP, a_combined, end_wide as u16));
                        let loop_start = self.ip();
                        for s in body {
                            self.node(s);
                        }
                        // Jump back to FOR_NUMBER_LOOP
                        self.emit(Instr::a1(OP_JUMP, loop_start));
                    } else {
                        // Fallback: use FOR_RANGE_START/NEXT for non-constant ranges
                        if let Some(s) = start {
                            self.node(s);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT, 0));
                        }
                        if let Some(e) = end {
                            self.node(e);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT, 0));
                        }
                        self.emit(Instr::none(OP_FOR_RANGE_START));
                        let start_ip = self.ip();
                        let next = self.ip();
                        self.emit(Instr::a1(OP_FOR_RANGE_NEXT, 0));
                        let var = self.local(variable);
                        self.emit(Instr::a1(OP_STORE_LOCAL, var));
                        for s in body {
                            self.node(s);
                        }
                        self.emit(Instr::a1(OP_JUMP, start_ip));
                        let end = self.ip();
                        self.patch_jump(next, end);
                    }
                } else {
                    self.node(iterable);
                    self.emit(Instr::none(OP_FOR_ITER_START));
                    let start_ip = self.ip();
                    let next = self.ip();
                    self.emit(Instr::a1(OP_FOR_ITER_NEXT, 0));
                    let var = self.local(variable);
                    self.emit(Instr::a1(OP_STORE_LOCAL, var));
                    for s in body {
                        self.node(s);
                    }
                    self.emit(Instr::a1(OP_JUMP, start_ip));
                    let end = self.ip();
                    self.patch_jump(next, end);
                }
            }

            AstNode::While {
                condition, body, ..
            } => {
                // Try to fuse: while $i < N { ... } → OP_WHILE_LT_INT
                // Or: while $a < $b { ... } → OP_WHILE_LT_LOCAL
                match condition.as_ref() {
                    AstNode::BinaryOp {
                        left,
                        op: BinaryOperator::Lt,
                        right,
                        ..
                    } => {
                        if let AstNode::Ident {
                            name: left_name, ..
                        } = left.as_ref()
                        {
                            if let AstNode::Int { value: limit, .. } = right.as_ref() {
                                let slot = self.local(left_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(
                                    OP_WHILE_LT_INT,
                                    slot,
                                    *limit as u16,
                                    exit_wide as u8,
                                ));
                                for s in body {
                                    self.node(s);
                                }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                return;
                            } else if let AstNode::Ident {
                                name: right_name, ..
                            } = right.as_ref()
                            {
                                let left_slot = self.local(left_name);
                                let right_slot = self.local(right_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(
                                    OP_WHILE_LT_LOCAL,
                                    left_slot,
                                    right_slot as u16,
                                    exit_wide as u8,
                                ));
                                for s in body {
                                    self.node(s);
                                }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                return;
                            }
                        }
                    }
                    _ => {}
                };

                // Fallback: generic while loop
                let start = self.ip();
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in body {
                    self.node(s);
                }
                self.emit(Instr::a1(OP_JUMP, start));
                let end = self.ip();
                self.patch_jump(j, end);
            }

            AstNode::FunctionDef {
                body, name, params, ..
            } => {
                let mut sub = Compiler::new();
                for p in params {
                    sub.local(&p.name);
                }
                for s in body {
                    sub.node(s);
                }
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                let f = FuncValue {
                    name: name.clone(),
                    bytecode: Some(Arc::new(bc)),
                    params: params.iter().map(|p| p.name.clone()).collect(),
                    closure_captures: Vec::new(),
                    closure_env: Vec::new(),
                };
                self.local(name);
                self.functions.push((name.as_str().into(), f));
            }

            AstNode::Call { callee, args, .. } => {
                // Build full dotted name from nested FieldAccess chains.
                // e.g., net.tcp.connect -> "net.tcp.connect"
                //       io.read -> "io.read"
                //       $r.map_err -> "$r.map_err"
                let full_name = collect_dotted_name(callee.as_ref());
                if let Some(name) = full_name {
                    // Check if the base identifier is a local variable AND there's a dot (method call)
                    let parts: Vec<&str> = name.split('.').collect();
                    let base = parts.first().copied().unwrap_or(&name);
                    // Strip $ prefix from base for local variable lookup
                    let base_unprefixed = base.strip_prefix('$').unwrap_or(base);
                    let is_local = self.locals.iter().any(|n| n.as_ref() == base_unprefixed);
                    if is_local && parts.len() > 1 {
                        // It's a method call on a local value: $x.method()
                        // Compile as OP_CALL_METHOD: load receiver, push args, CALL_METHOD
                        let method_name = parts.last().copied().unwrap_or(&name);
                        let field_idx = self.intern(method_name);
                        // Load the receiver (the local variable)
                        if let Some(slot) = self
                            .locals
                            .iter()
                            .position(|n| n.as_ref() == base_unprefixed)
                            .map(|i| i as u32)
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        // Push args
                        for a in args {
                            self.node(&a.value);
                        }
                        self.emit(Instr::a2(OP_CALL_METHOD, field_idx, args.len() as u16));
                    } else if is_local && parts.len() == 1 {
                        // Single-part local variable call: $f() or $f arg
                        if let Some(slot) = self
                            .locals
                            .iter()
                            .position(|n| **n == *base)
                            .map(|i| i as u32)
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        for a in args {
                            self.node(&a.value);
                        }
                        self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                    } else {
                        // It's a domain call: net.tcp.connect or io.read
                        let i = self.intern(&name);
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        for a in args {
                            self.node(&a.value);
                        }
                        self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                    }
                } else if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // If the name was originally a $-prefixed variable, load from local
                    // Otherwise push the name string for global/builtin lookup
                    if let Some(slot) = self
                        .locals
                        .iter()
                        .position(|n| **n == *name)
                        .map(|i| i as u32)
                    {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    } else {
                        let i = self.intern(name);
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                    }
                    for a in args {
                        self.node(&a.value);
                    }
                    self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                } else {
                    self.node(callee);
                    for a in args {
                        self.node(&a.value);
                    }
                    self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                }
            }

            AstNode::Int { value, .. } => {
                if (0..=u32::MAX as i64).contains(value) {
                    self.emit(Instr::a1(OP_PUSH_INT, *value as u32));
                } else {
                    let idx = self.wide(*value);
                    self.emit(Instr::a1(OP_PUSH_INT_WIDE, idx));
                }
            }
            AstNode::Float { value, .. } => {
                let idx = self.wide(value.to_bits() as i64);
                self.emit(Instr::a1(OP_PUSH_FLOAT, idx));
            }
            AstNode::Str { value, .. } => {
                let i = self.intern(value);
                self.emit(Instr::a1(OP_PUSH_STR, i));
            }
            AstNode::Bool { value, .. } => {
                self.emit(Instr::none(if *value {
                    OP_PUSH_TRUE
                } else {
                    OP_PUSH_FALSE
                }));
            }
            AstNode::Nil { .. } => self.emit(Instr::none(OP_PUSH_NIL)),

            AstNode::Ident { name, .. } => {
                // Check if this is a registered global (enum variant, type name)
                if self.is_global(name) {
                    let g_slot = self.global(name);
                    self.emit(Instr::a1(OP_LOAD_GLOBAL, g_slot));
                } else {
                    let slot = self.local(name);
                    if !self.try_fuse_load_load(slot) {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    }
                }
            }

            AstNode::BinaryOp {
                left, op, right, ..
            } => {
                self.node(left);
                self.node(right);
                // Try superinstructions AFTER emitting operands
                match op {
                    BinaryOperator::Add => {
                        if self.try_fuse_add_local_local() {
                            return;
                        }
                    }
                    BinaryOperator::Sub => {
                        if self.try_fuse_local_local(OP_SUB_LOCAL_LOCAL) {
                            return;
                        }
                    }
                    BinaryOperator::Mul => {
                        if self.try_fuse_local_local(OP_MUL_LOCAL_LOCAL) {
                            return;
                        }
                    }
                    BinaryOperator::Div => {
                        if self.try_fuse_local_local(OP_DIV_LOCAL_LOCAL) {
                            return;
                        }
                    }
                    _ => {}
                }
                let opcode = match op {
                    BinaryOperator::Add => OP_ADD,
                    BinaryOperator::Sub => OP_SUB,
                    BinaryOperator::Mul => OP_MUL,
                    BinaryOperator::Div => OP_DIV,
                    BinaryOperator::Mod => OP_MOD,
                    BinaryOperator::Eq => OP_EQ,
                    BinaryOperator::NotEq => OP_NEQ,
                    BinaryOperator::Lt => OP_LT,
                    BinaryOperator::Gt => OP_GT,
                    BinaryOperator::Le => OP_LE,
                    BinaryOperator::Ge => OP_GE,
                    BinaryOperator::And => OP_AND,
                    BinaryOperator::Or => OP_OR,
                    BinaryOperator::Coalesce => OP_COALESCE,
                    BinaryOperator::In => OP_IN,
                };
                self.emit(Instr::none(opcode));
            }

            AstNode::UnaryOp { op, operand, .. } => {
                self.node(operand);
                let opcode = match op {
                    UnaryOperator::Neg => OP_NEG,
                    UnaryOperator::Not => OP_NOT,
                };
                self.emit(Instr::none(opcode));
            }

            AstNode::ArrayLit { elements, .. } => {
                for e in elements {
                    self.node(e);
                }
                self.emit(Instr::a1(OP_MAKE_ARRAY, elements.len() as u32));
            }

            AstNode::DictLit { entries, .. } => {
                for (k, v) in entries {
                    let i = self.intern(k);
                    self.emit(Instr::a1(OP_PUSH_STR, i));
                    self.node(v);
                }
                self.emit(Instr::a1(OP_MAKE_DICT, entries.len() as u32));
            }

            AstNode::Block { statements, .. } => {
                for s in statements {
                    self.node(s);
                }
            }

            AstNode::Index { target, index, .. } => {
                self.node(target);
                self.node(index);
                self.emit(Instr::none(OP_GET_INDEX));
            }

            AstNode::FieldAccess { target, field, .. } => {
                self.node(target);
                let i = self.intern(field);
                self.emit(Instr::a1(OP_GET_FIELD, i));
            }

            AstNode::Range { start, end, .. } => {
                let i = self.intern("range");
                self.emit(Instr::a1(OP_PUSH_STR, i));
                if let Some(s) = start {
                    self.node(s);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                if let Some(e) = end {
                    self.node(e);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                self.emit(Instr::a2(OP_CALL, 2, 0));
            }

            AstNode::Lambda {
                params,
                body,
                implicit_it,
                ..
            } => {
                let mut sub = Compiler::new();
                // If implicit_it is true, add "it" as the first parameter
                if *implicit_it {
                    sub.local("it");
                }
                for p in params {
                    sub.local(p);
                }
                sub.node(body);
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                // Build params list with implicit "it" prepended if needed
                let all_params = if *implicit_it {
                    let mut p = vec!["it".to_string()];
                    p.extend(params.clone());
                    p
                } else {
                    params.clone()
                };
                let f = FuncValue {
                    name: "lambda".into(),
                    bytecode: Some(Arc::new(bc)),
                    params: all_params,
                    closure_captures: Vec::new(),
                    closure_env: Vec::new(),
                };
                let idx = self.closures.len() as u32;
                self.closures.push(f);
                self.emit(Instr::a1(OP_LOAD_LAMBDA, idx));
            }

            AstNode::Pipeline { input, stages, .. } => {
                self.node(input);
                for stage in stages {
                    let i = self.intern(&stage.name);
                    self.emit(Instr::a1(OP_PUSH_STR, i));
                    if let Some(a) = &stage.arg {
                        self.node(a);
                    } else {
                        self.emit(Instr::none(OP_PUSH_NIL));
                    }
                    self.emit(Instr::a2(OP_CALL, 2, 0));
                }
            }

            // Type definition: compile body (fields + methods), register type name as a global
            AstNode::TypeDef {
                name, kind, body, ..
            } => {
                // Compile any methods in the body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Register the type name as a global
                let type_idx = self.intern(name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                let g_slot = self.global(name);
                self.emit(Instr::a1(OP_STORE_GLOBAL, g_slot));
                // For sum types, register each variant as a global string constant
                if let TypeKind::Sum { variants } = kind {
                    for v in variants {
                        let v_idx = self.intern(&v.name);
                        self.emit(Instr::a1(OP_PUSH_STR, v_idx));
                        let vg_slot = self.global(&v.name);
                        self.emit(Instr::a1(OP_STORE_GLOBAL, vg_slot));
                    }
                }
            }

            // Impl block: compile methods, associate with type
            AstNode::ImplDef {
                type_name,
                trait_name,
                body,
                ..
            } => {
                // Compile all methods in the impl body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Store the impl relationship as a global marker
                let impl_key = format!("__impl_{}_{}", type_name, trait_name);
                let idx = self.intern(&impl_key);
                self.emit(Instr::a1(OP_PUSH_STR, idx));
                self.emit(Instr::none(OP_PUSH_TRUE));
            }

            // Type test: $x is TypeName
            AstNode::TypeTest {
                value, type_name, ..
            } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_TEST));
            }
        }
    }
}

/// Recursively collect a dotted name from a chain of FieldAccess nodes.
/// e.g., FieldAccess(FieldAccess(Ident("net"), "tcp"), "connect") -> Some("net.tcp.connect")
/// Returns None if the chain contains anything other than Ident + FieldAccess.
fn collect_dotted_name(node: &AstNode) -> Option<String> {
    match node {
        AstNode::Ident { name, .. } => Some(name.clone()),
        AstNode::FieldAccess { target, field, .. } => {
            let base = collect_dotted_name(target)?;
            Some(format!("{}.{}", base, field))
        }
        _ => None,
    }
}

/// Compiles an Abstract Syntax Tree (AST) slice into an executable [`Bytecode`] bundle.
///
/// Translates AST nodes into 64-bit [`Instr`] instructions, performs string interning,
/// registers wide numeric operands, and runs the peephole optimizer.
///
/// # Arguments
///
/// * `ast` - A slice of top-level [`AstNode`] statements to compile.
///
/// # Returns
///
/// A fully optimized [`Bytecode`] ready for execution by [`crate::vm::VM::run`].
///
/// # Examples
///
/// ```
/// use xcl_vm::lexer::lex;
/// use xcl_vm::parser::parse;
/// use xcl_vm::bytecode::compile;
///
/// let tokens = lex("$x = 10; $x + 5", "example.xcl");
/// let ast = parse(&tokens, "$x = 10; $x + 5");
/// let bytecode = compile(&ast);
///
/// assert!(!bytecode.instructions.is_empty());
/// ```
pub fn compile(ast: &[AstNode]) -> Bytecode {
    let mut c = Compiler::new();
    c.compile(ast)
}

/// Peephole optimizer — scans bytecode and fuses common patterns.
/// This runs after compilation and catches patterns the AST-level fusion misses.
fn peephole_optimize_once(code: &mut Vec<Instr>, _wide: &[i64]) {
    let mut i = 0;

    while i < code.len() {
        let matched = match code.get(i).copied() {
            Some(instr) => peephole_at(code, i, instr),
            None => false,
        };
        if !matched {
            i += 1;
        }
        // If matched, stay at same index to catch overlapping patterns
    }
}

/// Try to match and fuse a pattern at instruction index `i`.
/// Returns true if a fusion occurred (caller should not advance `i`).
#[inline]
fn peephole_at(code: &mut Vec<Instr>, i: usize, instr: Instr) -> bool {
    let op = instr.op();

    // Pattern 1: LOAD_LOCAL a; LOAD_LOCAL b; ADD → ADD_LOCAL_LOCAL a, b
    // Only fuse if STORE_LOCAL immediately follows (the fused op pushes to stack,
    // and STORE_LOCAL will consume it). Otherwise the caller expects the value on the stack.
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_LOAD_LOCAL && next3.op() == OP_STORE_LOCAL {
            let arith_op = match next2.op() {
                OP_ADD => Some(OP_ADD_LOCAL_LOCAL),
                OP_SUB => Some(OP_SUB_LOCAL_LOCAL),
                OP_MUL => Some(OP_MUL_LOCAL_LOCAL),
                OP_DIV => Some(OP_DIV_LOCAL_LOCAL),
                OP_LT => Some(OP_LT_LOCAL_LOCAL),
                _ => None,
            };
            if let Some(fused) = arith_op {
                code[i] = Instr::make(fused, instr.a(), next.a() as u16, 0);
                code.drain(i + 1..=i + 2);
                return true;
            }
        }
    }

    // Pattern 2: LOAD_LOCAL a; PUSH_INT k; ADD → ADD_INT_LOCAL a, k
    // Only fuse if STORE_LOCAL immediately follows. Otherwise the caller expects the value on the stack.
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT && next3.op() == OP_STORE_LOCAL {
            let arith_op = match next2.op() {
                OP_ADD => Some(OP_ADD_INT_LOCAL),
                OP_SUB => Some(OP_SUB_INT_LOCAL),
                OP_MUL => Some(OP_MUL_INT_LOCAL),
                _ => None,
            };
            if let Some(fused) = arith_op {
                code[i] = Instr::a2(fused, instr.a(), next.a() as u16);
                code.drain(i + 1..=i + 2);
                return true;
            }
        }
    }

    // Pattern 3: PUSH_INT a; PUSH_INT b; ADD → PUSH_INT (a+b) (constant folding)
    if op == OP_PUSH_INT && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_PUSH_INT {
            let a = instr.a() as i64;
            let b = next.a() as i64;
            let result = match next2.op() {
                OP_ADD => Some(a + b),
                OP_SUB => Some(a - b),
                OP_MUL => Some(a * b),
                OP_DIV if b != 0 => Some(a / b),
                _ => None,
            };
            if let Some(r) = result {
                if (0..=u32::MAX as i64).contains(&r) {
                    code[i] = Instr::a1(OP_PUSH_INT, r as u32);
                    code.drain(i + 1..=i + 2);
                    return true;
                }
            }
        }
    }

    // Pattern 4: STORE_LOCAL a; LOAD_LOCAL a → (delete the LOAD, duplicate value)
    // This only works if the LOAD is immediately after the STORE for the same slot
    if op == OP_STORE_LOCAL && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_LOAD_LOCAL && next.a() == instr.a() {
            // Check if the next instruction after LOAD is an arithmetic op that
            // would benefit from having the value on the stack
            // Actually, just delete the redundant LOAD — the STORE already left the value
            // But wait: STORE_LOCAL pops the value, so we need to PUSH it back
            // This pattern is only useful if we can replace STORE; LOAD with just LOAD
            // which means the value was already on the stack before STORE
            // For now, skip this — it's tricky with the stack semantics
        }
    }

    // Pattern 5: LOAD_LOCAL a; STORE_LOCAL a → (delete both, no-op)
    if op == OP_LOAD_LOCAL && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL && next.a() == instr.a() {
            code.drain(i..=i + 1);
            return true;
        }
    }

    // Pattern 6: LOAD_LOCAL a; PUSH_INT 1; ADD; STORE_LOCAL a → INC_LOCAL a
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT
            && next.a() == 1
            && next2.op() == OP_ADD
            && next3.op() == OP_STORE_LOCAL
            && next3.a() == instr.a()
        {
            code[i] = Instr::a1(OP_INC_LOCAL, instr.a());
            code.drain(i + 1..=i + 3);
            return true;
        }
    }

    // Pattern 7: LOAD_LOCAL a; LOAD_LOCAL b; ADD; STORE_LOCAL a → ADD_LOCAL_STORE a, b
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_LOAD_LOCAL && next3.op() == OP_STORE_LOCAL && next3.a() == instr.a() {
            let store_op = match next2.op() {
                OP_ADD => Some(OP_ADD_LOCAL_STORE),
                OP_SUB => Some(OP_SUB_LOCAL_STORE),
                OP_MUL => Some(OP_MUL_LOCAL_STORE),
                _ => None,
            };
            if let Some(fused) = store_op {
                code[i] = Instr::a2(fused, instr.a(), next.a() as u16);
                code.drain(i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 8: LOAD_LOCAL a; LOAD_LOCAL b; LOAD_LOCAL c; ADD; ADD → fused chain
    // (Two consecutive LOAD_LOCAL_LOAD_LOCAL + ADD can be folded)
    if op == OP_LOAD_LOCAL && code.len() >= i + 5 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        let next4 = code[i + 4];
        if next.op() == OP_LOAD_LOCAL
            && next2.op() == OP_LOAD_LOCAL
            && next3.op() == OP_ADD
            && next4.op() == OP_ADD
        {
            // LOAD a; LOAD b; LOAD c; ADD; ADD → ADD_LL a,b; LOAD c; ADD
            code[i] = Instr::make(OP_ADD_LOCAL_LOCAL, instr.a(), next.a() as u16, 0);
            code[i + 1] = next2;
            code[i + 2] = next3;
            code.drain(i + 3..=i + 4);
            return true;
        }
    }

    // Pattern 9: LOAD_LOCAL a; LOAD_LOCAL b; ADD; STORE_LOCAL c → ADD_LOCAL_LOCAL_STORE c, a, b
    // Like Luau's fused store pattern — eliminates 4 instructions into 1
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_LOAD_LOCAL && next3.op() == OP_STORE_LOCAL {
            let store_op = match next2.op() {
                OP_ADD => Some(OP_ADD_LOCAL_LOCAL_STORE),
                OP_SUB => Some(OP_SUB_LOCAL_LOCAL_STORE),
                OP_MUL => Some(OP_MUL_LOCAL_LOCAL_STORE),
                OP_DIV => Some(OP_DIV_LOCAL_LOCAL_STORE),
                _ => None,
            };
            if let Some(fused) = store_op {
                // a=dest, b=src2, c=src1
                code[i] = Instr::make(fused, next3.a(), next.a() as u16, instr.a() as u8);
                code.drain(i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 10: LOAD_LOCAL a; PUSH_INT k; ADD; STORE_LOCAL b → ADD_INT_STORE_LOCAL b, a, k
    // Like Luau's ADDK — constant on the right side
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT && next3.op() == OP_STORE_LOCAL {
            if next2.op() == OP_ADD {
                code[i] = Instr::a2(OP_ADD_INT_STORE_LOCAL, next3.a(), next.a() as u16);
                // encode src local in c
                code[i] = Instr::make(
                    OP_ADD_INT_STORE_LOCAL,
                    next3.a(),
                    next.a() as u16,
                    instr.a() as u8,
                );
                code.drain(i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 11: DEC_LOCAL — LOAD_LOCAL a; PUSH_INT 1; SUB; STORE_LOCAL a
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT
            && next.a() == 1
            && next2.op() == OP_SUB
            && next3.op() == OP_STORE_LOCAL
            && next3.a() == instr.a()
        {
            code[i] = Instr::a1(OP_DEC_LOCAL, instr.a());
            code.drain(i + 1..=i + 3);
            return true;
        }
    }

    // Pattern 12: Dead code elimination — consecutive NOPs
    if op == OP_NOP && code.len() >= i + 2 && code[i + 1].op() == OP_NOP {
        code.drain(i..=i + 1);
        return true;
    }

    false
}
