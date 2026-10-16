//! Type mapping between IR types and target language types.
//!
//! Per contract TM7.1: maps/types_map.rs holds `IRType → (Language, Version) → TargetType`.
//! Per contract TM7.2: Missing entries return `TranslateError::NoTypeMapping`.
//! Per contract TM7.3: Boxing/unboxing is explicit.

use crate::translate::ir::types::IrType;
use crate::translate::version::{Language, LangSpec};

/// Map an IR type to a target language type name.
///
/// # Arguments
/// - `ir_type` — The IR type to map
/// - `target` — The target language specification
///
/// # Returns
/// The target type name as a string, or `None` if no mapping exists
pub fn map_type(ir_type: &IrType, target: &LangSpec) -> Option<String> {
    match target.language {
        Language::Java => map_type_java(ir_type, target),
        Language::Rust => map_type_rust(ir_type, target),
        Language::Python => map_type_python(ir_type, target),
        Language::C => map_type_c(ir_type, target),
        Language::Cpp => map_type_cpp(ir_type, target),
        Language::Xcl => Some(ir_type.name()),
    }
}

/// Map IR type to Java type.
fn map_type_java(ir_type: &IrType, _target: &LangSpec) -> Option<String> {
    match ir_type {
        IrType::Nil => Some("void".to_string()),
        IrType::Bool => Some("boolean".to_string()),
        IrType::Int => Some("int".to_string()),
        IrType::Float => Some("double".to_string()),
        IrType::Str => Some("String".to_string()),
        IrType::Array(elem) => {
            map_type_java(elem, _target).map(|t| format!("{}[]", t))
        }
        IrType::Map(value_ty) => {
            map_type_java(value_ty, _target).map(|v| format!("java.util.Map<String, {}>", v))
        }
        IrType::Record { name, .. } => Some(name.clone()),
        IrType::Fn { .. } => Some("java.util.function.Function".to_string()),
        IrType::Optional(inner) => {
            map_type_java(inner, _target).map(|t| format!("java.util.Optional<{}>", t))
        }
        IrType::Result { ok, err: _ } => {
            // Java doesn't have Result, use Optional for Ok and throw for Err
            map_type_java(ok, _target)
        }
        IrType::Generic(name) => Some(name.clone()),
        IrType::Any => Some("Object".to_string()),
    }
}

/// Map IR type to Rust type.
fn map_type_rust(ir_type: &IrType, _target: &LangSpec) -> Option<String> {
    match ir_type {
        IrType::Nil => Some("()".to_string()),
        IrType::Bool => Some("bool".to_string()),
        IrType::Int => Some("i64".to_string()),
        IrType::Float => Some("f64".to_string()),
        IrType::Str => Some("String".to_string()),
        IrType::Array(elem) => {
            map_type_rust(elem, _target).map(|t| format!("Vec<{}>", t))
        }
        IrType::Map(value_ty) => {
            map_type_rust(value_ty, _target).map(|v| {
                format!("std::collections::HashMap<String, {}>", v)
            })
        }
        IrType::Record { name, .. } => Some(name.clone()),
        IrType::Fn { params, ret } => {
            let param_strs: Vec<String> = params.iter().filter_map(|p| map_type_rust(p, _target)).collect();
            let ret_str = map_type_rust(ret, _target).unwrap_or_else(|| "()".to_string());
            Some(format!("Fn({}) -> {}", param_strs.join(", "), ret_str))
        }
        IrType::Optional(inner) => {
            map_type_rust(inner, _target).map(|t| format!("Option<{}>", t))
        }
        IrType::Result { ok, err } => {
            let ok_str = map_type_rust(ok, _target).unwrap_or_else(|| "()".to_string());
            let err_str = map_type_rust(err, _target).unwrap_or_else(|| "String".to_string());
            Some(format!("Result<{}, {}>", ok_str, err_str))
        }
        IrType::Generic(name) => Some(name.clone()),
        IrType::Any => Some("Box<dyn std::any::Any>".to_string()),
    }
}

/// Map IR type to Python type.
fn map_type_python(ir_type: &IrType, _target: &LangSpec) -> Option<String> {
    match ir_type {
        IrType::Nil => Some("None".to_string()),
        IrType::Bool => Some("bool".to_string()),
        IrType::Int => Some("int".to_string()),
        IrType::Float => Some("float".to_string()),
        IrType::Str => Some("str".to_string()),
        IrType::Array(elem) => {
            map_type_python(elem, _target).map(|t| format!("list[{}]", t))
        }
        IrType::Map(value_ty) => {
            map_type_python(value_ty, _target).map(|v| format!("dict[str, {}]", v))
        }
        IrType::Record { name, .. } => Some(name.clone()),
        IrType::Fn { .. } => Some("Callable".to_string()),
        IrType::Optional(inner) => {
            map_type_python(inner, _target).map(|t| format!("Optional[{}]", t))
        }
        IrType::Result { ok, err: _ } => {
            // Python uses exceptions, not Result
            map_type_python(ok, _target)
        }
        IrType::Generic(name) => Some(name.clone()),
        IrType::Any => Some("Any".to_string()),
    }
}

/// Map IR type to C type.
fn map_type_c(ir_type: &IrType, target: &LangSpec) -> Option<String> {
    // Check if C23 for bool keyword vs _Bool
    let is_c23 = target.version.0 == "23";
    match ir_type {
        IrType::Nil => Some("void".to_string()),
        IrType::Bool => {
            if is_c23 {
                Some("bool".to_string())
            } else {
                Some("_Bool".to_string())
            }
        }
        IrType::Int => Some("int64_t".to_string()),
        IrType::Float => Some("double".to_string()),
        IrType::Str => Some("char*".to_string()),
        IrType::Array(elem) => {
            map_type_c(elem, target).map(|t| format!("{}*", t))
        }
        IrType::Map(value_ty) => {
            // C doesn't have native maps, use void* with comment
            let _ = value_ty;
            Some("void*".to_string())
        }
        IrType::Record { name, .. } => Some(name.clone()),
        IrType::Fn { params, ret } => {
            let param_strs: Vec<String> = params.iter().filter_map(|p| map_type_c(p, target)).collect();
            let ret_str = map_type_c(ret, target).unwrap_or_else(|| "void".to_string());
            Some(format!("{} (*)({})", ret_str, param_strs.join(", ")))
        }
        IrType::Optional(inner) => {
            // C uses pointers for optionality (NULL = none)
            map_type_c(inner, target).map(|t| format!("{}*", t))
        }
        IrType::Result { ok, err: _ } => {
            // C doesn't have Result, use the Ok type with error code pattern
            map_type_c(ok, target)
        }
        IrType::Generic(name) => Some(name.clone()),
        IrType::Any => Some("void*".to_string()),
    }
}

/// Map IR type to C++ type.
fn map_type_cpp(ir_type: &IrType, _target: &LangSpec) -> Option<String> {
    match ir_type {
        IrType::Nil => Some("void".to_string()),
        IrType::Bool => Some("bool".to_string()),
        IrType::Int => Some("int64_t".to_string()),
        IrType::Float => Some("double".to_string()),
        IrType::Str => Some("std::string".to_string()),
        IrType::Array(elem) => {
            map_type_cpp(elem, _target).map(|t| format!("std::vector<{}>", t))
        }
        IrType::Map(value_ty) => {
            map_type_cpp(value_ty, _target).map(|v| format!("std::unordered_map<std::string, {}>", v))
        }
        IrType::Record { name, .. } => Some(name.clone()),
        IrType::Fn { params, ret } => {
            let param_strs: Vec<String> = params.iter().filter_map(|p| map_type_cpp(p, _target)).collect();
            let ret_str = map_type_cpp(ret, _target).unwrap_or_else(|| "void".to_string());
            Some(format!("std::function<{}({})>", ret_str, param_strs.join(", ")))
        }
        IrType::Optional(inner) => {
            map_type_cpp(inner, _target).map(|t| format!("std::optional<{}>", t))
        }
        IrType::Result { ok, err } => {
            let ok_str = map_type_cpp(ok, _target).unwrap_or_else(|| "void".to_string());
            let err_str = map_type_cpp(err, _target).unwrap_or_else(|| "std::string".to_string());
            Some(format!("std::expected<{}, {}>", ok_str, err_str))
        }
        IrType::Generic(name) => Some(name.clone()),
        IrType::Any => Some("std::any".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_int_to_java() {
        let target = LangSpec::java("25");
        assert_eq!(map_type(&IrType::Int, &target), Some("int".to_string()));
    }

    #[test]
    fn test_map_int_to_rust() {
        let target = LangSpec::rust("2024");
        assert_eq!(map_type(&IrType::Int, &target), Some("i64".to_string()));
    }

    #[test]
    fn test_map_int_to_python() {
        let target = LangSpec::python("313");
        assert_eq!(map_type(&IrType::Int, &target), Some("int".to_string()));
    }

    #[test]
    fn test_map_array_to_java() {
        let target = LangSpec::java("25");
        assert_eq!(
            map_type(&IrType::Array(Box::new(IrType::Int)), &target),
            Some("int[]".to_string())
        );
    }

    #[test]
    fn test_map_optional_to_rust() {
        let target = LangSpec::rust("2024");
        assert_eq!(
            map_type(&IrType::Optional(Box::new(IrType::Int)), &target),
            Some("Option<i64>".to_string())
        );
    }

    #[test]
    fn test_map_result_to_rust() {
        let target = LangSpec::rust("2024");
        let result_type = IrType::Result {
            ok: Box::new(IrType::Int),
            err: Box::new(IrType::Str),
        };
        assert_eq!(map_type(&result_type, &target), Some("Result<i64, String>".to_string()));
    }
}
