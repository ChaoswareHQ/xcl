//! GCP Compute Engine — compute instance operations using unified `instance.*` naming.
//!
//! ## Domain
//! - `instance(name)` — Domain wrapper for Compute Instance
//! - `instance.create(config)` — Create instance (TODO)
//! - `instance.start()` — Start instance (TODO)
//! - `instance.stop()` — Stop instance (TODO)
//! - `instance.list()` — List instances (TODO)

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use std::collections::HashMap;

/// Set the current instance context.
#[xcl_fn(
    name = "instance",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a closure within a GCP Compute Engine instance context."
)]
fn instance_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    ctx.push_context("instance.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("instance.name");
    result
}

/// Get the current instance configuration.
#[xcl_fn(
    name = "instance.config",
    ret = "map",
    summary = "Get the current instance configuration."
)]
fn instance_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(inst_val) = ctx.get_context("instance.name") {
        config.insert("instance".to_string(), inst_val);
    }
    Value::from_map_boxed(config)
}

/// Create a new compute instance.
#[xcl_fn(
    name = "instance.create",
    args = "config: map",
    ret = "Result",
    summary = "Create a GCP Compute Engine instance."
)]
fn instance_create(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Compute Engine not yet implemented", 0)
}

/// Start the current instance.
#[xcl_fn(
    name = "instance.start",
    ret = "Result",
    summary = "Start a GCP Compute Engine instance."
)]
fn instance_start(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Compute Engine not yet implemented", 0)
}

/// Stop the current instance.
#[xcl_fn(
    name = "instance.stop",
    ret = "Result",
    summary = "Stop a GCP Compute Engine instance."
)]
fn instance_stop(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Compute Engine not yet implemented", 0)
}

/// List instances in the current project.
#[xcl_fn(
    name = "instance.list",
    ret = "Result",
    summary = "List GCP Compute Engine instances."
)]
fn instance_list(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Compute Engine not yet implemented", 0)
}
