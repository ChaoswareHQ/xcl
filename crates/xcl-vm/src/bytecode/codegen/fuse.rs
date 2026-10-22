//! Superinstruction fusion.
//!
//! Each helper matches the tail of the instruction stream against a pattern
//! the VM has a fused opcode for and rewrites it in place, returning `true`
//! when it did. A miss leaves the stream untouched, so callers emit the
//! unfused sequence as a fallback.

use super::opcode_map::*;
use super::*;

impl Compiler<'_> {
    /// Try to fuse a nested binary expression into superinstructions.
    /// For example, `$zr = $zr*$zr - $zi*$zi + $cr` becomes:
    ///   MUL_LL_ST(temp1, zr, zr)
    ///   MUL_LL_ST(temp2, zi, zi)
    ///   SUB_LL_ST(temp1, temp1, temp2)
    ///   ADD_LL_ST(zr, temp1, cr)
    pub(super) fn try_emit_binary_fused(&mut self, dest_slot: u32, node: &AstNode) -> bool {
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
                if let Some(opc) = local_store_opcode(*op) {
                    // Only emit fused opcode when rslot fits in u16.
                    if let Ok(r16) = u16::try_from(rslot) {
                        self.emit(Instr::a2(opc, dest_slot, r16));
                        return true;
                    }
                    // Fall through to unfused path.
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
                    if let Some(opc) = int_store_opcode(*op) {
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
            // Only emit fused opcode when both operands fit.
            if let (Ok(l8), Ok(r16)) = (u8::try_from(lslot), u16::try_from(rslot)) {
                if let Some(opc) = local_local_store_opcode(*op) {
                    self.emit(Instr::make(opc, dest_slot, r16, l8));
                    return true;
                }
            }
        }

        // Case 3: Nested 3-address register compilation
        if local_local_store_opcode(*op).is_some()
            && self.locals.len() < 200
            && self.can_emit_to_slot(node)
        {
            if let AstNode::Ident { name: ln, .. } = left.as_ref()
                && !self.is_global(ln)
                && self.local(ln) == dest_slot
                && local_store_opcode(*op).is_some()
            {
                // $dest = $dest + $obj.field  (or $obj["field"]) in one opcode.
                if matches!(op, BinaryOperator::Add) {
                    let field = match right.as_ref() {
                        AstNode::FieldAccess { field, .. } => Some(field),
                        AstNode::Index { index, .. } => match index.as_ref() {
                            AstNode::Str { value, .. } => Some(value),
                            _ => None,
                        },
                        _ => None,
                    };
                    let obj_name = match right.as_ref() {
                        AstNode::FieldAccess { target, .. } => match target.as_ref() {
                            AstNode::Ident { name, .. } => Some(name),
                            _ => None,
                        },
                        AstNode::Index { target, .. } => match target.as_ref() {
                            AstNode::Ident { name, .. } => Some(name),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let (Some(field), Some(obj_name)) = (field, obj_name)
                        && !self.is_global(obj_name)
                    {
                        let obj_slot = self.local(obj_name);
                        // Only emit fused opcode when operands fit.
                        if let (Ok(o8), Ok(fi16)) =
                            (u8::try_from(obj_slot), u16::try_from(self.intern(field)))
                        {
                            self.emit(Instr::make(OP_ADD_FIELD_LOCAL_STORE, dest_slot, fi16, o8));
                            return true;
                        }
                    }
                }

                let r_slot = self.emit_expr_to_slot(right, 0);
                let Some(opc) = local_store_opcode(*op) else {
                    unreachable!()
                };
                // Only emit fused opcode when r_slot fits in u16.
                if let Ok(r16) = u16::try_from(r_slot) {
                    self.emit(Instr::a2(opc, dest_slot, r16));
                    return true;
                }
                // Fall through to unfused path below.
            }

            let l_slot = self.emit_expr_to_slot(left, 0);
            let r_slot = self.emit_expr_to_slot(right, 1);
            // Only emit fused opcode when both operands fit.
            if let (Ok(l8), Ok(r16)) = (u8::try_from(l_slot), u16::try_from(r_slot)) {
                let Some(opc) = local_local_store_opcode(*op) else {
                    unreachable!()
                };
                self.emit(Instr::make(opc, dest_slot, r16, l8));
                return true;
            }
        }

        false
    }

    pub(super) fn try_fuse_load_load(&mut self, second: u32) -> bool {
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

    /// Check if the last instruction is LL_LL and upgrade it, or two LOAD_LOCALs
    pub(super) fn try_fuse_local_local(&mut self, op: u8) -> bool {
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
}
