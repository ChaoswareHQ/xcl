//! String operations for xcl-plugins.
//!
//! Provides functions for manipulating strings: split, join, trim, replace, contains, etc.

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Split a string by a delimiter.
#[xcl_fn(name = "str.split", args = "s: str; delimiter: str", ret = "array")]
pub fn str_split(ctx: &mut DomainCtx) -> Value {
    let s = ctx.arg(0).as_str().unwrap_or("");
    let delimiter = ctx.arg(1).as_str().unwrap_or("");
    let parts: Vec<Value> = s.split(delimiter)
        .map(|p| Value::from_str_boxed(p.to_string()))
        .collect();
    Value::from_array(parts, ctx.arena())
}

/// Join an array of strings with a separator.
#[xcl_fn(name = "str.join", args = "arr: array; separator: str", ret = "str")]
pub fn str_join(args: &[Value]) -> Value {
    let empty: Vec<Value> = Vec::new();
    let arr = args[0].as_array().unwrap_or(&empty);
    let separator = args[1].as_str().unwrap_or("");
    let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    Value::from_str_boxed(parts.join(separator))
}

/// Trim whitespace from both ends of a string.
#[xcl_fn(name = "str.trim", args = "s: str", ret = "str")]
pub fn str_trim(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    Value::from_str_boxed(s.trim().to_string())
}

/// Replace all occurrences of a substring.
#[xcl_fn(name = "str.replace", args = "s: str; from: str; to: str", ret = "str")]
pub fn str_replace(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    let from = args[1].as_str().unwrap_or("");
    let to = args[2].as_str().unwrap_or("");
    Value::from_str_boxed(s.replace(from, to))
}

/// Check if a string contains a substring.
#[xcl_fn(name = "str.contains", args = "s: str; substring: str", ret = "bool")]
pub fn str_contains(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    let substring = args[1].as_str().unwrap_or("");
    Value::bool_(s.contains(substring))
}

/// Check if a string starts with a prefix.
#[xcl_fn(name = "str.starts_with", args = "s: str; prefix: str", ret = "bool")]
pub fn str_starts_with(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    let prefix = args[1].as_str().unwrap_or("");
    Value::bool_(s.starts_with(prefix))
}

/// Check if a string ends with a suffix.
#[xcl_fn(name = "str.ends_with", args = "s: str; suffix: str", ret = "bool")]
pub fn str_ends_with(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    let suffix = args[1].as_str().unwrap_or("");
    Value::bool_(s.ends_with(suffix))
}

/// Convert a string to uppercase.
#[xcl_fn(name = "str.upper", args = "s: str", ret = "str")]
pub fn str_upper(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    Value::from_str_boxed(s.to_uppercase())
}

/// Convert a string to lowercase.
#[xcl_fn(name = "str.lower", args = "s: str", ret = "str")]
pub fn str_lower(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    Value::from_str_boxed(s.to_lowercase())
}

/// Get the length of a string in characters.
#[xcl_fn(name = "str.len", args = "s: str", ret = "int")]
pub fn str_len(args: &[Value]) -> Value {
    let s = args[0].as_str().unwrap_or("");
    Value::from_i48(s.chars().count() as i64)
}
