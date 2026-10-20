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

use xcl_macros::{Xcl, xcl_impl};
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

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

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
