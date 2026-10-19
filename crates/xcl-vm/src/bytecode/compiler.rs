//! AST-to-bytecode compiler: translates parsed XCL AST nodes into 64-bit packed instructions.
//!
//! The [`Compiler`] struct accumulates instructions, wide operands, string interning,
//! and local/global slot allocation. After compilation, the peephole optimizer and
//! data flow analysis passes are applied before returning the final [`Bytecode`].

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

use super::instr::Instr;
use super::opcodes::{Bytecode, *};
use super::peephole::peephole_optimize_once;
use crate::compiler::ast::{AstNode, BinaryOperator, TypeKind, UnaryOperator};
use crate::runtime::value::{FuncValue, Value};

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
            _ => false,
        }
    }

    fn emit_expr_to_slot(&mut self, node: &AstNode, temp_depth: u32) -> u32 {
        match node {
            AstNode::Ident { name, .. } => self.local(name),
            AstNode::Int { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                // Inline small values directly in operand b (kind=0)
                // For larger values, store in wide_operands (kind=6)
                if u16::try_from(*value).is_ok() {
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, *value as u16, 0));
                } else {
                    let w = self.wide(*value);
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 6));
                }
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
                    let neg = -*value;
                    if u16::try_from(neg).is_ok() {
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, neg as u16, 0));
                    } else {
                        let w = self.wide(neg);
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 6));
                    }
                    return dst;
                }
                if let AstNode::Float { value, .. } = operand.as_ref() {
                    let dst = self.temp_slot(temp_depth);
                    let w = self.wide((-*value).to_bits() as i64);
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, w as u16, 1));
                    return dst;
                }
                let zero_slot = self.temp_slot(temp_depth);
                self.emit(Instr::make(OP_LOAD_CONST_LOCAL, zero_slot, 0, 0));
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
        peephole_optimize_once(&mut code, &mut self.wide);

        // Run data flow analysis to eliminate dead stores
        crate::optimizer::optimize(&mut code, &mut self.wide);

        let strings = core::mem::take(&mut self.strings);
        let string_values: Vec<Value> = strings
            .iter()
            .map(|s| Value::from_str_boxed(s.to_string()))
            .collect();

        Bytecode {
            instructions: code.into(),
            wide_operands: core::mem::take(&mut self.wide).into(),
            string_table: strings.into(),
            string_values: string_values.into(),
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
                // Fast in-place array push: $arr = $arr + [elem]
                if let AstNode::BinaryOp {
                    left,
                    op: BinaryOperator::Add,
                    right,
                    ..
                } = value.as_ref()
                    && let AstNode::Ident { name: ln, .. } = left.as_ref()
                    && ln == name
                    && let AstNode::ArrayLit { elements, .. } = right.as_ref()
                    && elements.len() == 1
                {
                    if self.locals.len() < 200 && self.can_emit_to_slot(&elements[0]) {
                        let elem_slot = self.emit_expr_to_slot(&elements[0], 0);
                        self.emit(Instr::a2(OP_ARRAY_PUSH_LOCAL, slot, elem_slot as u16));
                        return;
                    }
                    self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    self.node(&elements[0]);
                    self.emit(Instr::none(OP_ARRAY_PUSH));
                    self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                    return;
                }
                // Try recursive RHS fusion for nested binary expressions
                // e.g., $zr = $zr*$zr - $zi*$zi + $cr
                if let AstNode::BinaryOp { .. } = value.as_ref() {
                    if self.try_emit_binary_fused(slot, value.as_ref()) {
                        return;
                    }
                }
                // Direct RHS fusion for array index and field access
                if self.locals.len() < 200 && self.can_emit_to_slot(value.as_ref()) {
                    match value.as_ref() {
                        AstNode::Index { target, index, .. } => {
                            let t_slot = self.emit_expr_to_slot(target, 0);
                            let i_slot = self.emit_expr_to_slot(index, 1);
                            if t_slot < 256 {
                                self.emit(Instr::make(
                                    OP_GET_INDEX_LOCAL_LOCAL_STORE,
                                    slot,
                                    i_slot as u16,
                                    t_slot as u8,
                                ));
                                return;
                            }
                        }
                        AstNode::FieldAccess { target, field, .. } => {
                            let t_slot = self.emit_expr_to_slot(target, 0);
                            if t_slot < 256 {
                                let field_idx = self.intern(field);
                                self.emit(Instr::make(
                                    OP_GET_FIELD_LOCAL_STORE,
                                    slot,
                                    field_idx as u16,
                                    t_slot as u8,
                                ));
                                return;
                            }
                        }
                        _ => {}
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
                                        // $x = $x + 1 â†’ OP_INC_LOCAL
                                        self.emit(Instr::a1(OP_INC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x + c â†’ OP_INC_LOCAL_INT
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
                                        // $x = $x + $y â†’ OP_ADD_LOCAL_STORE (direct local-to-local)
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
                                        // $x = $x - 1 â†’ OP_DEC_LOCAL
                                        self.emit(Instr::a1(OP_DEC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x - $y â†’ OP_SUB_LOCAL_STORE
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
                                        // $x = $x * c â†’ OP_MUL_INT_LOCAL
                                        self.emit(Instr::a2(OP_MUL_INT_LOCAL, slot, *c as u16));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x * $y â†’ OP_MUL_LOCAL_STORE
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
                                        // $x = $x / c â†’ handled by general path for now
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
                    // Check if start and end are integer constants â€” if so, use OP_FOR_NUMBER_LOOP
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
                        let start_minus_1 = self.wide(start_i.wrapping_sub(1));
                        self.emit(Instr::a1(OP_PUSH_INT_WIDE, start_minus_1));
                        self.emit(Instr::a1(OP_STORE_LOCAL, var));
                        let end_wide = self.wide(end_i);
                        let exit_wide = self.wide(0);
                        let loop_start = self.ip();
                        self.emit(Instr::make(
                            OP_FOR_NUMBER_LOOP,
                            var,
                            end_wide as u16,
                            exit_wide as u8,
                        ));
                        for s in body {
                            self.node(s);
                        }
                        // Jump back to FOR_NUMBER_LOOP
                        self.emit(Instr::a1(OP_JUMP, loop_start));
                        let exit_ip = self.ip();
                        self.wide[exit_wide as usize] = exit_ip as i64;
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
                // Try to fuse: while $i < N { ... } â†’ OP_WHILE_LT_INT
                // Or: while $a < $b { ... } â†’ OP_WHILE_LT_LOCAL
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
                    BinaryOperator::Shl => OP_SHL,
                    BinaryOperator::Shr => OP_SHR,
                    BinaryOperator::BitAnd => OP_BITAND,
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

            AstNode::Lambda { params, body, .. } => {
                let mut sub = Compiler::new();
                for p in params {
                    sub.local(p);
                }
                sub.node(body);
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                let f = FuncValue {
                    name: "lambda".into(),
                    bytecode: Some(Arc::new(bc)),
                    params: params.clone(),
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

            // Type cast: $x as TypeName
            AstNode::TypeCast {
                value, type_name, ..
            } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_CAST));
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
/// use xcl_vm::compiler::lexer::lex;
/// use xcl_vm::compiler::parser::parse;
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
