//! GCP Cloud Storage — object storage operations using unified `bucket.*` naming.
//!
//! ## Domain
//! - `bucket(name)` — Domain wrapper for Cloud Storage bucket
//! - `bucket.list()` — List objects in bucket (TODO)
//! - `bucket.get(key)` — Get object content (TODO)
//! - `bucket.put(key; value)` — Put object (TODO)
//! - `bucket.delete(key)` — Delete object (TODO)

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use std::collections::HashMap;

/// Set the current bucket context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket(name: str; f: any) -> any
/// ```
#[xcl_fn(
    name = "bucket",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a closure within a GCP Cloud Storage bucket context."
)]
fn bucket_run(ctx: &mut DomainCtx) -> Value {
    let bucket = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    ctx.push_context("bucket.name", Value::from_str_boxed(bucket.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("bucket.name");
    result
}

/// Get the current bucket configuration.
#[xcl_fn(
    name = "bucket.config",
    ret = "map",
    summary = "Get the current bucket configuration."
)]
fn bucket_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(bucket_val) = ctx.get_context("bucket.name") {
        config.insert("bucket".to_string(), bucket_val);
    }
    Value::from_map_boxed(config)
}

/// List objects in the current bucket.
#[xcl_fn(
    name = "bucket.list",
    args = "prefix: str",
    ret = "Result",
    summary = "List objects in the current GCP Cloud Storage bucket."
)]
fn bucket_list(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Cloud Storage not yet implemented", 0)
}

/// Get an object from the current bucket.
#[xcl_fn(
    name = "bucket.get",
    args = "key: str",
    ret = "Result",
    summary = "Get an object from GCP Cloud Storage."
)]
fn bucket_get(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Cloud Storage not yet implemented", 0)
}

/// Put an object into the current bucket.
#[xcl_fn(
    name = "bucket.put",
    args = "key: str; value: str",
    ret = "Result",
    summary = "Put an object into GCP Cloud Storage."
)]
fn bucket_put(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Cloud Storage not yet implemented", 0)
}

/// Delete an object from the current bucket.
#[xcl_fn(
    name = "bucket.delete",
    args = "key: str",
    ret = "Result",
    summary = "Delete an object from GCP Cloud Storage."
)]
fn bucket_delete(_ctx: &mut DomainCtx) -> Value {
    err_result("not_implemented", "GCP Cloud Storage not yet implemented", 0)
}
