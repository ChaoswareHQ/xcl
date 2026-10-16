//! Pretty-printer for IR nodes.
//!
//! Converts IR to a readable text representation for debugging and display.

use crate::translate::ir::node::{Ir, IrNode, IrLiteral};

/// Pretty-print IR nodes to a readable string.
pub fn emit(ir_nodes: &[Ir]) -> String {
    let mut lines = Vec::new();
    for node in ir_nodes {
        lines.push(emit_node(node, 0));
    }
    lines.join("\n")
}

/// Emit a single IR node.
fn emit_node(ir: &Ir, indent: usize) -> String {
    let indent_str = "  ".repeat(indent);

    match &ir.node {
        IrNode::Literal(lit) => emit_literal(lit),
        IrNode::Var(name) => name.clone(),
        IrNode::Call { callee, args } => {
            let callee_text = emit_node(callee, 0);
            let args_text: Vec<String> = args.iter().map(|a| emit_node(a, 0)).collect();
            format!("{}({})", callee_text, args_text.join(", "))
        }
        IrNode::BinOp { op, lhs, rhs } => {
            format!("{} {} {}", emit_node(lhs, 0), op, emit_node(rhs, 0))
        }
        IrNode::UnaryOp { op, operand } => {
            format!("{}{}", op, emit_node(operand, 0))
        }
        IrNode::If { cond, then, else_ } => {
            let cond_text = emit_node(cond, 0);
            let then_text = emit_node(then, indent + 1);
            if let Some(else_ir) = else_ {
                let else_text = emit_node(else_ir, indent + 1);
                format!(
                    "{}if {} {{\n{}\n{}}}\nelse {{\n{}\n{}}}",
                    indent_str, cond_text, then_text, indent_str, else_text, indent_str
                )
            } else {
                format!(
                    "{}if {} {{\n{}\n{}}}",
                    indent_str, cond_text, then_text, indent_str
                )
            }
        }
        IrNode::Let { name, ty, value, mutable } => {
            let mut_str = if *mutable { "mut " } else { "" };
            let value_text = emit_node(value, 0);
            format!(
                "{}let {}{}: {} = {}",
                indent_str, mut_str, name, ty, value_text
            )
        }
        IrNode::Fn { name, params, ret, body } => {
            let params_text: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect();
            let body_text = emit_node(body, indent + 1);
            format!(
                "{}fn {}({}) -> {} {{\n{}\n{}}}",
                indent_str, name, params_text.join(", "), ret, body_text, indent_str
            )
        }
        IrNode::Return(value) => {
            if let Some(val) = value {
                format!("{}return {}", indent_str, emit_node(val, 0))
            } else {
                format!("{}return", indent_str)
            }
        }
        IrNode::Block(nodes) => {
            let lines: Vec<String> = nodes.iter().map(|n| emit_node(n, indent)).collect();
            format!("{}{{\n{}\n{}}}", indent_str, lines.join("\n"), indent_str)
        }
        IrNode::Record { name, fields } => {
            let fields_text: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("  {}: {}", n, t))
                .collect();
            format!(
                "{}record {} {{\n{}\n{}}}",
                indent_str, name, fields_text.join("\n"), indent_str
            )
        }
        IrNode::FieldAccess { obj, field } => {
            format!("{}.{}", emit_node(obj, 0), field)
        }
        IrNode::Index { collection, index } => {
            format!(
                "{}[{}]",
                emit_node(collection, 0),
                emit_node(index, 0)
            )
        }
        IrNode::Defer(body_ir) => {
            let body_text = emit_node(body_ir, indent + 1);
            format!("{}defer {{\n{}\n{}}}", indent_str, body_text, indent_str)
        }
        IrNode::Lambda { params, ret: _, body } => {
            let params_text: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect();
            let body_text = emit_node(body, 0);
            format!(
                "|{}| -> {} {{}}",
                params_text.join(", "),
                body_text
            )
        }
        IrNode::Cast { value, to_type } => {
            format!("cast<{}>({})", to_type, emit_node(value, 0))
        }
        IrNode::Break => format!("{}break", indent_str),
        IrNode::Continue => format!("{}continue", indent_str),
        IrNode::Match { value, arms } => {
            let value_text = emit_node(value, 0);
            let mut lines = vec![format!("{}match {} {{", indent_str, value_text)];
            for (pattern, _guards, body_ir) in arms {
                let body_text = emit_node(body_ir, indent + 1);
                lines.push(format!("  {} => {}", pattern, body_text));
            }
            lines.push(format!("{}}}", indent_str));
            lines.join("\n")
        }
        IrNode::Loop { body, cond } => {
            let body_text = emit_node(body, indent + 1);
            if let Some(cond_ir) = cond {
                let cond_text = emit_node(cond_ir, 0);
                format!(
                    "{}while {} {{\n{}\n{}}}",
                    indent_str, cond_text, body_text, indent_str
                )
            } else {
                format!(
                    "{}loop {{\n{}\n{}}}",
                    indent_str, body_text, indent_str
                )
            }
        }
        IrNode::RecordLit { name, fields } => {
            let fields_text: Vec<String> = fields
                .iter()
                .map(|(n, v)| format!("{}: {}", n, emit_node(v, 0)))
                .collect();
            format!("{} {{{}}}", name, fields_text.join(", "))
        }
        IrNode::Slice { collection, start, end } => {
            format!(
                "{}[{}:{}]",
                emit_node(collection, 0),
                emit_node(start, 0),
                emit_node(end, 0)
            )
        }
    }
}

/// Emit a literal value.
fn emit_literal(lit: &IrLiteral) -> String {
    match lit {
        IrLiteral::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
        IrLiteral::Int(n) => n.to_string(),
        IrLiteral::Float(n) => n.to_string(),
        IrLiteral::Str(s) => format!("\"{}\"", s),
        IrLiteral::Nil => "nil".to_string(),
    }
}
