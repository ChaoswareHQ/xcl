//! Whole-program pre-pass: which functions are safe to inline.
//!
//! Runs before compilation. It finds top-level functions whose body is pure
//! (no calls to unknown code, no assignment to anything but its own locals)
//! and records their parameter names and body, so a call site can substitute
//! arguments and emit the body directly.

#[cfg(feature = "std")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(feature = "std"))]
use alloc::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::debug::is_language_name;

/// Walks one statement or expression, collecting variable reads (first-use
/// order) and the names assigned in this scope.
///
/// Nested function and lambda bodies are skipped: their free variables are
/// resolved when those bodies are compiled.
pub(super) fn scan_vars(node: &AstNode, reads: &mut Vec<String>, writes: &mut Vec<String>) {
    match node {
        AstNode::Assign { name, value, .. } => {
            if !writes.iter().any(|n| n == name) {
                writes.push(name.clone());
            }
            scan_vars(value, reads, writes);
        }
        AstNode::For {
            variable,
            iterable,
            body,
            ..
        } => {
            if !writes.iter().any(|n| n == variable) {
                writes.push(variable.clone());
            }
            scan_vars(iterable, reads, writes);
            for s in body {
                scan_vars(s, reads, writes);
            }
        }
        AstNode::Return { value: Some(v), .. } => scan_vars(v, reads, writes),
        AstNode::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            scan_vars(condition, reads, writes);
            for s in then_branch {
                scan_vars(s, reads, writes);
            }
            if let Some(b) = else_branch {
                for s in b {
                    scan_vars(s, reads, writes);
                }
            }
        }
        AstNode::While {
            condition, body, ..
        } => {
            scan_vars(condition, reads, writes);
            for s in body {
                scan_vars(s, reads, writes);
            }
        }
        AstNode::Block { statements, .. } => {
            for s in statements {
                scan_vars(s, reads, writes);
            }
        }
        AstNode::Call { callee, args, .. } => {
            scan_vars(callee, reads, writes);
            for a in args {
                scan_vars(&a.value, reads, writes);
            }
        }
        AstNode::DomainCall {
            callee,
            args,
            items,
            ..
        } => {
            scan_vars(callee, reads, writes);
            for a in args {
                scan_vars(&a.value, reads, writes);
            }
            for item in items {
                scan_vars(&item.value, reads, writes);
                for (_, v) in &item.named {
                    scan_vars(v, reads, writes);
                }
            }
        }
        AstNode::Ident { name, .. } => {
            if !reads.iter().any(|n| n == name) {
                reads.push(name.clone());
            }
        }
        AstNode::BinaryOp { left, right, .. } => {
            scan_vars(left, reads, writes);
            scan_vars(right, reads, writes);
        }
        AstNode::UnaryOp { operand, .. } => scan_vars(operand, reads, writes),
        AstNode::Index { target, index, .. } => {
            scan_vars(target, reads, writes);
            scan_vars(index, reads, writes);
        }
        AstNode::FieldAccess { target, .. } => scan_vars(target, reads, writes),
        AstNode::Pipeline { input, stages, .. } => {
            scan_vars(input, reads, writes);
            for stage in stages {
                if let Some(a) = &stage.arg {
                    scan_vars(a, reads, writes);
                }
            }
        }
        AstNode::ArrayLit { elements, .. } => {
            for e in elements {
                scan_vars(e, reads, writes);
            }
        }
        AstNode::DictLit { entries, .. } => {
            for (_, v) in entries {
                scan_vars(v, reads, writes);
            }
        }
        AstNode::Range { start, end, .. } => {
            if let Some(s) = start {
                scan_vars(s, reads, writes);
            }
            if let Some(e) = end {
                scan_vars(e, reads, writes);
            }
        }
        AstNode::TypeTest { value, .. } | AstNode::TypeCast { value, .. } => {
            scan_vars(value, reads, writes);
        }
        // A nested lambda's free variables are also free here (unless one of
        // its parameters or body locals binds them), so they must be captured
        // by this scope too.
        AstNode::Lambda { params, body, .. } => {
            let mut inner_reads = Vec::new();
            let mut inner_writes = Vec::new();
            scan_vars(body, &mut inner_reads, &mut inner_writes);
            for name in inner_reads {
                if params.contains(&name) || inner_writes.iter().any(|w| w == &name) {
                    continue;
                }
                if !reads.iter().any(|n| n == &name) {
                    reads.push(name);
                }
            }
        }
        // Named function bodies resolve free names through the root frame, not
        // through captures, so they are not part of this scope.
        AstNode::FunctionDef { .. } => {}
        _ => {}
    }
}

/// Names assigned in the top-level frame, in first-appearance order.
///
/// These are the variables a function body may read through `OP_LOAD_ROOT`.
pub(super) fn collect_top_level_vars(ast: &[AstNode]) -> Vec<String> {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    for node in ast {
        scan_vars(node, &mut reads, &mut writes);
    }
    writes
}

/// Check whether an AST expression is "pure" for inlining purposes.
/// Pure = no Call, Assign, While, For, If, Lambda, FunctionDef, SpecialVar, Break, Continue.
fn is_pure_expr(node: &AstNode) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_pure_expr(operand),
        AstNode::BinaryOp { left, right, .. } => is_pure_expr(left) && is_pure_expr(right),
        AstNode::FieldAccess { target, .. } => is_pure_expr(target),
        AstNode::Index { target, index, .. } => is_pure_expr(target) && is_pure_expr(index),
        AstNode::ArrayLit { elements, .. } => elements.iter().all(is_pure_expr),
        AstNode::DictLit { entries, .. } => entries.iter().all(|(_, v)| is_pure_expr(v)),
        AstNode::Range { start, end, .. } => {
            start.as_ref().map_or(true, |s| is_pure_expr(s.as_ref()))
                && end.as_ref().map_or(true, |e| is_pure_expr(e.as_ref()))
        }
        AstNode::Block { statements, .. } => statements.iter().all(is_pure_expr),
        // Impure constructs
        AstNode::Call { .. }
        | AstNode::Assign { .. }
        | AstNode::While { .. }
        | AstNode::For { .. }
        | AstNode::If { .. }
        | AstNode::Lambda { .. }
        | AstNode::FunctionDef { .. }
        | AstNode::SpecialVar { .. }
        | AstNode::Break { .. }
        | AstNode::Continue { .. }
        | AstNode::Return { .. }
        | AstNode::Pipeline { .. }
        | AstNode::TypeDef { .. }
        | AstNode::ImplDef { .. }
        | AstNode::TypeTest { .. }
        | AstNode::TypeCast { .. }
        | AstNode::DomainCall { .. } => false,
    }
}

/// Purity check for multi-statement block expressions.
/// Like `is_pure_expr` but allows `Call` when the callee is an inlineable function.
fn is_pure_block_expr(node: &AstNode, inlineable_fns: &BTreeSet<String>) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_pure_block_expr(operand, inlineable_fns),
        AstNode::BinaryOp { left, right, .. } => {
            is_pure_block_expr(left, inlineable_fns) && is_pure_block_expr(right, inlineable_fns)
        }
        AstNode::FieldAccess { target, .. } => is_pure_block_expr(target, inlineable_fns),
        AstNode::Index { target, index, .. } => {
            is_pure_block_expr(target, inlineable_fns) && is_pure_block_expr(index, inlineable_fns)
        }
        AstNode::ArrayLit { elements, .. } => elements
            .iter()
            .all(|e| is_pure_block_expr(e, inlineable_fns)),
        AstNode::DictLit { entries, .. } => entries
            .iter()
            .all(|(_, v)| is_pure_block_expr(v, inlineable_fns)),
        AstNode::Range { start, end, .. } => {
            start
                .as_ref()
                .map_or(true, |s| is_pure_block_expr(s.as_ref(), inlineable_fns))
                && end
                    .as_ref()
                    .map_or(true, |e| is_pure_block_expr(e.as_ref(), inlineable_fns))
        }
        AstNode::Call { callee, args, .. } => {
            // Call is allowed only if callee is an identifier matching an inlineable function
            if let AstNode::Ident { name, .. } = callee.as_ref() {
                if !inlineable_fns.contains(name) {
                    return false;
                }
            } else {
                return false;
            }
            args.iter()
                .all(|a| is_pure_block_expr(&a.value, inlineable_fns))
        }
        // Impure constructs
        AstNode::Assign { .. }
        | AstNode::While { .. }
        | AstNode::For { .. }
        | AstNode::If { .. }
        | AstNode::Lambda { .. }
        | AstNode::FunctionDef { .. }
        | AstNode::SpecialVar { .. }
        | AstNode::Break { .. }
        | AstNode::Continue { .. }
        | AstNode::Return { .. }
        | AstNode::Pipeline { .. }
        | AstNode::TypeDef { .. }
        | AstNode::ImplDef { .. }
        | AstNode::TypeTest { .. }
        | AstNode::TypeCast { .. }
        | AstNode::DomainCall { .. }
        | AstNode::Block { .. } => false,
    }
}

/// Purity check excluding Call entirely (for the first pass in collect_inlineable).
fn is_pure_expr_no_call(node: &AstNode) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_pure_expr_no_call(operand),
        AstNode::BinaryOp { left, right, .. } => {
            is_pure_expr_no_call(left) && is_pure_expr_no_call(right)
        }
        AstNode::FieldAccess { target, .. } => is_pure_expr_no_call(target),
        AstNode::Index { target, index, .. } => {
            is_pure_expr_no_call(target) && is_pure_expr_no_call(index)
        }
        AstNode::ArrayLit { elements, .. } => elements.iter().all(is_pure_expr_no_call),
        AstNode::DictLit { entries, .. } => entries.iter().all(|(_, v)| is_pure_expr_no_call(v)),
        AstNode::Range { start, end, .. } => {
            start
                .as_ref()
                .map_or(true, |s| is_pure_expr_no_call(s.as_ref()))
                && end
                    .as_ref()
                    .map_or(true, |e| is_pure_expr_no_call(e.as_ref()))
        }
        _ => false,
    }
}

/// Check whether a call argument is "simple" for inlining (criterion 4).
/// Simple = Ident, Int, Float, Str, Bool, Nil, or pure UnaryOp/BinaryOp of simple values.
pub(super) fn is_simple_arg(node: &AstNode) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_simple_arg(operand),
        AstNode::BinaryOp { left, right, .. } => is_simple_arg(left) && is_simple_arg(right),
        _ => false,
    }
}

/// Collect all identifiers referenced in an expression.
fn collect_idents(node: &AstNode, out: &mut Vec<String>) {
    match node {
        AstNode::Ident { name, .. } => out.push(name.clone()),
        AstNode::UnaryOp { operand, .. } => collect_idents(operand, out),
        AstNode::BinaryOp { left, right, .. } => {
            collect_idents(left, out);
            collect_idents(right, out);
        }
        AstNode::FieldAccess { target, .. } => collect_idents(target, out),
        AstNode::Index { target, index, .. } => {
            collect_idents(target, out);
            collect_idents(index, out);
        }
        AstNode::ArrayLit { elements, .. } => {
            for e in elements {
                collect_idents(e, out);
            }
        }
        AstNode::DictLit { entries, .. } => {
            for (_, v) in entries {
                collect_idents(v, out);
            }
        }
        AstNode::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_idents(s, out);
            }
            if let Some(e) = end {
                collect_idents(e, out);
            }
        }
        AstNode::Block { statements, .. } => {
            for s in statements {
                collect_idents(s, out);
            }
        }
        _ => {}
    }
}

/// Substitute parameter names with argument expressions in a cloned body.
pub(super) fn substitute(node: &AstNode, subs: &[(String, AstNode)]) -> AstNode {
    match node {
        AstNode::Ident { name, .. } => {
            for (param, arg) in subs {
                if param == name {
                    return arg.clone();
                }
            }
            node.clone()
        }
        AstNode::UnaryOp { op, operand, span } => AstNode::UnaryOp {
            op: *op,
            operand: Box::new(substitute(operand, subs)),
            span: span.clone(),
        },
        AstNode::BinaryOp {
            left,
            op,
            right,
            span,
        } => AstNode::BinaryOp {
            left: Box::new(substitute(left, subs)),
            op: *op,
            right: Box::new(substitute(right, subs)),
            span: span.clone(),
        },
        AstNode::FieldAccess {
            target,
            field,
            span,
        } => AstNode::FieldAccess {
            target: Box::new(substitute(target, subs)),
            field: field.clone(),
            span: span.clone(),
        },
        AstNode::Index {
            target,
            index,
            span,
        } => AstNode::Index {
            target: Box::new(substitute(target, subs)),
            index: Box::new(substitute(index, subs)),
            span: span.clone(),
        },
        AstNode::ArrayLit { elements, span } => AstNode::ArrayLit {
            elements: elements.iter().map(|e| substitute(e, subs)).collect(),
            span: span.clone(),
        },
        AstNode::DictLit { entries, span } => AstNode::DictLit {
            entries: entries
                .iter()
                .map(|(k, v)| (k.clone(), substitute(v, subs)))
                .collect(),
            span: span.clone(),
        },
        AstNode::Range { start, end, span } => AstNode::Range {
            start: start.as_ref().map(|s| Box::new(substitute(s, subs))),
            end: end.as_ref().map(|e| Box::new(substitute(e, subs))),
            span: span.clone(),
        },
        AstNode::Block { statements, span } => AstNode::Block {
            statements: statements.iter().map(|s| substitute(s, subs)).collect(),
            span: span.clone(),
        },
        AstNode::Call { callee, args, span } => AstNode::Call {
            callee: Box::new(substitute(callee, subs)),
            args: args
                .iter()
                .map(|a| CallArg {
                    name: a.name.clone(),
                    value: substitute(&a.value, subs),
                })
                .collect(),
            span: span.clone(),
        },
        _ => node.clone(),
    }
}

/// Pre-pass: collect inlineable functions from the AST.
pub(super) fn collect_inlineable(ast: &[AstNode]) -> BTreeMap<String, InlineFn> {
    let mut map = BTreeMap::new();

    // First pass: identify which functions are single-expr inlineable (needed for
    // checking whether Call targets in multi-statement bodies are inlineable).
    let single_expr_fns: BTreeSet<String> = ast
        .iter()
        .filter_map(|node| {
            if let AstNode::FunctionDef {
                name, params, body, ..
            } = node
            {
                if body.len() == 1 {
                    if let AstNode::Return {
                        value: Some(expr), ..
                    } = &body[0]
                    {
                        let param_names: Vec<String> =
                            params.iter().map(|p| p.name.clone()).collect();
                        let mut idents = Vec::new();
                        collect_idents(expr, &mut idents);
                        if idents.iter().all(|id| param_names.contains(id))
                            && is_pure_expr_no_call(expr)
                        {
                            return Some(name.clone());
                        }
                    }
                }
            }
            None
        })
        .collect();

    for node in ast {
        if let AstNode::FunctionDef {
            name, params, body, ..
        } = node
        {
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

            // Try single-expression form first
            if body.len() == 1 {
                if let AstNode::Return {
                    value: Some(expr), ..
                } = &body[0]
                {
                    if is_pure_expr(expr) {
                        let mut idents = Vec::new();
                        collect_idents(expr, &mut idents);
                        let all_ok = idents.iter().all(|id| {
                            param_names.contains(id)
                                || is_language_name(id)
                                || single_expr_fns.contains(id)
                        });
                        if all_ok {
                            map.insert(
                                name.clone(),
                                InlineFn {
                                    params: param_names,
                                    body: InlineBody::Expr(expr.as_ref().clone()),
                                },
                            );
                            continue;
                        }
                    }
                }
            }

            // Try multi-statement block form:
            // Zero or more Assign statements followed by exactly one Return { value: Some(result) }
            if !body.is_empty() {
                // Last statement must be Return { value: Some(result) }
                let last = &body[body.len() - 1];
                let AstNode::Return {
                    value: Some(result),
                    ..
                } = last
                else {
                    continue;
                };

                // All preceding statements must be Assign
                let mut assigns: Vec<(String, AstNode)> = Vec::new();
                let mut body_locals: Vec<String> = Vec::new();
                let mut ok = true;

                for stmt in &body[..body.len() - 1] {
                    if let AstNode::Assign {
                        name: aname,
                        value: aval,
                        ..
                    } = stmt
                    {
                        // Name must not be a parameter
                        if param_names.contains(aname) {
                            ok = false;
                            break;
                        }
                        // Each body-local assigned exactly once
                        if body_locals.contains(aname) {
                            ok = false;
                            break;
                        }
                        body_locals.push(aname.clone());
                        assigns.push((aname.clone(), aval.as_ref().clone()));
                    } else {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }

                // Check purity of all assigned expressions and the result
                let all_assigns_pure = assigns
                    .iter()
                    .all(|(_, expr)| is_pure_block_expr(expr, &single_expr_fns));
                if !all_assigns_pure {
                    continue;
                }
                if !is_pure_block_expr(result, &single_expr_fns) {
                    continue;
                }

                // Check all identifiers: must be params, body-locals, known globals, or inlineable callees
                let mut all_idents = Vec::new();
                for (_, expr) in &assigns {
                    collect_idents(expr, &mut all_idents);
                }
                collect_idents(result, &mut all_idents);

                let all_ok = all_idents.iter().all(|id| {
                    param_names.contains(id)
                        || body_locals.contains(id)
                        || is_language_name(id)
                        || single_expr_fns.contains(id)
                });
                if !all_ok {
                    continue;
                }

                // If there are no assigns, treat as Expr form
                if assigns.is_empty() {
                    map.insert(
                        name.clone(),
                        InlineFn {
                            params: param_names,
                            body: InlineBody::Expr(result.as_ref().clone()),
                        },
                    );
                } else {
                    map.insert(
                        name.clone(),
                        InlineFn {
                            params: param_names,
                            body: InlineBody::Block {
                                assigns,
                                result: result.as_ref().clone(),
                            },
                        },
                    );
                }
            }
        }
    }
    map
}
