//! XCL language support module.
//!
//! Provides introspection and reflection capabilities for XCL code.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the type name of a value.
#[xcl_fn]
pub fn lang_type_of(args: &[Value]) -> Value {
    let v = args[0];
    let ty = if v.is_nil() {
        "nil"
    } else if v.is_bool() {
        "bool"
    } else if v.is_int() {
        "int"
    } else if v.is_float() {
        "float"
    } else if v.is_ptr() {
        match v.heap_kind() {
            Some(0) => "str",
            Some(1) => "array",
            Some(2) => "map",
            Some(3) => "func",
            Some(6) => "handle",
            _ => "unknown",
        }
    } else {
        "unknown"
    };
    Value::from_str_boxed(ty.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcl_vm::{evaluate, VM};

    fn eval(source: &str) -> Value {
        let mut vm = VM::new();
        vm.register_all();
        evaluate(source, &mut vm)
    }

    #[test]
    fn enum_without_leading_pipe() {
        // Movement = Left | Right | — enum-style without leading |
        let result = eval("Movement = Left | Right |; Left");
        assert_eq!(result.as_str(), Some("Left"));
    }

    #[test]
    fn enum_with_leading_pipe() {
        // Movement = | Left | Right | — old syntax still works
        let result = eval("Movement = | Left | Right |; Right");
        assert_eq!(result.as_str(), Some("Right"));
    }

    #[test]
    fn enum_single_variant() {
        let result = eval("Status = Active |; Active");
        assert_eq!(result.as_str(), Some("Active"));
    }

    #[test]
    fn enum_with_fields() {
        // Color = Red { value: f64 } | Green { value: f64 } |
        let result = eval("Color = Red { value: f64 } | Green { value: f64 } |; Red");
        assert_eq!(result.as_str(), Some("Red"));
    }

    #[test]
    fn enum_type_name_registered() {
        // The type name itself should be registered as a string
        let result = eval("Movement = Left | Right |; Movement");
        assert_eq!(result.as_str(), Some("Movement"));
    }

    #[test]
    fn enum_variant_comparison() {
        let result = eval(r#"
            Direction = Up | Down | Left | Right |
            $d = Up
            $d == Up
        "#);
        assert!(result.as_bool());
    }

    #[test]
    fn enum_variant_not_equal() {
        let result = eval(r#"
            Direction = Up | Down |
            $d = Up
            $d == Down
        "#);
        assert!(!result.as_bool());
    }
}
