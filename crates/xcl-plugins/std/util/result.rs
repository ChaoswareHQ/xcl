//! The XCL `Result` value: `Ok(value)` or `Err(error)`.
//!
//! A result is a map with the VM's dedicated `Result` subtype and variant tag,
//! so it is distinguishable from an ordinary map at the value level:
//!
//! ```xcl
//! $r = util.ok 42
//! util.is_ok $r        # true
//! util.unwrap $r       # 42
//!
//! $e = util.err "boom"
//! util.is_err $e       # true
//! util.unwrap_or $e 0  # 0
//! ```
//!
//! The success payload lives under `Ok`, the failure payload under `Err`.

use std::collections::HashMap;

use xcl_macros::{Xcl, xcl_fn, xcl_impl};
use xcl_vm::{SUBTYPE_RESULT, VARIANT_ERR, VARIANT_OK, Value};

use super::Util;

/// Type descriptor for `Result`.
///
/// The variants are metadata only: values are maps built by [`Util::ok`] and
/// [`Util::err`], not Rust enum instances.
#[derive(Xcl)]
#[xcl(domain = "util", kind = "Sum")]
#[allow(dead_code)]
pub enum Result {
    /// Success payload.
    Ok(Value),
    /// Failure payload.
    Err(Value),
}

fn make(payload: Value, ok: bool) -> Value {
    let mut map = HashMap::with_capacity(1);
    map.insert(
        if ok {
            "Ok".to_string()
        } else {
            "Err".to_string()
        },
        payload,
    );
    Value::from_map_boxed_typed_v(
        map,
        SUBTYPE_RESULT,
        if ok { VARIANT_OK } else { VARIANT_ERR },
    )
}

fn field(value: Value, key: &str) -> Value {
    value
        .as_map()
        .and_then(|map| map.get(key).copied())
        .unwrap_or(Value::nil())
}

#[xcl_impl]
impl Util {
    /// Build a successful result.
    pub fn ok(value: Value) -> Value {
        make(value, true)
    }

    /// Build a failed result.
    pub fn err(error: Value) -> Value {
        make(error, false)
    }

    /// True when the value carries the Result subtype.
    pub fn is_result(value: Value) -> bool {
        value.map_subtype() == Some(SUBTYPE_RESULT)
    }

    /// True for a successful result.
    pub fn is_ok(value: Value) -> bool {
        Self::is_result(value) && value.map_variant() == Some(VARIANT_OK)
    }

    /// True for a failed result.
    pub fn is_err(value: Value) -> bool {
        Self::is_result(value) && value.map_variant() == Some(VARIANT_ERR)
    }

    /// The success payload, or panic with the failure payload.
    pub fn unwrap(value: Value) -> Value {
        if Self::is_ok(value) {
            return field(value, "Ok");
        }
        if Self::is_err(value) {
            let message = format!(
                "unwrap on an Err value: {}",
                field(value, "Err").as_string()
            );
            super::panic::panic_now(Value::from_str_boxed(message));
        }
        super::panic::panic_now(Value::from_str_boxed(
            "unwrap on a non-Result value".to_string(),
        ))
    }

    /// The success payload, or `default` for a failure.
    pub fn unwrap_or(value: Value, default: Value) -> Value {
        if Self::is_ok(value) {
            field(value, "Ok")
        } else {
            default
        }
    }

    /// The success payload, or `none` when the result is not `Ok`.
    pub fn ok_value(value: Value) -> Value {
        if Self::is_ok(value) {
            field(value, "Ok")
        } else {
            Value::nil()
        }
    }

    /// The failure payload, or `none` when the result is not `Err`.
    pub fn err_value(value: Value) -> Value {
        if Self::is_err(value) {
            field(value, "Err")
        } else {
            Value::nil()
        }
    }
}

/// Method-syntax aliases.
///
/// `$r.is_ok` is an `OP_CALL_METHOD`, and method dispatch on a value maps the
/// `Result` subtype to the domain named `result`. The helpers above live in the
/// `util` domain, so they are mirrored here to make `$r.is_ok`, `$r.unwrap()`
/// and friends resolve. Extra arguments follow the receiver, so
/// `$e.unwrap_or(0)` works too.
#[xcl_fn(name = "result.is_result", hot)]
pub fn result_is_result(value: Value) -> Value {
    Value::bool(Util::is_result(value))
}

/// True for a successful result: `$r.is_ok`.
#[xcl_fn(name = "result.is_ok", hot)]
pub fn result_is_ok(value: Value) -> Value {
    Value::bool(Util::is_ok(value))
}

/// True for a failed result: `$r.is_err`.
#[xcl_fn(name = "result.is_err", hot)]
pub fn result_is_err(value: Value) -> Value {
    Value::bool(Util::is_err(value))
}

/// The success payload, or a panic: `$r.unwrap`.
#[xcl_fn(name = "result.unwrap", hot)]
pub fn result_unwrap(value: Value) -> Value {
    Util::unwrap(value)
}

/// The success payload, or a default: `$r.unwrap_or(0)`.
#[xcl_fn(name = "result.unwrap_or", hot)]
pub fn result_unwrap_or(value: Value, default: Value) -> Value {
    Util::unwrap_or(value, default)
}

/// The success payload, or `none`: `$r.ok_value`.
#[xcl_fn(name = "result.ok_value", hot)]
pub fn result_ok_value(value: Value) -> Value {
    Util::ok_value(value)
}

/// The failure payload, or `none`: `$e.err_value`.
#[xcl_fn(name = "result.err_value", hot)]
pub fn result_err_value(value: Value) -> Value {
    Util::err_value(value)
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn result_method_syntax() {
        // `$r.is_ok` dispatches through the `result` domain; the `util` helpers
        // are mirrored there so method syntax resolves.
        assert!(evaluate_new("$r = util.ok 5\n$r.is_ok").as_bool());
        assert!(!evaluate_new("$r = util.ok 5\n$r.is_err").as_bool());
        assert_eq!(evaluate_new("$r = util.ok 5\n$r.unwrap").as_i48(), 5);

        let e = evaluate_new("$e = util.err \"bad\"\n$e.err_value");
        assert_eq!(e.as_str(), Some("bad"));
    }

    #[test]
    fn result_method_in_condition() {
        let src = "$r = util.ok 5\npick() { if $r.is_ok { return 1 } return 0 }\npick";
        assert_eq!(evaluate_new(src).as_i48(), 1);
    }

    #[test]
    fn result_method_with_argument() {
        assert_eq!(
            evaluate_new("$e = util.err \"bad\"\n($e.unwrap_or 0)").as_i48(),
            0
        );
    }

    #[test]
    fn result_round_trip() {
        assert!(evaluate_new("util.is_ok (util.ok 5)").as_bool());
        assert!(!evaluate_new("util.is_err (util.ok 5)").as_bool());
        assert!(evaluate_new("util.is_err (util.err \"x\")").as_bool());
        assert_eq!(evaluate_new("util.unwrap (util.ok 7)").as_i48(), 7);
        assert_eq!(
            evaluate_new("util.unwrap_or (util.err \"x\") 3").as_i48(),
            3
        );
        assert_eq!(evaluate_new("util.ok_value (util.ok 9)").as_i48(), 9);
        assert!(evaluate_new("util.ok_value (util.err \"x\")").is_nil());
        assert_eq!(
            evaluate_new("util.err_value (util.err \"x\")").as_str(),
            Some("x")
        );
        assert!(evaluate_new("util.is_result (util.ok 5)").as_bool());
        assert!(!evaluate_new("util.is_result 5").as_bool());
    }
}
