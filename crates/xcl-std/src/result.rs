//! Result type and methods.
//!
//! Result is a sum type with two variants: Ok { value: any } and Err { error: Error }.
//! It uses subtype=1 and variant=0/1 for runtime dispatch, not a string tag.

use std::collections::HashMap;
use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::{Value, DomainCtx, SUBTYPE_RESULT, VARIANT_OK, VARIANT_ERR};
use crate::error::{make_error, error_message};

/// The Result type registered with the VM.
#[xcl_type(
    name = "Result",
    kind = "Sum",
    summary = "A success or failure. Construct with Ok(value) or Err(error). \
               Check with $r.is_ok, destructure with match.",
    variants(
        Ok { value: "any" },
        Err { error: "Error" },
    ),
)]
pub struct ResultType;

// ─── Payload accessors ───

#[inline]
fn is_ok_v(r: Value) -> bool {
    r.map_subtype() == Some(SUBTYPE_RESULT) && r.map_variant() == Some(VARIANT_OK)
}

#[inline]
fn is_err_v(r: Value) -> bool {
    r.map_subtype() == Some(SUBTYPE_RESULT) && r.map_variant() == Some(VARIANT_ERR)
}

#[inline]
fn ok_payload(r: Value) -> Value {
    r.as_map().and_then(|m| m.get("value")).copied().unwrap_or(Value::nil())
}

#[inline]
fn err_payload(r: Value) -> Value {
    r.as_map().and_then(|m| m.get("error")).copied().unwrap_or(Value::nil())
}

// ─── Constructors ───

/// Construct a successful Result value.
#[xcl_fn(name = "Ok", args = "value: any", ret = "Result")]
fn ok_fn(args: &[Value]) -> Value {
    let value = args.first().copied().unwrap_or(Value::nil());
    let mut m = HashMap::with_capacity(1);
    m.insert("value".into(), value);
    Value::from_map_boxed_typed_v(m, SUBTYPE_RESULT, VARIANT_OK)
}

/// Construct an error Result value.
#[xcl_fn(name = "Err", args = "error: Error", ret = "Result")]
fn err_fn(args: &[Value]) -> Value {
    let error = args.first().copied().unwrap_or(Value::nil());
    let mut m = HashMap::with_capacity(1);
    m.insert("error".into(), error);
    Value::from_map_boxed_typed_v(m, SUBTYPE_RESULT, VARIANT_ERR)
}

// ─── Helper constructors ───

pub fn make_ok(value: Value) -> Value { ok_fn(&[value]) }
pub fn make_err(error: Value) -> Value { err_fn(&[error]) }

pub fn ok_result(value: Value) -> Value { make_ok(value) }
pub fn err_result(kind: &str, message: &str, code: i64) -> Value {
    make_err(make_error(kind, message, code))
}

// ═══════════════════════════════════════════════════════════
//  10 methods on Result
// ═══════════════════════════════════════════════════════════

/// Check if the Result is Ok.
#[xcl_fn(name = "result.is_ok", args = "r: Result", ret = "bool")]
fn result_is_ok(args: &[Value]) -> Value {
    Value::bool_(is_ok_v(args[0]))
}

/// Check if the Result is Err.
#[xcl_fn(name = "result.is_err", args = "r: Result", ret = "bool")]
fn result_is_err(args: &[Value]) -> Value {
    Value::bool_(is_err_v(args[0]))
}

/// Unwrap the value or panic.
#[xcl_fn(name = "result.unwrap", args = "r: Result", ret = "any")]
fn result_unwrap(args: &[Value]) -> Value {
    let r = args[0];
    if is_ok_v(r) { return ok_payload(r); }
    let e = err_payload(r);
    let msg = error_message(&e).unwrap_or("called unwrap on an Err value");
    panic!("Result.unwrap: {msg}");
}

/// Unwrap the error or panic.
#[xcl_fn(name = "result.unwrap_err", args = "r: Result", ret = "Error")]
fn result_unwrap_err(args: &[Value]) -> Value {
    let r = args[0];
    if is_err_v(r) { return err_payload(r); }
    panic!("Result.unwrap_err: called on an Ok value");
}

/// Return the value or a default.
#[xcl_fn(name = "result.unwrap_or", args = "r: Result; default: any", ret = "any")]
fn result_unwrap_or(args: &[Value]) -> Value {
    let r = args[0];
    if is_ok_v(r) { ok_payload(r) } else { args[1] }
}

/// Return the value or call a fallback function with the error.
#[xcl_fn(name = "result.unwrap_or_else", args = "r: Result; f: any", ret = "any")]
fn result_unwrap_or_else(ctx: &mut DomainCtx) -> Value {
    let r = ctx.arg(0);
    if is_ok_v(r) { return ok_payload(r); }
    let f = ctx.arg(1);
    let error = err_payload(r);
    ctx.call(f, &[error])
}

/// Transform the Ok value.
#[xcl_fn(name = "result.map", args = "r: Result; f: any", ret = "Result")]
fn result_map(ctx: &mut DomainCtx) -> Value {
    let r = ctx.arg(0);
    if !is_ok_v(r) { return r; }
    let f = ctx.arg(1);
    let value = ok_payload(r);
    let new_value = ctx.call(f, &[value]);
    make_ok(new_value)
}

/// Transform the Err value.
#[xcl_fn(name = "result.map_err", args = "r: Result; f: any", ret = "Result")]
fn result_map_err(ctx: &mut DomainCtx) -> Value {
    let r = ctx.arg(0);
    if !is_err_v(r) { return r; }
    let f = ctx.arg(1);
    let error = err_payload(r);
    let new_error = ctx.call(f, &[error]);
    make_err(new_error)
}

/// Chain on Ok.
#[xcl_fn(name = "result.and_then", args = "r: Result; f: any", ret = "Result")]
fn result_and_then(ctx: &mut DomainCtx) -> Value {
    let r = ctx.arg(0);
    if !is_ok_v(r) { return r; }
    let f = ctx.arg(1);
    let value = ok_payload(r);
    ctx.call(f, &[value])
}

/// Recover from Err.
#[xcl_fn(name = "result.or_else", args = "r: Result; f: any", ret = "Result")]
fn result_or_else(ctx: &mut DomainCtx) -> Value {
    let r = ctx.arg(0);
    if is_ok_v(r) { return r; }
    let f = ctx.arg(1);
    let error = err_payload(r);
    ctx.call(f, &[error])
}
