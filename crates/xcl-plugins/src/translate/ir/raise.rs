//! Raise IR back to XCL AST.
//!
//! Converts IR nodes back to XCL source code text. This is used for
//! round-trip translation: IR → XCL source.
//!
//! Per contract IR3.2: IR is lossless enough to reconstruct XCL.

use super::node::{Ir, IrNode, IrLiteral};
use super::types::IrType;

/// Raise IR nodes back to XCL source code.
///
/// # Arguments
/// - `ir_nodes` — Vector of IR nodes to convert
///
/// # Returns
/// XCL source code string
pub fn raise_to_xcl(ir_nodes: &[Ir]) -> String {
    let mut lines = Vec::new();
    for ir in ir_nodes {
        let text = raise_node(ir, 0);
        lines.push(text);
    }
    lines.join("\n")
}

/// Raise a single IR node to XCL text.
fn raise_node(ir: &Ir, indent: usize) -> String {
    let indent_str = "    ".repeat(indent);

    match &ir.node {
        IrNode::Literal(lit) => raise_literal(lit),
        IrNode::Var(name) => name.clone(),
        IrNode::Call { callee, args } => {
            let callee_text = raise_node(callee, 0);
            let args_text: Vec<String> = args.iter().map(|a| raise_node(a, 0)).collect();
            format!("{}({})", callee_text, args_text.join(", "))
        }
        IrNode::BinOp { op, lhs, rhs } => {
            let lhs_text = raise_node(lhs, 0);
            let rhs_text = raise_node(rhs, 0);
            format!("{} {} {}", lhs_text, op, rhs_text)
        }
        IrNode::UnaryOp { op, operand } => {
            let operand_text = raise_node(operand, 0);
            format!("{}{}", op, operand_text)
        }
        IrNode::If { cond, then, else_ } => {
            let cond_text = raise_node(cond, 0);
            let then_text = raise_node(then, indent + 1);

            if let Some(else_ir) = else_ {
                let else_text = raise_node(else_ir, indent + 1);
                format!(
                    "{}if {} {{\n{}\n{}}}\n{}else {{\n{}\n{}}}",
                    indent_str, cond_text, then_text, indent_str,
                    indent_str, else_text, indent_str
                )
            } else {
                format!(
                    "{}if {} {{\n{}\n{}}}",
                    indent_str, cond_text, then_text, indent_str
                )
            }
        }
        IrNode::Loop { body, cond } => {
            let body_text = raise_node(body, indent + 1);
            if let Some(cond_ir) = cond {
                let cond_text = raise_node(cond_ir, 0);
                format!(
                    "{}while {} {{\n{}\n{}}}",
                    indent_str, cond_text, body_text, indent_str
                )
            } else {
                format!(
                    "{}while true {{\n{}\n{}}}",
                    indent_str, body_text, indent_str
                )
            }
        }
        IrNode::Block(nodes) => {
            let mut lines = Vec::new();
            for node in nodes {
                lines.push(raise_node(node, indent));
            }
            lines.join("\n")
        }
        IrNode::Return(value) => {
            if let Some(val) = value {
                format!("{}return {}", indent_str, raise_node(val, 0))
            } else {
                format!("{}return", indent_str)
            }
        }
        IrNode::Let { name, ty, value, mutable } => {
            let value_text = raise_node(value, 0);
            let type_ann = if ty != &IrType::Any {
                format!(": {}", ty)
            } else {
                String::new()
            };

            if *mutable {
                format!(
                    "{}let mut {}{} = {}",
                    indent_str, name, type_ann, value_text
                )
            } else {
                format!(
                    "{}let {}{} = {}",
                    indent_str, name, type_ann, value_text
                )
            }
        }
        IrNode::Record { name, fields } => {
            let mut lines = vec![format!("{}type {}", indent_str, name)];
            for (field_name, field_ty) in fields {
                lines.push(format!("{}    {}: {}", indent_str, field_name, field_ty));
            }
            lines.join("\n")
        }
        IrNode::RecordLit { name, fields } => {
            let fields_text: Vec<String> = fields
                .iter()
                .map(|(fn_name, fv)| format!("{}: {}", fn_name, raise_node(fv, 0)))
                .collect();
            format!("{}({})", name, fields_text.join(", "))
        }
        IrNode::Fn { name, params, ret, body } => {
            let params_text: Vec<String> = params
                .iter()
                .map(|(pn, pt)| format!("{}: {}", pn, pt))
                .collect();
            let body_text = raise_node(body, indent + 1);
            format!(
                "{}fn {}({}) -> {} {{\n{}\n{}}}",
                indent_str, name, params_text.join(", "), ret, body_text, indent_str
            )
        }
        IrNode::Lambda { params, ret: _, body } => {
            let params_text: Vec<String> = params
                .iter()
                .map(|(pn, pt)| format!("{}: {}", pn, pt))
                .collect();
            let body_text = raise_node(body, 0);
            format!("|{}| -> {}", params_text.join(", "), body_text)
        }
        IrNode::Defer(body_ir) => {
            let body_text = raise_node(body_ir, indent + 1);
            format!("{}defer {{\n{}\n{}}}", indent_str, body_text, indent_str)
        }
        IrNode::FieldAccess { obj, field } => {
            format!("{}.{}", raise_node(obj, 0), field)
        }
        IrNode::Index { collection, index } => {
            format!(
                "{}[{}]",
                raise_node(collection, 0),
                raise_node(index, 0)
            )
        }
        IrNode::Slice { collection, start, end } => {
            format!(
                "{}[{}:{}]",
                raise_node(collection, 0),
                raise_node(start, 0),
                raise_node(end, 0)
            )
        }
        IrNode::Cast { value, to_type } => {
            format!("as {}({})", to_type, raise_node(value, 0))
        }
        IrNode::Break => format!("{}break", indent_str),
        IrNode::Continue => format!("{}continue", indent_str),
        IrNode::Match { value, arms } => {
            let value_text = raise_node(value, 0);
            let mut lines = vec![format!("{}match {} {{", indent_str, value_text)];
            for (pattern, _guards, body_ir) in arms {
                let body_text = raise_node(body_ir, indent + 1);
                lines.push(format!("{}    {} => {}", indent_str, pattern, body_text));
            }
            lines.push(format!("{}}}", indent_str));
            lines.join("\n")
        }
    }
}

/// Raise a literal to XCL text.
fn raise_literal(lit: &IrLiteral) -> String {
    match lit {
        IrLiteral::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
        IrLiteral::Int(n) => n.to_string(),
        IrLiteral::Float(n) => {
            if n.fract() == 0.0 {
                format!("{}.0", n)
            } else {
                n.to_string()
            }
        }
        IrLiteral::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        IrLiteral::Nil => "none".to_string(),
    }
}
