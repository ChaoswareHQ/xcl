//! Error type and construction helpers.
//!
//! Every error in XCL is an `Error` record with three fields:
//! `kind` (portable discriminator), `message` (human-readable), and `code` (raw OS errno).

use std::collections::HashMap;
use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::{Value, SUBTYPE_ERROR, VARIANT_DEFAULT};

/// The Error type registered with the VM.
#[xcl_type(
    name = "Error",
    kind = "Record",
    summary = "An error value. `kind` is a portable discriminator \
               (`not_found`, `permission_denied`, ...), `message` is \
               human-readable with path/context, `code` is the raw OS \
               errno (0 if unknown).",
    fields(
        kind: "str",
        message: "str",
        code: "i64",
    ),
)]
pub struct Error;

/// Create an Error value with the given kind, message, and code.
pub fn make_error(kind: &str, message: &str, code: i64) -> Value {
    let mut m = HashMap::with_capacity(3);
    m.insert("kind".into(),    Value::from_str_boxed(kind.into()));
    m.insert("message".into(), Value::from_str_boxed(message.into()));
    m.insert("code".into(),    Value::from_i48(code));
    Value::from_map_boxed_typed_v(m, SUBTYPE_ERROR, VARIANT_DEFAULT)
}

/// Top-level builtin: construct an Error value.
#[xcl_fn(name = "make_error", args = "kind: str; message: str; code: i64", ret = "Error")]
fn make_error_fn(args: &[Value]) -> Value {
    let kind = args.first().and_then(|v| v.as_str()).unwrap_or("other");
    let message = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
    let code = args.get(2).map(|v| v.as_i48()).unwrap_or(0);
    make_error(kind, message, code)
}

/// Extract the kind string from an Error value.
pub fn error_kind(e: &Value) -> Option<&'static str> {
    e.as_map()?.get("kind")?.as_str()
}

/// Extract the message string from an Error value.
pub fn error_message(e: &Value) -> Option<&'static str> {
    e.as_map()?.get("message")?.as_str()
}

/// Extract the code (i64) from an Error value, defaulting to 0.
pub fn error_code(e: &Value) -> i64 {
    e.as_map()
        .and_then(|m| m.get("code"))
        .map(|v| v.as_i48())
        .unwrap_or(0)
}
