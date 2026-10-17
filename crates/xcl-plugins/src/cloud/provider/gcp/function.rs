//! GCP Cloud Functions — serverless function operations using unified `function.*` naming.
//!
//! ## Domain
//! - `function(name)` — Domain wrapper for Cloud Function
//! - `function.invoke(payload)` — Invoke function (TODO)
//! - `function.list()` — List functions (TODO)

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use std::collections::HashMap;

/// Set the current function context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn function(name: str; f: any) -> any
/// ```
#[xcl_fn(
    name = "function",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a closure within a GCP Cloud Function context."
)]
fn function_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    ctx.push_context("function.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("function.name");
    result
}

/// Get the current function configuration.
#[xcl_fn(
    name = "function.config",
    ret = "map",
    summary = "Get the current function configuration."
)]
fn function_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(fn_val) = ctx.get_context("function.name") {
        config.insert("function".to_string(), fn_val);
    }
    Value::from_map_boxed(config)
}

/// Invoke the current function.
#[xcl_fn(
    name = "function.invoke",
    args = "payload: map",
    ret = "Result",
    summary = "Invoke a GCP Cloud Function."
)]
fn function_invoke(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Cloud Functions not yet implemented", 0)
}

/// List functions in the current project.
#[xcl_fn(
    name = "function.list",
    ret = "Result",
    summary = "List GCP Cloud Functions."
)]
fn function_list(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Cloud Functions not yet implemented", 0)
}
