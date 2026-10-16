//! Core traits for xcl-plugins.
//!
//! Defines the fundamental traits used across the standard library.

/// Trait for types that can be converted to an XCL Value.
pub trait IntoXclValue {
    fn into_xcl_value(self) -> xcl_vm::Value;
}

/// Trait for types that can be created from an XCL Value.
pub trait FromXclValue {
    fn from_xcl_value(value: &xcl_vm::Value) -> Option<Self>
    where
        Self: Sized;
}

impl IntoXclValue for i64 {
    fn into_xcl_value(self) -> xcl_vm::Value {
        xcl_vm::Value::from_i48(self)
    }
}

impl IntoXclValue for f64 {
    fn into_xcl_value(self) -> xcl_vm::Value {
        xcl_vm::Value::from_f64(self)
    }
}

impl IntoXclValue for bool {
    fn into_xcl_value(self) -> xcl_vm::Value {
        xcl_vm::Value::bool_(self)
    }
}

impl IntoXclValue for String {
    fn into_xcl_value(self) -> xcl_vm::Value {
        xcl_vm::Value::from_str_boxed(self)
    }
}

impl IntoXclValue for &str {
    fn into_xcl_value(self) -> xcl_vm::Value {
        xcl_vm::Value::from_str_boxed(self.to_string())
    }
}
