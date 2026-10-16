//! Java type mapping helpers.
//!
//! Provides functions for mapping IR types to Java-specific type names
//! with proper boxing/unboxing handling.

use crate::translate::ir::types::IrType;

/// Map an IR type to a Java primitive type name, if applicable.
pub fn to_java_primitive(ir_type: &IrType) -> Option<&'static str> {
    match ir_type {
        IrType::Bool => Some("boolean"),
        IrType::Int => Some("int"),
        IrType::Float => Some("double"),
        _ => None,
    }
}

/// Map an IR type to a Java boxed type name.
pub fn to_java_boxed(ir_type: &IrType) -> Option<&'static str> {
    match ir_type {
        IrType::Bool => Some("Boolean"),
        IrType::Int => Some("Integer"),
        IrType::Float => Some("Double"),
        IrType::Str => Some("String"),
        _ => None,
    }
}

/// Get the Java array type name.
pub fn java_array_type(element_type: &str) -> String {
    format!("{}[]", element_type)
}
