//! Lower XCL AST to IR.
//!
//! Converts XCL abstract syntax tree nodes into the intermediate representation.
//! This is the first step in translation: XCL source → XCL AST → IR.
//!
//! Per contract FE4.3: a frontend must preserve semantics — no silent lowering.

use super::node::{Ir, IrNode, IrLiteral, SourceSpan, Origin};
use super::types::IrType;
use super::ops::{IrOp, Hint};
use crate::translate::version::LangSpec;

/// Lower an XCL expression (represented as a simplified AST) to IR.
///
/// Since XCL parsing produces `Value`-based structures, this function
/// converts them to typed IR nodes.
///
/// # Arguments
/// - `source` — XCL source code string
/// - `origin` — Origin metadata for the IR nodes
///
/// # Returns
/// A vector of IR nodes representing the lowered XCL code
pub fn lower_xcl(source: &str, origin: Origin) -> Vec<Ir> {
    let mut result = Vec::new();
    lower_source(source, &origin, &mut result);
    result
}

/// Lower XCL source text to IR nodes.
fn lower_source(source: &str, origin: &Origin, result: &mut Vec<Ir>) {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        let line_num = result.len() + 1;
        let span = SourceSpan::point(line_num, 1);

        // Parse simple XCL expressions
        if let Some(ir) = lower_simple_expr(trimmed, origin, span) {
            result.push(ir);
        }
    }
}

/// Lower a simple XCL expression to an IR node.
fn lower_simple_expr(text: &str, origin: &Origin, span: SourceSpan) -> Option<Ir> {
    let text = text.trim();

    // Literal values
    if text == "true" || text == "false" {
        return Some(Ir::literal(
            IrLiteral::Bool(text == "true"),
            origin.clone(),
        ));
    }

    if text == "none" {
        return Some(Ir::literal(IrLiteral::Nil, origin.clone()));
    }

    if let Some(num) = parse_int(text) {
        return Some(Ir::literal(IrLiteral::Int(num), origin.clone()));
    }

    if let Some(num) = parse_float(text) {
        return Some(Ir::literal(IrLiteral::Float(num), origin.clone()));
    }

    if text.starts_with('"') && text.ends_with('"') && text.len() >= 2 {
        let str_val = &text[1..text.len() - 1];
        return Some(Ir::literal(IrLiteral::Str(str_val.to_string()), origin.clone()));
    }

    // XCL $-prefix assignment: $x = 10
    if let Some(rest) = text.strip_prefix('$') {
        if let Some((name, value_text)) = rest.split_once('=') {
            let name = name.trim().to_string();
            let value_text = value_text.trim();
            let (value, ty) = if let Some(v) = lower_simple_expr(value_text, origin, span.clone()) {
                let ty = v.ty.clone();
                (v, ty)
            } else {
                (Ir::literal(IrLiteral::Nil, origin.clone()), IrType::Any)
            };
            let node = IrNode::Let {
                name,
                ty,
                value: Box::new(value),
                mutable: true,
            };
            return Some(Ir::simple(node, IrType::Nil, origin.clone()));
        }
        // $-prefix variable reference: $x
        if is_identifier(rest.trim()) {
            return Some(Ir::var(rest.trim().to_string(), origin.clone()));
        }
    }

    // Let binding
    if let Some(rest) = text.strip_prefix("let ") {
        return lower_let(rest, origin, span);
    }

    // Function definition (with or without fn keyword)
    if let Some(rest) = text.strip_prefix("fn ") {
        return lower_fn_def(rest, origin, span);
    }

    // If statement
    if let Some(rest) = text.strip_prefix("if ") {
        return lower_if(rest, origin, span);
    }

    // Return statement
    if let Some(rest) = text.strip_prefix("return ") {
        let value = lower_simple_expr(rest.trim(), origin, span.clone())?;
        return Some(Ir::simple(
            IrNode::Return(Some(Box::new(value))),
            IrType::Nil,
            origin.clone(),
        ));
    }

    // Variable reference or function call
    if text.contains('(') {
        return lower_call(text, origin, span);
    }

    // Binary operation
    if let Some(binop) = parse_binary_op(text) {
        return Some(binop);
    }

    // Plain variable reference
    if is_identifier(text) {
        return Some(Ir::var(text.to_string(), origin.clone()));
    }

    None
}

/// Lower a `let` binding.
fn lower_let(text: &str, origin: &Origin, span: SourceSpan) -> Option<Ir> {
    let mut text = text.trim();
    let mut mutable = false;

    if let Some(rest) = text.strip_prefix("mut ") {
        mutable = true;
        text = rest;
    }

    // Parse: name = value
    if let Some((name, value_text)) = text.split_once('=') {
        let name = name.trim().to_string();
        let value_text = value_text.trim();

        // Try to infer type from value
        let (value, ty) = if let Some(v) = lower_simple_expr(value_text, origin, span.clone()) {
            let ty = v.ty.clone();
            (v, ty)
        } else {
            let val = Ir::literal(IrLiteral::Nil, origin.clone());
            (val, IrType::Any)
        };

        let node = IrNode::Let {
            name,
            ty,
            value: Box::new(value),
            mutable,
        };

        return Some(Ir::simple(node, IrType::Nil, origin.clone()));
    }

    None
}

/// Lower a function definition.
fn lower_fn_def(text: &str, origin: &Origin, span: SourceSpan) -> Option<Ir> {
    let text = text.trim();

    // Parse: name(params) { body }
    if let Some((header, body_text)) = text.split_once('{') {
        let header = header.trim();
        let body_text = body_text.trim_end_matches('}');

        // Parse: name(params)
        if let Some((name, params_text)) = header.split_once('(') {
            let name = name.trim().to_string();
            let params_text = params_text.trim_end_matches(')');

            // Parse parameters
            let mut params = Vec::new();
            if !params_text.trim().is_empty() {
                for param in params_text.split(',') {
                    let param = param.trim();
                    if let Some((pname, ptype)) = param.split_once(':') {
                        let ptype = parse_type_str(ptype.trim());
                        params.push((pname.trim().to_string(), ptype));
                    } else {
                        params.push((param.to_string(), IrType::Any));
                    }
                }
            }

            // Lower body
            let body_ir = lower_simple_expr(body_text.trim(), origin, span.clone())
                .unwrap_or_else(|| Ir::literal(IrLiteral::Nil, origin.clone()));

            let node = IrNode::Fn {
                name,
                params,
                ret: IrType::Any,
                body: Box::new(body_ir),
            };

            return Some(Ir::simple(node, IrType::Nil, origin.clone())
                .with_hint(Hint::Pure));
        }
    }

    None
}

/// Lower an if statement.
fn lower_if(text: &str, origin: &Origin, span: SourceSpan) -> Option<Ir> {
    let text = text.trim();

    // Simple if: if cond { then } else { else_ }
    if let Some((cond_text, rest)) = text.split_once('{') {
        let cond = lower_simple_expr(cond_text.trim(), origin, span.clone())?;
        
        // Find matching closing brace and check for else
        let mut depth = 1;
        let mut has_else = false;
        let chars: Vec<char> = rest.chars().collect();
        let mut i = 0;
        while i < chars.len() && depth > 0 {
            match chars[i] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        // Check for "else" after this closing brace
                        let after = &rest[i + 1..];
                        if let Some(else_rest) = after.strip_prefix("else ") {
                            if else_rest.contains('{') {
                                has_else = true;
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        let then_text = &rest[..i];
        let then_ir = lower_simple_expr(then_text.trim(), origin, span.clone())
            .unwrap_or_else(|| Ir::literal(IrLiteral::Nil, origin.clone()));

        let else_ir = if has_else {
            // Extract else body
            let after_then = &rest[i + 1..];
            if let Some(else_body_start) = after_then.strip_prefix("else ") {
                if let Some(body) = else_body_start.strip_prefix('{') {
                    let body = body.trim_end_matches('}');
                    let else_val = lower_simple_expr(body.trim(), origin, span.clone())
                        .unwrap_or_else(|| Ir::literal(IrLiteral::Nil, origin.clone()));
                    Some(Box::new(else_val))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let node = IrNode::If {
            cond: Box::new(cond),
            then: Box::new(then_ir),
            else_: else_ir,
        };

        return Some(Ir::simple(node, IrType::Nil, origin.clone()));
    }

    None
}

/// Lower a function call.
fn lower_call(text: &str, origin: &Origin, span: SourceSpan) -> Option<Ir> {
    let text = text.trim();

    // Find the opening paren
    if let Some(paren_pos) = text.find('(') {
        let callee_text = &text[..paren_pos].trim();
        let args_text = &text[paren_pos + 1..];
        let args_text = args_text.trim_end_matches(')');

        let callee = lower_simple_expr(callee_text, origin, span.clone())?;

        let mut args = Vec::new();
        if !args_text.trim().is_empty() {
            for arg in args_text.split(',') {
                if let Some(arg_ir) = lower_simple_expr(arg.trim(), origin, span.clone()) {
                    args.push(arg_ir);
                }
            }
        }

        let node = IrNode::Call {
            callee: Box::new(callee),
            args,
        };

        return Some(Ir::simple(node, IrType::Any, origin.clone()));
    }

    None
}

/// Parse a binary operation from text.
fn parse_binary_op(text: &str) -> Option<Ir> {
    // Check for binary operators (longest match first)
    let ops = [
        ("==", IrOp::Eq), ("!=", IrOp::Ne), ("<=", IrOp::Le), (">=", IrOp::Ge),
        ("<", IrOp::Lt), (">", IrOp::Gt), ("&&", IrOp::And), ("||", IrOp::Or),
        ("+", IrOp::Add), ("-", IrOp::Sub), ("*", IrOp::Mul), ("/", IrOp::Div),
        ("%", IrOp::Rem),
    ];

    for (op_str, op) in &ops {
        // Find the operator, avoiding string literals
        if let Some(pos) = find_op_outside_strings(text, op_str) {
            let lhs_text = &text[..pos].trim();
            let rhs_text = &text[pos + op_str.len()..].trim();

            let origin = Origin::new(LangSpec::xcl("1"));
            let span = SourceSpan::dummy();

            if let (Some(lhs), Some(rhs)) = (
                lower_simple_expr(lhs_text, &origin, span.clone()),
                lower_simple_expr(rhs_text, &origin, span.clone()),
            ) {
                let node = IrNode::BinOp {
                    op: *op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };

                let result_type = if matches!(op, IrOp::Eq | IrOp::Ne | IrOp::Lt | IrOp::Le | IrOp::Gt | IrOp::Ge | IrOp::And | IrOp::Or) {
                    IrType::Bool
                } else {
                    IrType::Any
                };

                return Some(Ir::simple(node, result_type, origin));
            }
        }
    }

    None
}

/// Find an operator outside of string literals.
fn find_op_outside_strings(text: &str, op: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escape = false;
    let mut i = 0;
    let bytes = text.as_bytes();

    while i < bytes.len() {
        let ch = bytes[i] as char;
        i += 1;

        if escape {
            escape = false;
            continue;
        }

        if ch == '\\' {
            escape = true;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            continue;
        }

        if !in_string {
            let remaining = &text[i - 1..];
            if remaining.starts_with(op) {
                return Some(i - 1);
            }
        }
    }

    None
}

/// Parse an integer from text.
fn parse_int(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
}

/// Parse a float from text.
fn parse_float(text: &str) -> Option<f64> {
    if text.contains('.') {
        text.parse::<f64>().ok()
    } else {
        None
    }
}

/// Parse a type string to IrType.
fn parse_type_str(text: &str) -> IrType {
    match text {
        "int" => IrType::Int,
        "float" => IrType::Float,
        "bool" => IrType::Bool,
        "str" => IrType::Str,
        "nil" => IrType::Nil,
        "any" => IrType::Any,
        _ => IrType::Any,
    }
}

/// Check if text is a valid identifier.
fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
        && text.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_')
}
