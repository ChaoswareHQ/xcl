//! ABI layout information for xcl-plugins.
//!
//! Provides information about data layout and repr attributes.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the platform's data model (ILP32, LP64, LLP64).
///
/// # Returns
/// String data model name
///
/// # Example
/// ```xcl
/// abi.data_model()
/// ```
#[xcl_fn("abi.data_model", signature = "() -> str")]
pub fn data_model() -> Value {
    let ptr_size = std::mem::size_of::<usize>();
    let long_size = std::mem::size_of::<libc::c_long>();
    let int_size = std::mem::size_of::<i32>();

    let model = if int_size == 4 && long_size == 4 && ptr_size == 4 {
        "ILP32"
    } else if int_size == 4 && long_size == 8 && ptr_size == 8 {
        "LP64"
    } else if int_size == 4 && long_size == 4 && ptr_size == 8 {
        "LLP64"
    } else {
        "Unknown"
    };
    Value::from_str_boxed(model.to_string())
}

/// Get type sizes for the platform.
///
/// # Returns
/// Map of type names to sizes in bytes
///
/// # Example
/// ```xcl
/// abi.type_sizes()
/// ```
#[xcl_fn("abi.type_sizes", signature = "() -> map")]
pub fn type_sizes() -> Value {
    let mut sizes = HashMap::new();
    sizes.insert("char".to_string(), Value::from_i48(std::mem::size_of::<u8>() as i64));
    sizes.insert("short".to_string(), Value::from_i48(std::mem::size_of::<i16>() as i64));
    sizes.insert("int".to_string(), Value::from_i48(std::mem::size_of::<i32>() as i64));
    sizes.insert("long".to_string(), Value::from_i48(std::mem::size_of::<libc::c_long>() as i64));
    sizes.insert("long_long".to_string(), Value::from_i48(std::mem::size_of::<i64>() as i64));
    sizes.insert("float".to_string(), Value::from_i48(std::mem::size_of::<f32>() as i64));
    sizes.insert("double".to_string(), Value::from_i48(std::mem::size_of::<f64>() as i64));
    sizes.insert("pointer".to_string(), Value::from_i48(std::mem::size_of::<usize>() as i64));
    sizes.insert("size_t".to_string(), Value::from_i48(std::mem::size_of::<usize>() as i64));
    sizes.insert("ssize_t".to_string(), Value::from_i48(std::mem::size_of::<isize>() as i64));
    Value::from_map(sizes)
}

/// Get the maximum alignment for the platform.
///
/// # Returns
/// Integer maximum alignment in bytes
///
/// # Example
/// ```xcl
/// abi.max_align()
/// ```
#[xcl_fn("abi.max_align", signature = "() -> int")]
pub fn max_align() -> Value {
    Value::from_i48(std::mem::align_of::<u128>() as i64)
}

/// Get the natural alignment of a type.
///
/// # Arguments
/// - `type_name` (str): Type name
///
/// # Returns
/// Integer alignment in bytes
///
/// # Example
/// ```xcl
/// abi.natural_align("double")
/// ```
#[xcl_fn("abi.natural_align", signature = "(type_name: str) -> int")]
pub fn natural_align(type_name: &str) -> Value {
    let align = match type_name {
        "char" | "u8" | "i8" => std::mem::align_of::<u8>(),
        "short" | "u16" | "i16" => std::mem::align_of::<u16>(),
        "int" | "u32" | "i32" | "float" => std::mem::align_of::<u32>(),
        "long" => std::mem::align_of::<libc::c_long>(),
        "long_long" | "u64" | "i64" | "double" | "pointer" => std::mem::align_of::<u64>(),
        _ => 0,
    };
    Value::from_i48(align as i64)
}

/// Get repr attribute information.
///
/// # Arguments
/// - `repr` (str): Repr attribute name (e.g., "C", "transparent", "packed")
///
/// # Returns
/// Map containing repr information
///
/// # Example
/// ```xcl
/// abi.repr_info("C")
/// ```
#[xcl_fn("abi.repr_info", signature = "(repr: str) -> map")]
pub fn repr_info(repr: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(repr.to_string()));

    match repr {
        "C" => {
            info.insert("description".to_string(), Value::from_str_boxed(
                "Uses C ABI layout and calling convention".to_string()
            ));
            info.insert("alignment".to_string(), Value::from_str_boxed(
                "Natural alignment of fields".to_string()
            ));
        }
        "transparent" => {
            info.insert("description".to_string(), Value::from_str_boxed(
                "Has the same layout as the single non-zero-sized field".to_string()
            ));
            info.insert("alignment".to_string(), Value::from_str_boxed(
                "Same as the inner type".to_string()
            ));
        }
        "packed" => {
            info.insert("description".to_string(), Value::from_str_boxed(
                "Removes padding between fields".to_string()
            ));
            info.insert("alignment".to_string(), Value::from_str_boxed(
                "1 byte (no alignment)".to_string()
            ));
        }
        "align(N)" => {
            info.insert("description".to_string(), Value::from_str_boxed(
                "Sets minimum alignment to N bytes".to_string()
            ));
        }
        _ => {
            info.insert("description".to_string(), Value::from_str_boxed(
                "Unknown repr attribute".to_string()
            ));
        }
    }

    Value::from_map(info)
}

/// List available repr attributes.
///
/// # Returns
/// Array of repr attribute names
///
/// # Example
/// ```xcl
/// abi.repr_attributes()
/// ```
#[xcl_fn("abi.repr_attributes", signature = "() -> array")]
pub fn repr_attributes() -> Value {
    let attrs = vec![
        "C", "rust", "transparent", "packed",
        "align", "simd", "extern",
    ];
    let values: Vec<Value> = attrs.iter().map(|a| Value::from_str_boxed(a.to_string())).collect();
    Value::from_array(values)
}
