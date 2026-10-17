//! Result and Error types for xcl-plugins.
//!
//! Provides `Ok`, `Err`, `make_error`, `result.*` operations, as well as
//! internal helper functions `ok_result` and `err_result` used by other modules.

use std::collections::HashMap;
use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::{Value, DomainCtx, SUBTYPE_RESULT, SUBTYPE_ERROR, VARIANT_OK, VARIANT_ERR};

/// An error value with a kind, message, and optional code.
///
/// Represents a structured error that can be returned in the `Err` variant of a `Result`.
/// Created via `make_error` or returned by failing operations.
///
/// ## EXAMPLES
///
/// ```xcl
/// $err = make_error "not_found" "file missing" 2
/// io.print $err.kind    // "not_found"
/// io.print $err.message // "file missing"
/// io.print $err.code    // 2
/// ```
#[xcl_type(
    name = "Error",
    kind = "Record",
    fields { kind: "str", message: "str", code: "int" },
    summary = "A structured error with kind, message, and code."
)]
pub struct ErrorType;

/// A result value that is either `Ok` (success) or `Err` (failure).
///
/// Represents the outcome of an operation that may fail. The `Ok` variant contains
/// a success value, while the `Err` variant contains an `Error` value.
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = Ok 42
/// if result.is_ok $r {
///     io.print $r.value  // 42
/// }
///
/// $r = Err (make_error "not_found" "missing" 2)
/// if result.is_err $r {
///     io.print $r.error.message  // "missing"
/// }
/// ```
#[xcl_type(
    name = "Result",
    kind = "Sum",
    variants { Ok { value: "any" }, Err { error: "Error" } },
    summary = "Represents the outcome of an operation: success or failure."
)]
pub struct ResultType;

/// Create an Error value with the given kind, message, and code.
///
/// ## SYNTAX
///
/// ```xcl
/// fn make_error($kind: str; $message: str; $code: int) -> Error
/// ```
///
/// ## PARAMETERS
///
/// `$kind`: A short string identifying the error category (e.g., "not_found", "permission_denied").
///
/// `$message`: A human-readable description of the error.
///
/// `$code`: An optional numeric error code.
///
/// ## EXAMPLES
///
/// ```xcl
/// $err = make_error "not_found" "file missing" 2
/// ```
#[xcl_fn(name = "make_error", args = "kind: str; message: str; code: int", ret = "Error")]
pub fn make_error(args: &[Value]) -> Value {
    let kind = args[0].as_str().unwrap_or("").to_string();
    let message = args[1].as_str().unwrap_or("").to_string();
    let code = args[2].as_i48();

    let mut m = HashMap::new();
    m.insert("kind".to_string(), Value::from_str_boxed(kind));
    m.insert("message".to_string(), Value::from_str_boxed(message));
    m.insert("code".to_string(), Value::from_i48(code));
    Value::from_map_boxed_typed_v(m, SUBTYPE_ERROR, VARIANT_ERR)
}

/// Wrap a value in an Ok result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn Ok($value: any) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = Ok 42
/// io.print $r.value  // 42
/// ```
#[xcl_fn(name = "Ok", args = "value: any", ret = "Result")]
pub fn ok_variant(args: &[Value]) -> Value {
    ok_result(args[0])
}

/// Wrap an error in an Err result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn Err($error: Error) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = Err (make_error "not_found" "missing" 2)
/// io.print $r.error.message  // "missing"
/// ```
#[xcl_fn(name = "Err", args = "error: Error", ret = "Result")]
pub fn err_variant(args: &[Value]) -> Value {
    let error = args[0];
    let mut m = HashMap::new();
    m.insert("error".to_string(), error);
    Value::from_map_boxed_typed_v(m, SUBTYPE_RESULT, VARIANT_ERR)
}

/// Apply a function to the value inside an Ok result, or pass through Err unchanged.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.map($result: Result; $f: any) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = result.map (Ok 10) (($x) => $x * 2)
/// io.print $r.value  // 20
/// ```
#[xcl_fn(name = "result.map", args = "result: Result; f: any", ret = "Result")]
pub fn result_map(ctx: &mut DomainCtx) -> Value {
    let result = ctx.arg(0);
    let f = ctx.arg(1);

    match result.map_variant() {
        Some(variant) if variant == VARIANT_OK => {
            let map = result.as_map().unwrap();
            let value = map.get("value").copied().unwrap_or(Value::nil());
            let new_value = ctx.call(f, &[value]);
            ok_result(new_value)
        }
        _ => result, // Pass through Err unchanged
    }
}

/// Apply a function returning a Result to the value inside an Ok result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.and_then($result: Result; $f: any) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $double = ($x) => Ok ($x * 2)
/// $r = result.and_then (Ok 5) $double
/// io.print $r.value  // 10
/// ```
#[xcl_fn(name = "result.and_then", args = "result: Result; f: any", ret = "Result")]
pub fn result_and_then(ctx: &mut DomainCtx) -> Value {
    let result = ctx.arg(0);
    let f = ctx.arg(1);

    match result.map_variant() {
        Some(variant) if variant == VARIANT_OK => {
            let map = result.as_map().unwrap();
            let value = map.get("value").copied().unwrap_or(Value::nil());
            ctx.call(f, &[value])
        }
        _ => result, // Pass through Err unchanged
    }
}

/// Apply a function to the error inside an Err result, or pass through Ok unchanged.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.map_err($result: Result; $f: any) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $wrapper = ($e) => make_error "wrapped" $e.message $e.code
/// $r = result.map_err (Err (make_error "not_found" "file" 2)) $wrapper
/// ```
#[xcl_fn(name = "result.map_err", args = "result: Result; f: any", ret = "Result")]
pub fn result_map_err(ctx: &mut DomainCtx) -> Value {
    let result = ctx.arg(0);
    let f = ctx.arg(1);

    match result.map_variant() {
        Some(variant) if variant == VARIANT_ERR => {
            let map = result.as_map().unwrap();
            let error = map.get("error").copied().unwrap_or(Value::nil());
            let new_error = ctx.call(f, &[error]);
            let mut m = HashMap::new();
            m.insert("error".to_string(), new_error);
            Value::from_map_boxed_typed_v(m, SUBTYPE_RESULT, VARIANT_ERR)
        }
        _ => result, // Pass through Ok unchanged
    }
}

/// Apply a function returning a Result to the error inside an Err result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.or_else($result: Result; $f: any) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $fallback = ($_) => Ok 99
/// $r = result.or_else (Err (make_error "x" "y" 0)) $fallback
/// io.print $r.value  // 99
/// ```
#[xcl_fn(name = "result.or_else", args = "result: Result; f: any", ret = "Result")]
pub fn result_or_else(ctx: &mut DomainCtx) -> Value {
    let result = ctx.arg(0);
    let f = ctx.arg(1);

    match result.map_variant() {
        Some(variant) if variant == VARIANT_ERR => {
            let map = result.as_map().unwrap();
            let error = map.get("error").copied().unwrap_or(Value::nil());
            ctx.call(f, &[error])
        }
        _ => result, // Pass through Ok unchanged
    }
}

/// Extract the value from an Ok result, or return the fallback if Err.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.unwrap_or($result: Result; $fallback: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = result.unwrap_or (Ok 42) 0
/// io.print $r  // 42
///
/// $r = result.unwrap_or (Err (make_error "x" "y" 0)) 99
/// io.print $r  // 99
/// ```
#[xcl_fn(name = "result.unwrap_or", args = "result: Result; fallback: any", ret = "any")]
pub fn result_unwrap_or(args: &[Value]) -> Value {
    let result = args[0];
    let fallback = args[1];

    match result.map_variant() {
        Some(variant) if variant == VARIANT_OK => {
            let map = result.as_map().unwrap();
            map.get("value").copied().unwrap_or(Value::nil())
        }
        _ => fallback,
    }
}

/// Extract the value from an Ok result, or call a function with the error if Err.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.unwrap_or_else($result: Result; $f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $fallback = ($_) => 77
/// $r = result.unwrap_or_else (Err (make_error "x" "y" 0)) $fallback
/// io.print $r  // 77
/// ```
#[xcl_fn(name = "result.unwrap_or_else", args = "result: Result; f: any", ret = "any")]
pub fn result_unwrap_or_else(ctx: &mut DomainCtx) -> Value {
    let result = ctx.arg(0);
    let f = ctx.arg(1);

    match result.map_variant() {
        Some(variant) if variant == VARIANT_OK => {
            let map = result.as_map().unwrap();
            map.get("value").copied().unwrap_or(Value::nil())
        }
        _ => {
            let map = result.as_map().unwrap();
            let error = map.get("error").copied().unwrap_or(Value::nil());
            ctx.call(f, &[error])
        }
    }
}

/// Check if a result is Ok.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.is_ok($result: Result) -> bool
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = Ok 42
/// if result.is_ok $r {
///     io.print $r.value  // 42
/// }
/// ```
#[xcl_fn(name = "result.is_ok", args = "result: Result", ret = "bool")]
pub fn result_is_ok(args: &[Value]) -> Value {
    let result = args[0];
    Value::bool_(result.map_variant() == Some(VARIANT_OK))
}

/// Check if a result is Err.
///
/// ## SYNTAX
///
/// ```xcl
/// fn result.is_err($result: Result) -> bool
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = Err (make_error "not_found" "missing" 2)
/// if result.is_err $r {
///     io.print $r.error.message  // "missing"
/// }
/// ```
#[xcl_fn(name = "result.is_err", args = "result: Result", ret = "bool")]
pub fn result_is_err(args: &[Value]) -> Value {
    let result = args[0];
    Value::bool_(result.map_variant() == Some(VARIANT_ERR))
}

// ─── Internal helpers used by other modules ───

/// Create an Ok result wrapping the given value.
///
/// Used internally by io, proc, and other modules to return success results.
#[inline]
pub fn ok_result(value: Value) -> Value {
    let mut m = HashMap::new();
    m.insert("value".to_string(), value);
    Value::from_map_boxed_typed_v(m, SUBTYPE_RESULT, VARIANT_OK)
}

/// Create an Err result with the given error kind, message, and code.
///
/// Used internally by io, proc, and other modules to return error results.
#[inline]
pub fn err_result(kind: &str, message: &str, code: i64) -> Value {
    let mut error = HashMap::new();
    error.insert("kind".to_string(), Value::from_str_boxed(kind.to_string()));
    error.insert("message".to_string(), Value::from_str_boxed(message.to_string()));
    error.insert("code".to_string(), Value::from_i48(code));
    let error_val = Value::from_map_boxed_typed_v(error, SUBTYPE_ERROR, VARIANT_ERR);

    let mut m = HashMap::new();
    m.insert("error".to_string(), error_val);
    Value::from_map_boxed_typed_v(m, SUBTYPE_RESULT, VARIANT_ERR)
}
