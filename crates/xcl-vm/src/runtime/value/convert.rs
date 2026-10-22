//! Conversions: `From` impls and the native-argument traits.

use super::*;

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::from_i48(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::from_i48(v as i64)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::from_i48(v as i64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::from_f64(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::bool(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::from_str_boxed(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::from_str_boxed(v.to_string())
    }
}

// — Argument extraction for typed native functions —

/// Extracts a typed Rust argument from a `&[Value]` slice at the given index.
///
/// If `index` is out of bounds, returns the default value for `T`.
///
/// # Examples
///
/// ```
/// use xcl_vm::runtime::{extract_arg, Value};
///
/// let args = [Value::from_i48(100), Value::bool(true)];
/// let first: i64 = extract_arg(&args, 0);
/// let second: bool = extract_arg(&args, 1);
///
/// assert_eq!(first, 100);
/// assert_eq!(second, true);
/// ```
pub fn extract_arg<T: NativeArg>(args: &[Value], index: usize) -> T {
    let v = args.get(index).copied().unwrap_or(Value::nil());
    T::from_value(v)
}

/// Trait for types that can be automatically converted from a [`Value`].
pub trait NativeArg: Sized {
    /// Converts a [`Value`] into `Self`.
    fn from_value(v: Value) -> Self;
}

impl NativeArg for &str {
    fn from_value(v: Value) -> Self {
        v.as_str().unwrap_or("")
    }
}

impl NativeArg for String {
    fn from_value(v: Value) -> Self {
        v.as_str().unwrap_or("").to_string()
    }
}

impl NativeArg for i64 {
    fn from_value(v: Value) -> Self {
        v.as_i48()
    }
}

impl NativeArg for i32 {
    fn from_value(v: Value) -> Self {
        v.as_i48() as i32
    }
}

impl NativeArg for u64 {
    fn from_value(v: Value) -> Self {
        v.as_i48() as u64
    }
}

impl NativeArg for f64 {
    fn from_value(v: Value) -> Self {
        v.as_float()
    }
}

impl NativeArg for bool {
    fn from_value(v: Value) -> Self {
        v.as_bool()
    }
}

impl NativeArg for Value {
    fn from_value(v: Value) -> Self {
        v
    }
}

impl NativeArg for usize {
    fn from_value(v: Value) -> Self {
        v.as_i48() as usize
    }
}

// — Custom type to Value conversion —

/// Trait for converting custom Rust structs into a typed [`Value`] map.
///
/// The `#[xcl_impl]` macro detects when a method returns a type that implements
/// this trait and calls `.into_value()` automatically, so the method returns
/// a properly typed map instead of a generic `Value`.
///
/// # Example
///
/// ```ignore
/// #[derive(Xcl)]
/// pub struct HttpResponse;
///
/// impl IntoValue for HttpResponse {
///     fn into_value(self) -> Value {
///         let mut m = HashMap::new();
///         m.insert("status".to_string(), Value::from_i48(self.status));
///         m.insert("body".to_string(), Value::from_str_boxed(self.body));
///         Value::from_map_boxed(m)
///     }
/// }
/// ```
pub trait IntoValue {
    /// Convert this struct into a [`Value`] map.
    fn into_value(self) -> Value;
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::from_str_boxed(self)
    }
}

impl IntoValue for i64 {
    fn into_value(self) -> Value {
        Value::from_i48(self)
    }
}

impl IntoValue for i32 {
    fn into_value(self) -> Value {
        Value::from_i48(self as i64)
    }
}

impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::from_f64(self)
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::bool(self)
    }
}

impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        let arr: Vec<Value> = self.into_iter().map(|t| t.into_value()).collect();
        Value::from_array_boxed(arr)
    }
}
