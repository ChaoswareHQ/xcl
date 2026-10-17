#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(not(feature = "std"))]
use alloc::vec;
#[cfg(feature = "std")]
use std::sync::Arc;

use crate::ast::{AstNode, BinaryOperator, UnaryOperator, TypeKind};
use crate::value::FuncValue;

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Instr(pub u64);

impl Instr {
    #[inline(always)] pub const fn op(self) -> u8 { (self.0 & 0xFF) as u8 }
    #[inline(always)] pub const fn a(self)  -> u32 { ((self.0 >> 8)  & 0xFFFF_FFFF) as u32 }
    #[inline(always)] pub const fn b(self)  -> u16 { ((self.0 >> 40) & 0xFFFF) as u16 }
    #[inline(always)] pub const fn c(self)  -> u8  { ((self.0 >> 56) & 0xFF) as u8 }

    #[inline(always)]
    pub const fn make(op: u8, a: u32, b: u16, c: u8) -> Self {
        Instr((op as u64) | ((a as u64) << 8) | ((b as u64) << 40) | ((c as u64) << 56))
    }

    #[inline(always)] pub const fn none(op: u8)               -> Self { Self::make(op, 0, 0, 0) }
    #[inline(always)] pub const fn a1(op: u8, a: u32)         -> Self { Self::make(op, a, 0, 0) }
    #[inline(always)] pub const fn a2(op: u8, a: u32, b: u16) -> Self { Self::make(op, a, b, 0) }
}

pub const OP_NOP:             u8 = 0;
pub const OP_PUSH_NIL:        u8 = 1;
pub const OP_PUSH_TRUE:       u8 = 2;
pub const OP_PUSH_FALSE:      u8 = 3;
pub const OP_PUSH_INT:        u8 = 4;
pub const OP_PUSH_INT_WIDE:   u8 = 5;
pub const OP_PUSH_FLOAT:      u8 = 6;
pub const OP_PUSH_STR:        u8 = 7;
pub const OP_LOAD_LOCAL:      u8 = 8;
pub const OP_STORE_LOCAL:     u8 = 9;
pub const OP_LOAD_GLOBAL:     u8 = 10;
pub const OP_STORE_GLOBAL:    u8 = 11;

pub const OP_ADD:             u8 = 12;
pub const OP_SUB:             u8 = 13;
pub const OP_MUL:             u8 = 14;
pub const OP_DIV:             u8 = 15;
pub const OP_MOD:             u8 = 16;
pub const OP_NEG:             u8 = 17;

pub const OP_EQ:              u8 = 18;
pub const OP_NEQ:             u8 = 19;
pub const OP_LT:              u8 = 20;
pub const OP_GT:              u8 = 21;
pub const OP_LE:              u8 = 22;
pub const OP_GE:              u8 = 23;
pub const OP_AND:             u8 = 24;
pub const OP_OR:              u8 = 25;
pub const OP_NOT:             u8 = 26;
pub const OP_IN:              u8 = 27;

pub const OP_JUMP:            u8 = 28;
pub const OP_JUMP_IF_FALSE:   u8 = 29;
pub const OP_JUMP_IF_NIL:     u8 = 30;

pub const OP_CALL:            u8 = 32;
pub const OP_RETURN:          u8 = 33;
pub const OP_HALT:            u8 = 34;

pub const OP_MAKE_ARRAY:      u8 = 35;
pub const OP_MAKE_DICT:       u8 = 36;
pub const OP_GET_INDEX:       u8 = 37;
pub const OP_GET_FIELD:       u8 = 38;
pub const OP_ARRAY_PUSH:      u8 = 39;
pub const OP_LEN:             u8 = 41;

pub const OP_FOR_ITER_START:  u8 = 42;
pub const OP_FOR_ITER_NEXT:   u8 = 43;
pub const OP_FOR_RANGE_START: u8 = 44;
pub const OP_FOR_RANGE_NEXT:  u8 = 45;

pub const OP_LOAD_LAMBDA:     u8 = 46;

pub const OP_COALESCE:        u8 = 47;

/// Type test: check if a value is of a given type (by tag field or type index)
pub const OP_TYPE_TEST:         u8 = 52;

pub const OP_LOAD_LOCAL_LOAD_LOCAL: u8 = 60;
pub const OP_ADD_LOCAL_LOCAL:       u8 = 61;
pub const OP_SUB_LOCAL_LOCAL:       u8 = 62;
pub const OP_LT_LOCAL_LOCAL:        u8 = 63;
pub const OP_LOAD_LOCAL_PUSH_INT:   u8 = 64;
pub const OP_LOAD_LOCAL_STORE_LOCAL:u8 = 65;

/// Call a method on a value by looking up its type's domain.
/// Stack: receiver, arg1, arg2, ... -> result
/// Operand a = field name string index, b = arg count (excluding receiver)
pub const OP_CALL_METHOD: u8 = 66;

/// Integer-specialized arithmetic — bypasses Value type checking
pub const OP_ADD_INT:       u8 = 70;
pub const OP_SUB_INT:       u8 = 71;
pub const OP_MUL_INT:       u8 = 72;
pub const OP_LT_INT:        u8 = 73;
pub const OP_GT_INT:        u8 = 74;
pub const OP_EQ_INT:        u8 = 75;

/// Superinstructions for hot patterns
pub const OP_INC_LOCAL:           u8 = 80;  /// $x = $x + 1
pub const OP_DEC_LOCAL:           u8 = 81;  /// $x = $x - 1
pub const OP_MUL_LOCAL_LOCAL:     u8 = 82;  /// $a * $b
pub const OP_DIV_LOCAL_LOCAL:     u8 = 83;  /// $a / $b
pub const OP_ADD_INT_STORE_LOCAL: u8 = 84;  /// $a = $b + c (a=dest, b=local, c=int)
pub const OP_INC_LOCAL_INT:       u8 = 85;  /// $a = $a + c (a=local, c=int from b)

/// Single-opcode numeric for-loop (like Luau's FORNLOOP)
/// Operand a = local slot index, b = jump offset (negative = backward), c = step (default 1)
/// This replaces: FOR_RANGE_NEXT + STORE_LOCAL + body + JUMP
pub const OP_FOR_NUMBER_LOOP:     u8 = 90;

/// Fused: load local + add int constant + store back (for $x = $x + c pattern)
/// Operand a = local slot, b = int constant
pub const OP_ADD_INT_LOCAL:       u8 = 91;

/// Fused: load two locals + add + store to first (for $a = $a + $b pattern)
/// Operand a = dest slot, b = src slot
pub const OP_ADD_LOCAL_STORE:     u8 = 92;

/// Fused: load local + sub int constant + store back
/// Operand a = local slot, b = int constant
pub const OP_SUB_INT_LOCAL:       u8 = 93;

/// Fused: load local + mul int constant + store back
/// Operand a = local slot, b = int constant
pub const OP_MUL_INT_LOCAL:       u8 = 94;

/// Fused while-loop condition + jump: while $a < b { ... }
/// Operand a = local slot, b = int constant to compare against
/// If locals[a] < b, continue (Action::Next), else jump past body
/// The body follows, ending with OP_JUMP back to this instruction
pub const OP_WHILE_LT_INT:        u8 = 95;

/// Fused: load local + mul local + store back (for $zr = $zr * $zr pattern)
/// Operand a = dest slot, b = src slot
pub const OP_MUL_LOCAL_STORE:     u8 = 96;

/// Fused: load local + div local + store back (for $a = $a / $b pattern)
/// Operand a = dest slot, b = src slot
pub const OP_DIV_LOCAL_STORE:     u8 = 97;

/// Fused: load local + sub local + store back (for $a = $a - $b pattern)
/// Operand a = dest slot, b = src slot
pub const OP_SUB_LOCAL_STORE:     u8 = 98;

/// Fused: load local + add local + store to third (for $c = $a + $b pattern)
/// Operand a = dest slot, b = src2 slot, c = src1 slot
pub const OP_ADD_LOCAL_LOCAL_STORE: u8 = 99;

/// Fused: load local + mul local + store to third (for $c = $a * $b pattern)
/// Operand a = dest slot, b = src2 slot, c = src1 slot
pub const OP_MUL_LOCAL_LOCAL_STORE: u8 = 100;

/// Fused: load local + sub local + store to third (for $c = $a - $b pattern)
/// Operand a = dest slot, b = src2 slot, c = src1 slot
pub const OP_SUB_LOCAL_LOCAL_STORE: u8 = 101;

/// Fused: load local + div local + store to third (for $c = $a / $b pattern)
/// Operand a = dest slot, b = src2 slot, c = src1 slot
pub const OP_DIV_LOCAL_LOCAL_STORE: u8 = 102;

/// Fused while-loop condition + jump: while $a < $b { ... } (both locals)
/// Operand a = local slot (left), b = local slot (right), c = exit_wide_idx
pub const OP_WHILE_LT_LOCAL: u8 = 103;

#[derive(Debug, Clone)]
pub struct Bytecode {
    pub instructions:  Arc<[Instr]>,
    pub wide_operands: Arc<[i64]>,
    pub string_table:  Arc<[Box<str>]>,
    pub local_names:   Arc<[Box<str>]>,
    pub global_names:  Arc<[Box<str>]>,
    pub user_functions: Vec<(Box<str>, FuncValue)>,
    pub closures:      Vec<FuncValue>,
}

struct Compiler {
    code:      Vec<Instr>,
    wide:      Vec<i64>,
    strings:   Vec<Box<str>>,
    locals:    Vec<Box<str>>,
    globals:   Vec<Box<str>>,
    functions: Vec<(Box<str>, FuncValue)>,
    closures:  Vec<FuncValue>,
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

    #[inline(always)] fn emit(&mut self, i: Instr) { self.code.push(i); }
    #[inline(always)] fn ip(&self) -> u32 { self.code.len() as u32 }

    fn patch_jump(&mut self, at: u32, target: u32) {
        let i = &mut self.code[at as usize];
        *i = Instr::make(i.op(), target, i.b(), i.c());
    }

    #[inline]
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| &**x == s) { return i as u32; }
        let i = self.strings.len() as u32;
        self.strings.push(s.into());
        i
    }

    #[inline]
    fn local(&mut self, name: &str) -> u32 {
        if let Some(i) = self.locals.iter().position(|x| &**x == name) { return i as u32; }
        let i = self.locals.len() as u32;
        self.locals.push(name.into());
        i
    }

    #[inline]
    fn global(&mut self, name: &str) -> u32 {
        if let Some(i) = self.globals.iter().position(|x| &**x == name) { return i as u32; }
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

    /// Try to fuse a nested binary expression into superinstructions.
    /// For example, `$zr = $zr*$zr - $zi*$zi + $cr` becomes:
    ///   MUL_LL_ST(temp1, zr, zr)
    ///   MUL_LL_ST(temp2, zi, zi)
    ///   SUB_LL_ST(temp1, temp1, temp2)
    ///   ADD_LL_ST(zr, temp1, cr)
    fn try_emit_binary_fused(&mut self, dest_slot: u32, node: &AstNode) -> bool {
        let AstNode::BinaryOp { left, op, right, .. } = node else { return false };

        // Case 1: $dest = $dest op $b (simple self-assignment)
        if let AstNode::Ident { name, .. } = left.as_ref() {
            if self.local(name) == dest_slot {
                if let AstNode::Ident { name: rn, .. } = right.as_ref() {
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
        if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. })
            = (left.as_ref(), right.as_ref())
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
                self.emit(Instr::make(opc, dest_slot, b_src2, c_src1));
                return true;
            }
        }

        // Case 3: Nested binary — recursively emit left and right into temps, then combine
        // This handles `$zr = $zr*$zr - $zi*$zi + $cr`
        if let BinaryOperator::Add | BinaryOperator::Sub | BinaryOperator::Mul | BinaryOperator::Div = op {
            // Check if both sides are simple (ident or int) or nested binary
            let left_is_simple = matches!(left.as_ref(), AstNode::Ident { .. } | AstNode::Int { .. });
            let right_is_simple = matches!(right.as_ref(), AstNode::Ident { .. } | AstNode::Int { .. });

            if left_is_simple && right_is_simple {
                // Already handled by Case 1 and 2 above
                return false;
            }

            // For nested expressions, emit left and right into temps, then combine
            // This is a simplified approach: just emit normally and let peephole handle it
            // A full recursive approach would need temp slot allocation
            return false;
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
        if self.code.len() < 2 { return false; }
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
        if self.code.len() < 2 { return false; }
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
        for n in ast { self.node(n); }
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
                if let AstNode::BinaryOp { left, op, right, .. } = value.as_ref() {
                    if let AstNode::Ident { name: left_name, .. } = left.as_ref() {
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
                                        self.emit(Instr::make(OP_INC_LOCAL_INT, slot, *c as u16, 0));
                                        return;
                                    } else if let AstNode::Ident { name: right_name, .. } = right.as_ref() {
                                        // $x = $x + $y → OP_ADD_LOCAL_STORE (direct local-to-local)
                                        let right_slot = self.local(right_name);
                                        self.emit(Instr::a2(OP_ADD_LOCAL_STORE, slot, right_slot as u16));
                                        return;
                                    }
                                }
                                BinaryOperator::Sub => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x - 1 → OP_DEC_LOCAL
                                        self.emit(Instr::a1(OP_DEC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Ident { name: right_name, .. } = right.as_ref() {
                                        // $x = $x - $y → OP_SUB_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        self.emit(Instr::a2(OP_SUB_LOCAL_STORE, slot, right_slot as u16));
                                        return;
                                    }
                                }
                                BinaryOperator::Mul => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x * c → OP_MUL_INT_LOCAL
                                        self.emit(Instr::a2(OP_MUL_INT_LOCAL, slot, *c as u16));
                                        return;
                                    } else if let AstNode::Ident { name: right_name, .. } = right.as_ref() {
                                        // $x = $x * $y → OP_MUL_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        self.emit(Instr::a2(OP_MUL_LOCAL_STORE, slot, right_slot as u16));
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
                    if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. })
                        = (left.as_ref(), right.as_ref())
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
                if let Some(v) = value { self.node(v); }
                else { self.emit(Instr::none(OP_PUSH_NIL)); }
                self.emit(Instr::none(OP_RETURN));
            }
            AstNode::Break { .. } | AstNode::Continue { .. } => {}

            AstNode::If { condition, then_branch, else_branch, .. } => {
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in then_branch { self.node(s); }
                if let Some(e) = else_branch {
                    let j2 = self.ip();
                    self.emit(Instr::a1(OP_JUMP, 0));
                    let t = self.ip();
                    self.patch_jump(j, t);
                    for s in e { self.node(s); }
                    let end = self.ip();
                    self.patch_jump(j2, end);
                } else {
                    let end = self.ip();
                    self.patch_jump(j, end);
                }
            }

            AstNode::For { variable, iterable, body, .. } => {
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
                        for s in body { self.node(s); }
                        // Jump back to FOR_NUMBER_LOOP
                        self.emit(Instr::a1(OP_JUMP, loop_start));
                    } else {
                        // Fallback: use FOR_RANGE_START/NEXT for non-constant ranges
                        if let Some(s) = start { self.node(s); }
                        else { self.emit(Instr::a1(OP_PUSH_INT, 0)); }
                        if let Some(e) = end { self.node(e); }
                        else { self.emit(Instr::a1(OP_PUSH_INT, 0)); }
                        self.emit(Instr::none(OP_FOR_RANGE_START));
                        let start_ip = self.ip();
                        let next = self.ip();
                        self.emit(Instr::a1(OP_FOR_RANGE_NEXT, 0));
                        let var = self.local(variable);
                        self.emit(Instr::a1(OP_STORE_LOCAL, var));
                        for s in body { self.node(s); }
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
                    for s in body { self.node(s); }
                    self.emit(Instr::a1(OP_JUMP, start_ip));
                    let end = self.ip();
                    self.patch_jump(next, end);
                }
            }

            AstNode::While { condition, body, .. } => {
                // Try to fuse: while $i < N { ... } → OP_WHILE_LT_INT
                // Or: while $a < $b { ... } → OP_WHILE_LT_LOCAL
                match condition.as_ref() {
                    AstNode::BinaryOp { left, op: BinaryOperator::Lt, right, .. } => {
                        if let AstNode::Ident { name: left_name, .. } = left.as_ref() {
                            if let AstNode::Int { value: limit, .. } = right.as_ref() {
                                let slot = self.local(left_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(OP_WHILE_LT_INT, slot, *limit as u16, exit_wide as u8));
                                for s in body { self.node(s); }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                return;
                            } else if let AstNode::Ident { name: right_name, .. } = right.as_ref() {
                                let left_slot = self.local(left_name);
                                let right_slot = self.local(right_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(OP_WHILE_LT_LOCAL, left_slot, right_slot as u16, exit_wide as u8));
                                for s in body { self.node(s); }
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
                for s in body { self.node(s); }
                self.emit(Instr::a1(OP_JUMP, start));
                let end = self.ip();
                self.patch_jump(j, end);
            }

            AstNode::FunctionDef { body, name, params, .. } => {
                let mut sub = Compiler::new();
                for p in params { sub.local(&p.name); }
                for s in body { sub.node(s); }
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
                    let is_local = self.locals.iter().any(|n| **n == *base);
                    if is_local && parts.len() > 1 {
                        // It's a method call on a local value: $x.method()
                        // Compile as OP_CALL_METHOD: load receiver, push args, CALL_METHOD
                        let method_name = parts.last().copied().unwrap_or(&name);
                        let field_idx = self.intern(method_name);
                        // Load the receiver (the local variable)
                        if let Some(slot) = self.locals.iter().position(|n| **n == *base).map(|i| i as u32) {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        // Push args
                        for a in args { self.node(&a.value); }
                        self.emit(Instr::a2(OP_CALL_METHOD, field_idx, args.len() as u16));
                    } else if is_local && parts.len() == 1 {
                        // Single-part local variable call: $f() or $f arg
                        if let Some(slot) = self.locals.iter().position(|n| **n == *base).map(|i| i as u32) {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        for a in args { self.node(&a.value); }
                        self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                    } else {
                        // It's a domain call: net.tcp.connect or io.read
                        let i = self.intern(&name);
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        for a in args { self.node(&a.value); }
                        self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                    }
                } else if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // If the name was originally a $-prefixed variable, load from local
                    // Otherwise push the name string for global/builtin lookup
                    if let Some(slot) = self.locals.iter().position(|n| **n == *name).map(|i| i as u32) {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    } else {
                        let i = self.intern(name);
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                    }
                    for a in args { self.node(&a.value); }
                    self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
                } else {
                    self.node(callee);
                    for a in args { self.node(&a.value); }
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
                self.emit(Instr::none(if *value { OP_PUSH_TRUE } else { OP_PUSH_FALSE }));
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

            AstNode::BinaryOp { left, op, right, .. } => {
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
                for e in elements { self.node(e); }
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

            AstNode::Block { statements, .. } => for s in statements { self.node(s); },

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
                if let Some(s) = start { self.node(s); } else { self.emit(Instr::none(OP_PUSH_NIL)); }
                if let Some(e) = end { self.node(e); } else { self.emit(Instr::none(OP_PUSH_NIL)); }
                self.emit(Instr::a2(OP_CALL, 2, 0));
            }

            AstNode::Lambda { params, body, implicit_it, .. } => {
                let mut sub = Compiler::new();
                // If implicit_it is true, add "it" as the first parameter
                if *implicit_it {
                    sub.local("it");
                }
                for p in params { sub.local(p); }
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
                    if let Some(a) = &stage.arg { self.node(a); }
                    else { self.emit(Instr::none(OP_PUSH_NIL)); }
                    self.emit(Instr::a2(OP_CALL, 2, 0));
                }
            }

            // Type definition: compile body (fields + methods), register type name as a global
            AstNode::TypeDef { name, kind, body, .. } => {
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
            AstNode::ImplDef { type_name, trait_name, body, .. } => {
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
            AstNode::TypeTest { value, type_name, .. } => {
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
                OP_LT  => Some(OP_LT_LOCAL_LOCAL),
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
        if next.op() == OP_PUSH_INT && next.a() == 1 && next2.op() == OP_ADD
            && next3.op() == OP_STORE_LOCAL && next3.a() == instr.a()
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
        if next.op() == OP_LOAD_LOCAL && next2.op() == OP_LOAD_LOCAL
            && next3.op() == OP_ADD && next4.op() == OP_ADD
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
                code[i] = Instr::make(OP_ADD_INT_STORE_LOCAL, next3.a(), next.a() as u16, instr.a() as u8);
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
        if next.op() == OP_PUSH_INT && next.a() == 1 && next2.op() == OP_SUB
            && next3.op() == OP_STORE_LOCAL && next3.a() == instr.a()
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