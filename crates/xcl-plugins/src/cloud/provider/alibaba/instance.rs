//! Alibaba ECS — compute instance operations using unified `instance.*` naming.
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use std::collections::HashMap;

#[xcl_fn(name = "instance", args = "name: str; f: any", ret = "any", summary = "Run a closure within an Alibaba ECS context.")]
fn instance_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    ctx.push_context("instance.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("instance.name");
    result
}

#[xcl_fn(name = "instance.config", ret = "map", summary = "Get the current instance configuration.")]
fn instance_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(inst_val) = ctx.get_context("instance.name") {
        config.insert("instance".to_string(), inst_val);
    }
    Value::from_map_boxed(config)
}

#[xcl_fn(name = "instance.create", args = "config: map", ret = "Result", summary = "Create an Alibaba ECS instance.")]
fn instance_create(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "Alibaba ECS not yet implemented", 0)
}

#[xcl_fn(name = "instance.start", ret = "Result", summary = "Start an Alibaba ECS instance.")]
fn instance_start(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "Alibaba ECS not yet implemented", 0)
}

#[xcl_fn(name = "instance.stop", ret = "Result", summary = "Stop an Alibaba ECS instance.")]
fn instance_stop(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "Alibaba ECS not yet implemented", 0)
}

#[xcl_fn(name = "instance.list", ret = "Result", summary = "List Alibaba ECS instances.")]
fn instance_list(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "Alibaba ECS not yet implemented", 0)
}
