//! C FFI type definitions for xcl-plugins.
//!
//! Provides C-compatible types for FFI interoperability.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the size of a C type in bytes.
///
/// # Arguments
/// - `type_name` (str): C type name (e.g., "c_char", "c_int", "c_long", "c_void_ptr")
///
/// # Returns
/// Integer size in bytes
///
/// # Example
/// ```xcl
/// ffi.c_size("c_int")  // Returns 4 on most platforms
/// ```
#[xcl_fn("ffi.c_size", signature = "(type_name: str) -> int")]
pub fn c_size(type_name: &str) -> Value {
    let size = match type_name {
        "c_char" => std::mem::size_of::<std::ffi::c_char>(),
        "c_schar" => std::mem::size_of::<std::ffi::c_schar>(),
        "c_uchar" => std::mem::size_of::<std::ffi::c_uchar>(),
        "c_short" => std::mem::size_of::<std::ffi::c_short>(),
        "c_ushort" => std::mem::size_of::<std::ffi::c_ushort>(),
        "c_int" => std::mem::size_of::<std::ffi::c_int>(),
        "c_uint" => std::mem::size_of::<std::ffi::c_uint>(),
        "c_long" => std::mem::size_of::<std::ffi::c_long>(),
        "c_ulong" => std::mem::size_of::<std::ffi::c_ulong>(),
        "c_longlong" => std::mem::size_of::<std::ffi::c_longlong>(),
        "c_ulonglong" => std::mem::size_of::<std::ffi::c_ulonglong>(),
        "c_float" => std::mem::size_of::<std::ffi::c_float>(),
        "c_double" => std::mem::size_of::<std::ffi::c_double>(),
        "c_void_ptr" => std::mem::size_of::<*mut std::ffi::c_void>(),
        "c_ptrdiff" => std::mem::size_of::<std::ffi::c_ptrdiff>(),
        "c_size_t" => std::mem::size_of::<std::ffi::c_size_t>(),
        _ => 0,
    };
    Value::from_i48(size as i64)
}

/// Get the alignment of a C type in bytes.
///
/// # Arguments
/// - `type_name` (str): C type name
///
/// # Returns
/// Integer alignment in bytes
///
/// # Example
/// ```xcl
/// ffi.c_align("c_double")
/// ```
#[xcl_fn("ffi.c_align", signature = "(type_name: str) -> int")]
pub fn c_align(type_name: &str) -> Value {
    let align = match type_name {
        "c_char" => std::mem::align_of::<std::ffi::c_char>(),
        "c_int" => std::mem::align_of::<std::ffi::c_int>(),
        "c_long" => std::mem::align_of::<std::ffi::c_long>(),
        "c_float" => std::mem::align_of::<std::ffi::c_float>(),
        "c_double" => std::mem::align_of::<std::ffi::c_double>(),
        "c_void_ptr" => std::mem::align_of::<*mut std::ffi::c_void>(),
        _ => 0,
    };
    Value::from_i48(align as i64)
}

/// Get the pointer size in bytes (4 on 32-bit, 8 on 64-bit).
///
/// # Returns
/// Integer pointer size
///
/// # Example
/// ```xcl
/// ffi.pointer_size()
/// ```
#[xcl_fn("ffi.pointer_size", signature = "() -> int")]
pub fn pointer_size() -> Value {
    Value::from_i48(std::mem::size_of::<*mut std::ffi::c_void>() as i64)
}

/// Get the platform byte order.
///
/// # Returns
/// String: "little_endian" or "big_endian"
///
/// # Example
/// ```xcl
/// ffi.byte_order()
/// ```
#[xcl_fn("ffi.byte_order", signature = "() -> str")]
pub fn byte_order() -> Value {
    let order = if cfg!(target_endian = "little") {
        "little_endian"
    } else {
        "big_endian"
    };
    Value::from_str_boxed(order.to_string())
}

/// Get C type information.
///
/// # Arguments
/// - `type_name` (str): C type name
///
/// # Returns
/// Map containing type information (size, alignment, signed)
///
/// # Example
/// ```xcl
/// ffi.c_type_info("c_int")
/// ```
#[xcl_fn("ffi.c_type_info", signature = "(type_name: str) -> map")]
pub fn c_type_info(type_name: &str) -> Value {
    let mut info = std::collections::HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(type_name.to_string()));
    info.insert("size".to_string(), c_size(type_name));
    info.insert("alignment".to_string(), c_align(type_name));

    let is_signed = matches!(type_name, "c_char" | "c_schar" | "c_short" | "c_int"
        | "c_long" | "c_longlong" | "c_float" | "c_double" | "c_ptrdiff");
    info.insert("signed".to_string(), Value::bool_(is_signed));

    Value::from_map(info)
}

/// List all available C FFI types.
///
/// # Returns
/// Array of type name strings
///
/// # Example
/// ```xcl
/// ffi.c_types()
/// ```
#[xcl_fn("ffi.c_types", signature = "() -> array")]
pub fn c_types() -> Value {
    let types = vec![
        "c_char", "c_schar", "c_uchar",
        "c_short", "c_ushort",
        "c_int", "c_uint",
        "c_long", "c_ulong",
        "c_longlong", "c_ulonglong",
        "c_float", "c_double",
        "c_void_ptr", "c_ptrdiff", "c_size_t",
    ];
    let values: Vec<Value> = types.iter().map(|t| Value::from_str_boxed(t.to_string())).collect();
    Value::from_array(values)
}
