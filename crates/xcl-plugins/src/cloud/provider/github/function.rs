//! GitHub Actions — serverless function operations using unified `function.*` naming.
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use std::collections::HashMap;

#[xcl_fn(name = "function", args = "name: str; f: any", ret = "any", summary = "Run a closure within a GitHub Action context.")]
fn function_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    ctx.push_context("function.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("function.name");
    result
}

#[xcl_fn(name = "function.config", ret = "map", summary = "Get the current function configuration.")]
fn function_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(fn_val) = ctx.get_context("function.name") {
        config.insert("function".to_string(), fn_val);
    }
    Value::from_map_boxed(config)
}

#[xcl_fn(name = "function.invoke", args = "payload: map", ret = "Result", summary = "Invoke a GitHub Action.")]
fn function_invoke(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GitHub Actions not yet implemented", 0)
}

#[xcl_fn(name = "function.list", ret = "Result", summary = "List GitHub Actions.")]
fn function_list(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GitHub Actions not yet implemented", 0)
}
