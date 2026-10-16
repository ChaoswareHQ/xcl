//! Java standard library call mapping.
//!
//! Maps IR operations to Java standard library calls.

use crate::translate::ir::ops::IrOp;

/// Get the Java method name for an IR operation.
pub fn java_method_name(op: &IrOp) -> &'static str {
    match op {
        IrOp::Add => "+",
        IrOp::Sub => "-",
        IrOp::Mul => "*",
        IrOp::Div => "/",
        IrOp::Rem => "%",
        IrOp::Eq => "==",
        IrOp::Ne => "!=",
        IrOp::Lt => "<",
        IrOp::Le => "<=",
        IrOp::Gt => ">",
        IrOp::Ge => ">=",
        IrOp::And => "&&",
        IrOp::Or => "||",
        IrOp::Not => "!",
        IrOp::Len => ".length",
        IrOp::Concat => ".concat(",
        IrOp::Index => "[",
        IrOp::Call => "(",
        _ => "",
    }
}

/// Get the Java string comparison pattern.
pub fn java_string_equals(var: &str, other: &str) -> String {
    format!("{}.equals({})", var, other)
}
