//! `defer { ... }` — Go-style deferred execution.
//!
//! The parser rewrites `defer { body }` into a call to the global `defer` with a
//! zero-argument closure. This module provides that global and its namespaced
//! twin, `util.defer`.

use xcl_macros::xcl_fn;
use xcl_vm::{DomainCtx, Value};

fn defer_closure(ctx: &mut DomainCtx<'_>) -> Value {
    let closure = ctx.arg(0);
    if closure.as_func().is_some() {
        ctx.defer(closure);
    }
    Value::nil()
}

/// Defer the given zero-argument closure until the current frame returns.
///
/// Registered as the global `defer`, which is the target of the
/// `defer { ... }` statement. Deferred closures run LIFO on normal return and
/// on early `return`.
#[xcl_fn(name = "defer")]
pub fn defer_current(ctx: &mut DomainCtx<'_>) -> Value {
    defer_closure(ctx)
}

/// Namespaced form: `util.defer { ... }` (same behaviour as `defer { ... }`).
#[xcl_fn(name = "util.defer")]
pub fn util_defer(ctx: &mut DomainCtx<'_>) -> Value {
    defer_closure(ctx)
}
