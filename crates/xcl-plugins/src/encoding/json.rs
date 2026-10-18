<<<<<<< HEAD
=======
//! JSON encoding and decoding for XCL values.
//!
//! Converts between XCL `Value` and `serde_json::Value` bidirectionally.
//! XCL maps → JSON objects, XCL arrays → JSON arrays, primitives map directly.
//!
//! ## Design
//!
//! - `Value::nil` → `null`
//! - `Value::bool` → `true`/`false`
//! - `Value::int` → JSON number (integer)
//! - `Value::float` → JSON number (float)
//! - `Value::str` → JSON string
//! - `Value::array` → JSON array
//! - `Value::map` → JSON object
//! - `Value::func` → `null` (functions cannot be serialized)
//!
//! For the reverse direction, JSON numbers are stored as int when possible,
//! otherwise as float.

use std::collections::HashMap;
use xcl_vm::Value;

/// Convert an XCL `Value` into a `serde_json::Value`.
///
/// Returns `None` for function values (not serializable).
pub fn value_to_json(v: Value) -> Option<serde_json::Value> {
    if v.is_nil() {
        return Some(serde_json::Value::Null);
    }
    if v.is_bool() {
        return Some(serde_json::Value::Bool(v.as_bool()));
    }
    if v.is_int() {
        let n = v.as_i48();
        // JSON numbers are doubles; use Number for integers when they fit
        let num = serde_json::Number::from(n);
        return Some(serde_json::Value::Number(num));
    }
    if v.is_float() {
        let f = v.as_f64();
        if let Some(num) = serde_json::Number::from_f64(f) {
            return Some(serde_json::Value::Number(num));
        }
        return None;
    }
    if let Some(s) = v.as_str() {
        return Some(serde_json::Value::String(s.to_string()));
    }
    if let Some(arr) = v.as_array() {
        let items: Vec<serde_json::Value> = arr.iter()
            .filter_map(|x| value_to_json(*x))
            .collect();
        return Some(serde_json::Value::Array(items));
    }
    if let Some(map) = v.as_map() {
        let obj: serde_json::Map<String, serde_json::Value> = map.iter()
            .filter_map(|(k, val)| {
                value_to_json(*val).map(|v| (k.clone(), v))
            })
            .collect();
        return Some(serde_json::Value::Object(obj));
    }
    // Functions and handles cannot be serialized
    None
}

/// Convert a `serde_json::Value` into an XCL `Value`.
///
/// JSON numbers are stored as int when they have no fractional part,
/// otherwise as float.
pub fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::nil(),
        serde_json::Value::Bool(b) => Value::bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from_i48(i)
            } else if let Some(f) = n.as_f64() {
                Value::from_f64(f)
            } else {
                Value::nil()
            }
        }
        serde_json::Value::String(s) => Value::from_str_boxed(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(json_to_value).collect();
            Value::from_array_boxed(items)
        }
        serde_json::Value::Object(obj) => {
            let map: HashMap<String, Value> = obj.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::from_map_boxed(map)
        }
    }
}

/// Parse a JSON string into an XCL `Value`.
///
/// Returns `Value::nil()` on parse error.
pub fn parse_json(s: &str) -> Value {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(v) => json_to_value(&v),
        Err(_) => Value::nil(),
    }
}

/// Serialize an XCL `Value` to a JSON string.
///
/// Returns `None` if the value cannot be serialized (e.g., a function).
pub fn to_json_string(v: Value) -> Option<String> {
    let json = value_to_json(v)?;
    Some(serde_json::to_string(&json).unwrap_or_default())
}

/// Serialize an XCL `Value` to a pretty-printed JSON string.
///
/// Returns `None` if the value cannot be serialized.
pub fn to_json_pretty(v: Value) -> Option<String> {
    let json = value_to_json(v)?;
    Some(serde_json::to_string_pretty(&json).unwrap_or_default())
}

// ─── XCL builtins ───

use xcl_macros::xcl_fn;

/// Parse a JSON string into an XCL value.
///
/// Converts JSON text into native XCL types: objects become dicts,
/// arrays become arrays, numbers become int or float, strings stay strings.
///
/// ## SYNTAX
///
/// ```xcl
/// fn parse($json: str) -> any
/// ```
///
/// ## PARAMETERS
///
/// `$json`: A JSON-encoded string to parse.
///
/// ## EXAMPLES
///
/// ```xcl
/// $data = json.parse '{"name": "Alice", "age": 30}'
/// io.print $data.name
/// ```
///
/// ```xcl
/// $items = json.parse '[1, 2, 3]'
/// io.print (arr.sum $items)
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `any` (dict, arr, int, float, str, or bool depending on JSON root)
///
/// ## NOTES
///
/// - Returns `none` if the JSON string is malformed.
/// - JSON `null` becomes XCL `none`.
/// - JSON numbers without decimals become XCL int; with decimals become float.
#[xcl_fn]
pub fn json_parse(json_str: &str) -> Value {
    parse_json(json_str)
}

/// Serialize an XCL value to a JSON string.
///
/// Converts any XCL value into its JSON representation. Dicts become objects,
/// arrays become arrays, primitives map directly.
///
/// ## SYNTAX
///
/// ```xcl
/// fn stringify($value: any) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$value`: The XCL value to serialize to JSON.
///
/// ## EXAMPLES
///
/// ```xcl
/// $obj = { name: "Alice", age: 30 }
/// $json = json.stringify $obj
/// io.print $json  # '{"age":30,"name":"Alice"}'
/// ```
///
/// ```xcl
/// $arr = [1; "hello"; true; none]
/// json.stringify $arr  # '[1,"hello",true,null]'
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
/// - Returns empty string `""` if the value cannot be serialized (e.g., functions).
/// - Key order in objects is not guaranteed.
/// - Use `json.pretty` for formatted output.
#[xcl_fn]
pub fn json_stringify(args: &[Value]) -> Value {
    let v = args.first().copied().unwrap_or(Value::nil());
    match to_json_string(v) {
        Some(s) => Value::from_str_boxed(s),
        None => Value::from_str_boxed(String::new()),
    }
}

/// Serialize an XCL value to a pretty-printed JSON string.
///
/// Same as `json.stringify` but with indentation for readability.
///
/// ## SYNTAX
///
/// ```xcl
/// fn pretty($value: any) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$value`: The XCL value to serialize.
///
/// ## EXAMPLES
///
/// ```xcl
/// $obj = { name: "Alice", scores: [90; 85; 95] }
/// io.print (json.pretty $obj)
/// ```
///
/// ## INPUT TYPES
///
/// - `any`
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn]
pub fn json_pretty(args: &[Value]) -> Value {
    let v = args.first().copied().unwrap_or(Value::nil());
    match to_json_pretty(v) {
        Some(s) => Value::from_str_boxed(s),
        None => Value::from_str_boxed(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nil_to_json() {
        assert_eq!(value_to_json(Value::nil()), Some(serde_json::Value::Null));
    }

    #[test]
    fn bool_to_json() {
        assert_eq!(value_to_json(Value::bool(true)), Some(serde_json::Value::Bool(true)));
        assert_eq!(value_to_json(Value::bool(false)), Some(serde_json::Value::Bool(false)));
    }

    #[test]
    fn int_to_json() {
        let v = Value::from_i48(42);
        let j = value_to_json(v).unwrap();
        assert_eq!(j.as_i64(), Some(42));
    }

    #[test]
    fn float_to_json() {
        let v = Value::from_f64(3.14);
        let j = value_to_json(v).unwrap();
        assert_eq!(j.as_f64(), Some(3.14));
    }

    #[test]
    fn string_to_json() {
        let v = Value::from_str_boxed("hello".to_string());
        let j = value_to_json(v).unwrap();
        assert_eq!(j.as_str(), Some("hello"));
    }

    #[test]
    fn array_to_json() {
        let items = vec![Value::from_i48(1), Value::from_i48(2), Value::from_i48(3)];
        let v = Value::from_array_boxed(items);
        let j = value_to_json(v).unwrap();
        assert!(j.is_array());
        assert_eq!(j.as_array().unwrap().len(), 3);
    }

    #[test]
    fn map_to_json() {
        let mut m = HashMap::new();
        m.insert("key".to_string(), Value::from_i48(42));
        let v = Value::from_map_boxed(m);
        let j = value_to_json(v).unwrap();
        assert!(j.is_object());
        assert_eq!(j.get("key").unwrap().as_i64(), Some(42));
    }

    #[test]
    fn json_to_value_roundtrip() {
        let json = serde_json::json!({"name": "Alice", "age": 30, "scores": [90, 85]});
        let v = json_to_value(&json);
        let back = value_to_json(v).unwrap();
        assert_eq!(back, json);
    }

    #[test]
    fn parse_json_string() {
        let v = parse_json(r#"{"x": 1}"#);
        assert!(v.as_map().is_some());
        assert_eq!(v.as_map().unwrap().get("x").unwrap().as_i48(), 1);
    }

    #[test]
    fn parse_invalid_json() {
        let v = parse_json("not json");
        assert!(v.is_nil());
    }

    #[test]
    fn to_json_string_output() {
        let v = Value::from_i48(42);
        let s = to_json_string(v).unwrap();
        assert_eq!(s, "42");
    }
}
>>>>>>> 1aea8a05633db723e7f0ce648f5ca4182fce7072
