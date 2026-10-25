//! Register-directed emission.
//!
//! Compiling straight into a slot (`emit_expr_to_slot`) avoids the push/store
//! round trip for the shapes the fused instructions consume directly.
//! `emit_call_args` materialises call arguments — positional values followed
//! by an optional named-argument map — on the operand stack.

use super::opcode_map::*;
use super::*;

impl Compiler<'_> {
    /// Emits a call whose callee is a name, choosing the cheapest encoding.
    ///
    /// `OP_CALL_HOT` when the VM assigned the name a slot, otherwise the fused
    /// `OP_CALL_NAME`. Both consume the same `n` argument values and leave one
    /// result, so the two are interchangeable; only the callee lookup differs.
    #[inline]
    pub(super) fn emit_named_call(&mut self, name: u32, name16: u16, n: usize) {
        match self.hot_slot(name) {
            Some(slot) => self.emit(Instr::make(OP_CALL_HOT, n as u32, slot, 0)),
            None => self.emit(Instr::make(OP_CALL_NAME, n as u32, name16, 0)),
        }
    }

    pub(super) fn can_emit_to_slot(&self, node: &AstNode) -> bool {
        match node {
            AstNode::Ident { name, .. } => {
                !self.is_global(name) && self.root_read_slot(name).is_none()
            }
            AstNode::Int { .. } | AstNode::Float { .. } => true,
            AstNode::UnaryOp {
                op: UnaryOperator::Neg,
                operand,
                ..
            } => self.can_emit_to_slot(operand),
            AstNode::BinaryOp {
                left, op, right, ..
            } if local_local_store_opcode(*op).is_some() => {
                self.can_emit_to_slot(left) && self.can_emit_to_slot(right)
            }
            AstNode::Index { target, index, .. } => {
                // A string-literal index is compiled as an interned field
                // read, so it does not need to be stack-emittable.
                self.can_emit_to_slot(target)
                    && (matches!(index.as_ref(), AstNode::Str { .. })
                        || self.can_emit_to_slot(index))
            }
            AstNode::FieldAccess { target, .. } => {
                self.zero_arg_domain_call(node).is_none() && self.can_emit_to_slot(target)
            }
            _ => false,
        }
    }

    pub(super) fn emit_expr_to_slot(&mut self, node: &AstNode, temp_depth: u32) -> u32 {
        match node {
            AstNode::Ident { name, .. } => {
                if let Some(slot) = self.root_read_slot(name) {
                    // Top-level variable read from a function body: load it and
                    // park the value in this expression's temp slot.
                    let dst = self.temp_slot(temp_depth);
                    self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                    return dst;
                }
                self.local(name)
            }
            AstNode::Int { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                // Inline small values directly in operand b (kind=0)
                // For larger values, store in wide_operands (kind=6)
                if let Ok(v) = u16::try_from(*value) {
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, v, 0));
                } else {
                    let w = self.wide(*value);
                    // Fall back to unfused push + store when wide index overflows u16.
                    if let Ok(wu) = u16::try_from(w) {
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 6));
                    } else {
                        self.emit(Instr::a1(OP_PUSH_INT_WIDE, w));
                        self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                    }
                }
                dst
            }
            AstNode::Float { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                let w = self.wide(value.to_bits() as i64);
                // Fall back to unfused push + store when wide index overflows u16.
                if let Ok(wu) = u16::try_from(w) {
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 1));
                } else {
                    self.emit(Instr::a1(OP_PUSH_FLOAT, w));
                    self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                }
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
                        // Fall back to unfused push + store when wide index overflows u16.
                        if let Ok(wu) = u16::try_from(w) {
                            self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 6));
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT_WIDE, w));
                            self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                        }
                    }
                    return dst;
                }
                if let AstNode::Float { value, .. } = operand.as_ref() {
                    let dst = self.temp_slot(temp_depth);
                    let w = self.wide((-*value).to_bits() as i64);
                    // Fall back to unfused push + store when wide index overflows u16.
                    if let Ok(wu) = u16::try_from(w) {
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 1));
                    } else {
                        self.emit(Instr::a1(OP_PUSH_FLOAT, w));
                        self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                    }
                    return dst;
                }
                let zero_slot = self.temp_slot(temp_depth);
                self.emit(Instr::make(OP_LOAD_CONST_LOCAL, zero_slot, 0, 0));
                let src = self.emit_expr_to_slot(operand, temp_depth + 1);
                // Fall back to unfused sequence when operands overflow.
                if let (Ok(src16), Ok(z8)) = (u16::try_from(src), u8::try_from(zero_slot)) {
                    self.emit(Instr::make(OP_SUB_LOCAL_LOCAL_STORE, zero_slot, src16, z8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, zero_slot));
                    self.emit(Instr::a1(OP_LOAD_LOCAL, src));
                    self.emit(Instr::none(OP_SUB));
                    self.emit(Instr::a1(OP_STORE_LOCAL, zero_slot));
                }
                zero_slot
            }
            AstNode::Index { target, index, .. } => {
                // `$obj["literal"]` is exactly a field read: intern the literal
                // and use the field opcode instead of materializing the key on
                // the operand stack first.
                if let AstNode::Str { value, .. } = index.as_ref() {
                    let t_slot = self.emit_expr_to_slot(target, temp_depth);
                    let field_idx = self.intern(value);
                    let dst = self.temp_slot(temp_depth);
                    // Fall back to unfused sequence when operands overflow.
                    if let (Ok(fi16), Ok(t8)) = (u16::try_from(field_idx), u8::try_from(t_slot)) {
                        self.emit(Instr::make(OP_GET_FIELD_LOCAL_STORE, dst, fi16, t8));
                    } else {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                        self.emit(Instr::a1(OP_GET_FIELD, field_idx));
                        self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                    }
                    return dst;
                }
                let t_slot = self.emit_expr_to_slot(target, temp_depth);
                let i_slot = self.emit_expr_to_slot(index, temp_depth + 1);
                let dst = self.temp_slot(temp_depth);
                // Fall back to unfused sequence when operands overflow.
                if let (Ok(is16), Ok(t8)) = (u16::try_from(i_slot), u8::try_from(t_slot)) {
                    self.emit(Instr::make(OP_GET_INDEX_LOCAL_LOCAL_STORE, dst, is16, t8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                    self.emit(Instr::a1(OP_LOAD_LOCAL, i_slot));
                    self.emit(Instr::none(OP_GET_INDEX));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                }
                dst
            }
            AstNode::FieldAccess { target, field, .. } => {
                if let Some(name) = self.zero_arg_domain_call(node) {
                    let i = self.intern(&name);
                    if let Ok(name16) = u16::try_from(i) {
                        self.emit_named_call(i, name16, 0);
                    } else {
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        self.emit(Instr::a2(OP_CALL, 0, 0));
                    }
                    let dst = self.temp_slot(temp_depth);
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                    return dst;
                }
                let t_slot = self.emit_expr_to_slot(target, temp_depth);
                let field_idx = self.intern(field);
                let dst = self.temp_slot(temp_depth);
                // A name that is also a function may be a zero-argument method
                // call; only `OP_GET_FIELD` falls back to dispatch.
                if self.field_may_be_method(field) {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                    self.emit(Instr::a1(OP_GET_FIELD, field_idx));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                } else if let (Ok(fi16), Ok(t8)) = (u16::try_from(field_idx), u8::try_from(t_slot))
                {
                    // Fall back to unfused sequence when operands overflow.
                    self.emit(Instr::make(OP_GET_FIELD_LOCAL_STORE, dst, fi16, t8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                    self.emit(Instr::a1(OP_GET_FIELD, field_idx));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                }
                dst
            }
            AstNode::BinaryOp {
                left, op, right, ..
            } => {
                let l_slot = self.emit_expr_to_slot(left, temp_depth);
                let r_slot = self.emit_expr_to_slot(right, temp_depth + 1);
                let dst = self.temp_slot(temp_depth);
                let Some(opc) = local_local_store_opcode(*op) else {
                    unreachable!()
                };
                // Fall back to unfused sequence when operands overflow.
                if let (Ok(rs16), Ok(ls8)) = (u16::try_from(r_slot), u8::try_from(l_slot)) {
                    self.emit(Instr::make(opc, dst, rs16, ls8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, l_slot));
                    self.emit(Instr::a1(OP_LOAD_LOCAL, r_slot));
                    self.emit(Instr::none(binop_opcode(*op)));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                }
                dst
            }
            _ => unreachable!(),
        }
    }

    /// Emits `local[a] ± int` as a single push when a call argument has that
    /// shape. Returns `true` if the fused opcode was emitted.
    /// Emits call arguments, fusing `local ± int` into a single push.
    /// Emits call arguments and returns the argument count the callee receives.
    ///
    /// Positional arguments go first, in source order. Named arguments are
    /// packed into one trailing map (`name: value` pairs), which natives split
    /// with `split_named_args`. The map counts as one argument.
    pub(super) fn emit_call_args(&mut self, args: &[CallArg]) -> usize {
        let mut positional = 0;
        for a in args {
            if a.name.is_none() {
                positional += 1;
                if !self.try_emit_arg_fused(&a.value) {
                    self.node(&a.value);
                }
            }
        }
        let named = args.iter().filter(|a| a.name.is_some()).count();
        if named == 0 {
            return positional;
        }
        for a in args {
            if let Some(name) = &a.name {
                let key = self.intern(name);
                self.emit(Instr::a1(OP_PUSH_STR, key));
                self.node(&a.value);
            }
        }
        self.emit(Instr::a1(OP_MAKE_DICT, named as u32));
        positional + 1
    }

    pub(super) fn try_emit_arg_fused(&mut self, node: &AstNode) -> bool {
        let AstNode::BinaryOp {
            left, op, right, ..
        } = node
        else {
            return false;
        };
        let AstNode::Ident { name, .. } = left.as_ref() else {
            return false;
        };
        let AstNode::Int { value, .. } = right.as_ref() else {
            return false;
        };
        if self.is_global(name) || !(0..=u16::MAX as i64).contains(value) {
            return false;
        }
        let Some(slot) = self.locals.iter().position(|n| &**n == name) else {
            return false;
        };
        let kind = match op {
            BinaryOperator::Add => 0u8,
            BinaryOperator::Sub => 1u8,
            BinaryOperator::Mul => 2u8,
            _ => return false,
        };
        self.emit(Instr::make(
            OP_PUSH_LOCAL_INT_OP,
            slot as u32,
            *value as u16,
            kind,
        ));
        true
    }
}
