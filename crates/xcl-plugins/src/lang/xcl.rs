//! Universal XCL value translator.
//!
//! Provides bidirectional conversion between XCL `Value` and external data formats.
//! This is the core protocol adapter layer — any language or format can translate
//! to/from XCL through this module.
//!
//! ## Architecture
//!
//! ```text
//!  ┌──────────┐     ┌──────────────┐     ┌──────────┐
//!  │  XCL     │────▶│  XclValue    │◀────│  JSON    │
//!  │  Value   │◀────│  (universal) │────▶│  object  │
//!  └──────────┘     └──────────────┘     └──────────┘
//!                          │
//!                          ▼
//!                    ┌──────────┐
//!                    │  Future  │
//!                    │ formats  │
//!                    └──────────┘
//! ```
//!
//! `XclValue` is an owned, format-agnostic representation of an XCL value
//! that can be serialized to any external format (JSON, MessagePack, etc.)
//! and reconstructed back into a `Value`.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use xcl_vm::Value;
use crate::encoding::json;

/// Format-agnostic representation of an XCL value.
///
/// This type mirrors the XCL `Value` type but is owned and serializable
/// to any format. It is the intermediate representation for protocol
/// translation between XCL and external systems.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum XclValue {
    /// XCL `none` / nil
    Nil,
    /// XCL boolean
    Bool(bool),
    /// XCL integer (48-bit signed)
    Int(i64),
    /// XCL float (64-bit)
    Float(f64),
    /// XCL string
    Str(String),
    /// XCL array
    Arr(Vec<XclValue>),
    /// XCL dict / map
    Map(HashMap<String, XclValue>),
}

impl XclValue {
    /// Convert an XCL `Value` into an `XclValue`.
    ///
    /// Returns `None` for unserializable types (functions, handles).
    pub fn from_value(v: Value) -> Option<Self> {
        if v.is_nil() {
            return Some(XclValue::Nil);
        }
        if v.is_bool() {
            return Some(XclValue::Bool(v.as_bool()));
        }
        if v.is_int() {
            return Some(XclValue::Int(v.as_i48()));
        }
        if v.is_float() {
            return Some(XclValue::Float(v.as_f64()));
        }
        if let Some(s) = v.as_str() {
            return Some(XclValue::Str(s.to_string()));
        }
        if let Some(arr) = v.as_array() {
            let items: Vec<XclValue> = arr.iter()
                .filter_map(|x| XclValue::from_value(*x))
                .collect();
            return Some(XclValue::Arr(items));
        }
        if let Some(map) = v.as_map() {
            let m: HashMap<String, XclValue> = map.iter()
                .filter_map(|(k, val)| {
                    XclValue::from_value(*val).map(|v| (k.clone(), v))
                })
                .collect();
            return Some(XclValue::Map(m));
        }
        None
    }

    /// Convert an `XclValue` back into an XCL `Value`.
    pub fn to_value(self) -> Value {
        match self {
            XclValue::Nil => Value::nil(),
            XclValue::Bool(b) => Value::bool(b),
            XclValue::Int(i) => Value::from_i48(i),
            XclValue::Float(f) => Value::from_f64(f),
            XclValue::Str(s) => Value::from_str_boxed(s),
            XclValue::Arr(items) => {
                let values: Vec<Value> = items.into_iter().map(|x| x.to_value()).collect();
                Value::from_array_boxed(values)
            }
            XclValue::Map(map) => {
                let m: HashMap<String, Value> = map.into_iter()
                    .map(|(k, v)| (k, v.to_value()))
                    .collect();
                Value::from_map_boxed(m)
            }
        }
    }

    /// Convert an `XclValue` to a JSON string.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse an `XclValue` from a JSON string.
    pub fn from_json_string(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

/// Convert an XCL `Value` to a JSON string via the universal translator.
///
/// This is a convenience wrapper around `XclValue::from_value` + `to_json_string`.
pub fn value_to_json_string(v: Value) -> Option<String> {
    XclValue::from_value(v).map(|xv| xv.to_json_string())
}

/// Parse a JSON string into an XCL `Value` via the universal translator.
pub fn json_string_to_value(s: &str) -> Value {
    match XclValue::from_json_string(s) {
        Some(xv) => xv.to_value(),
        None => json::parse_json(s),  // fallback to direct JSON parsing
    }
}

// ─── XCL builtins ───

use xcl_macros::xcl_fn;

/// Convert an XCL value to a universal JSON representation.
///
/// Produces a tagged JSON format that preserves XCL type information,
/// enabling round-trip conversion between any language and XCL.
///
/// ## SYNTAX
///
/// ```xcl
/// fn to_any($value: any) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$value`: The XCL value to convert.
///
/// ## EXAMPLES
///
/// ```xcl
/// $data = xcl.to_any { name: "Alice", age: 30 }
/// # Output: {"type":"Map","value":{"name":{"type":"Str","value":"Alice"},...}}
/// ```
///
/// ## INPUT TYPES
///
/// - `any`
///
/// ## OUTPUT TYPES
///
/// - `str`
///
/// ## NOTES
///
/// - The tagged format preserves exact XCL types across language boundaries.
/// - Use `xcl.from_any` to reconstruct the value.
/// - Functions and handles return empty string.
#[xcl_fn(name = "xcl.to_any", args = "value: any", ret = "str")]
pub fn xcl_to_any(args: &[Value]) -> Value {
    let v = args.first().copied().unwrap_or(Value::nil());
    match value_to_json_string(v) {
        Some(s) => Value::from_str_boxed(s),
        None => Value::from_str_boxed(String::new()),
    }
}

/// Reconstruct an XCL value from a universal JSON representation.
///
/// Parses the tagged JSON format produced by `xcl.to_any` and reconstructs
/// the original XCL value with preserved types.
///
/// ## SYNTAX
///
/// ```xcl
/// fn from_any($json: str) -> any
/// ```
///
/// ## PARAMETERS
///
/// `$json`: A tagged JSON string from `xcl.to_any`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $json = '{"type":"Int","value":42}'
/// $num = xcl.from_any $json
/// io.print $num  # 42
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `any`
///
/// ## NOTES
///
/// - Falls back to standard JSON parsing if the tagged format is not recognized.
/// - Returns `none` on parse error.
#[xcl_fn(name = "xcl.from_any", args = "json: str", ret = "any")]
pub fn xcl_from_any(json_str: &str) -> Value {
    json_string_to_value(json_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xcl_value_nil_roundtrip() {
        let xv = XclValue::from_value(Value::nil()).unwrap();
        assert!(matches!(xv, XclValue::Nil));
        assert!(xv.to_value().is_nil());
    }

    #[test]
    fn xcl_value_int_roundtrip() {
        let v = Value::from_i48(42);
        let xv = XclValue::from_value(v).unwrap();
        assert!(matches!(&xv, XclValue::Int(n) if *n == 42));
        assert_eq!(xv.to_value().as_i48(), 42);
    }

    #[test]
    fn xcl_value_nested_roundtrip() {
        let items = vec![
            Value::from_str_boxed("hello".to_string()),
            Value::from_i48(42),
        ];
        let v = Value::from_array_boxed(items);
        let xv = XclValue::from_value(v).unwrap();
        let back = xv.to_value();
        let arr = back.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str(), Some("hello"));
        assert_eq!(arr[1].as_i48(), 42);
    }

    #[test]
    fn xcl_value_json_serialization() {
        let xv = XclValue::Int(42);
        let json = xv.to_json_string();
        let back = XclValue::from_json_string(&json).unwrap();
        assert_eq!(xv, back);
    }

    #[test]
    fn value_to_json_string_via_translator() {
        let v = Value::from_i48(100);
        let s = value_to_json_string(v).unwrap();
        assert!(s.contains("Int"));
        assert!(s.contains("100"));
    }

    #[test]
    fn json_string_to_value_via_translator() {
        let v = json_string_to_value(r#"{"type":"Int","value":100}"#);
        assert_eq!(v.as_i48(), 100);
    }
}
