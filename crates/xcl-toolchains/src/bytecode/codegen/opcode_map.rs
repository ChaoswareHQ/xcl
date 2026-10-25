//! Operator → opcode tables.
//!
//! Small, total mappings used by emission and fusion. Keeping them in one
//! table makes "which operator has a fused form?" a single lookup per site.

use super::super::opcodes::*;
use crate::compiler::ast::BinaryOperator;

/// Opcode for a general binary expression.
#[inline(always)]
pub(super) fn binop_opcode(op: BinaryOperator) -> u8 {
    match op {
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
        BinaryOperator::BitOr => OP_BITOR,
        BinaryOperator::BitXor => OP_BITXOR,
        BinaryOperator::Pow => OP_POW,
    }
}

/// Opcode for `LOAD_LOCAL_LOAD_LOCAL -> ADD/SUB/MUL/DIV_LOCAL_LOCAL` fusion.
#[inline(always)]
pub(super) fn local_local_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_LOCAL_LOCAL),
        BinaryOperator::Sub => Some(OP_SUB_LOCAL_LOCAL),
        BinaryOperator::Mul => Some(OP_MUL_LOCAL_LOCAL),
        BinaryOperator::Div => Some(OP_DIV_LOCAL_LOCAL),
        _ => None,
    }
}

/// Opcode for `$dest = $dest op $rhs` (local-store form).
#[inline(always)]
pub(super) fn local_store_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_LOCAL_STORE),
        BinaryOperator::Sub => Some(OP_SUB_LOCAL_STORE),
        BinaryOperator::Mul => Some(OP_MUL_LOCAL_STORE),
        BinaryOperator::Div => Some(OP_DIV_LOCAL_STORE),
        _ => None,
    }
}

/// Arithmetic code for `OP_LOCAL_OP_TOP` (`$dest = $dest op <stack value>`).
///
/// Shares the vocabulary of the fused field updates: 0 add, 1 sub, 2 mul.
#[inline(always)]
pub(super) fn top_local_op(op: BinaryOperator) -> Option<u8> {
    use crate::runtime::opcode::field_op;
    match op {
        BinaryOperator::Add => Some(field_op::ADD),
        BinaryOperator::Sub => Some(field_op::SUB),
        BinaryOperator::Mul => Some(field_op::MUL),
        _ => None,
    }
}

/// Opcode for `$dest = $lhs op $rhs` (local-local-store form).
#[inline(always)]
pub(super) fn local_local_store_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_LOCAL_LOCAL_STORE),
        BinaryOperator::Sub => Some(OP_SUB_LOCAL_LOCAL_STORE),
        BinaryOperator::Mul => Some(OP_MUL_LOCAL_LOCAL_STORE),
        BinaryOperator::Div => Some(OP_DIV_LOCAL_LOCAL_STORE),
        BinaryOperator::Mod => Some(OP_MOD_LOCAL_LOCAL_STORE),
        _ => None,
    }
}

/// Opcode for `$dest = $dest op <integer constant>`.
#[inline(always)]
pub(super) fn int_store_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_INT_LOCAL),
        BinaryOperator::Sub => Some(OP_SUB_INT_LOCAL),
        BinaryOperator::Mul => Some(OP_MUL_INT_LOCAL),
        _ => None,
    }
}
