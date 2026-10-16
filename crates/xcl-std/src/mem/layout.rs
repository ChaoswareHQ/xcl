//! Memory layout information for xcl-std.
//!
//! Provides type layout and repr information.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the layout of a type.
///
/// # Arguments
/// - `type_name` (str): Type name
///
/// # Returns
/// Map containing size, alignment, and offset information
///
/// # Example
/// ```xcl
/// mem.layout("i64")
/// ```
#[xcl_fn("mem.layout", signature = "(type_name: str) -> map")]
pub fn layout(type_name: &str) -> Value {
    let mut info = HashMap::new();
    let (size, align) = match type_name {
        "u8" | "i8" => (std::mem::size_of::<u8>(), std::mem::align_of::<u8>()),
        "u16" | "i16" => (std::mem::size_of::<u16>(), std::mem::align_of::<u16>()),
        "u32" | "i32" | "f32" => (std::mem::size_of::<u32>(), std::mem::align_of::<u32>()),
        "u64" | "i64" | "f64" => (std::mem::size_of::<u64>(), std::mem::align_of::<u64>()),
        "u128" | "i128" => (std::mem::size_of::<u128>(), std::mem::align_of::<u128>()),
        "bool" => (std::mem::size_of::<bool>(), std::mem::align_of::<bool>()),
        "ptr" => (std::mem::size_of::<usize>(), std::mem::align_of::<usize>()),
        _ => (0, 0),
    };
    info.insert("name".to_string(), Value::from_str_boxed(type_name.to_string()));
    info.insert("size".to_string(), Value::from_i48(size as i64));
    info.insert("alignment".to_string(), Value::from_i48(align as i64));
    Value::from_map(info)
}

/// Calculate the offset of a field in a struct (conceptual).
///
/// # Arguments
/// - `struct_name` (str): Struct name
/// - `field_name` (str): Field name
///
/// # Returns
/// Integer byte offset
///
/// # Example
/// ```xcl
/// mem.field_offset("MyStruct", "field1")
/// ```
#[xcl_fn("mem.field_offset", signature = "(struct_name: str, field_name: str) -> int")]
pub fn field_offset(_struct_name: &str, _field_name: &str) -> Value {
    Value::from_i48(0)
}

/// Calculate padding needed for alignment.
///
/// # Arguments
/// - `current_offset` (int): Current byte offset
/// - `alignment` (int): Required alignment
///
/// # Returns
/// Integer padding bytes needed
///
/// # Example
/// ```xcl
/// mem.padding(5, 8)  // Returns 3
/// ```
#[xcl_fn("mem.padding", signature = "(current_offset: int, alignment: int) -> int")]
pub fn padding(current_offset: i64, alignment: i64) -> Value {
    if alignment == 0 {
        return Value::from_i48(0);
    }
    let remainder = current_offset % alignment;
    let pad = if remainder == 0 { 0 } else { alignment - remainder };
    Value::from_i48(pad)
}

/// Calculate the total size of a struct with given fields.
///
/// # Arguments
/// - `fields` (array): Array of [size, alignment] pairs
///
/// # Returns
/// Integer total size including padding
///
/// # Example
/// ```xcl
/// mem.struct_size([[1, 1], [4, 4], [1, 1]])
/// ```
#[xcl_fn("mem.struct_size", signature = "(fields: array) -> int")]
pub fn struct_size(fields: &[Value]) -> Value {
    let mut offset = 0i64;
    let mut max_align = 1i64;

    for field in fields {
        if let Some(arr) = field.as_array() {
            if arr.len() >= 2 {
                let size = arr[0].as_i64().unwrap_or(0);
                let align = arr[1].as_i64().unwrap_or(1);
                if align > max_align {
                    max_align = align;
                }
                let pad = if align > 0 && offset % align != 0 {
                    align - (offset % align)
                } else {
                    0
                };
                offset += pad + size;
            }
        }
    }

    // Trailing padding
    if max_align > 0 && offset % max_align != 0 {
        offset += max_align - (offset % max_align);
    }

    Value::from_i48(offset)
}

/// Get repr(C) layout information.
///
/// # Returns
/// Map containing C ABI layout rules
///
/// # Example
/// ```xcl
/// mem.repr_c_info()
/// ```
#[xcl_fn("mem.repr_c_info", signature = "() -> map")]
pub fn repr_c_info() -> Value {
    let mut info = HashMap::new();
    info.insert("abi".to_string(), Value::from_str_boxed("C".to_string()));
    info.insert("pointer_size".to_string(), Value::from_i48(std::mem::size_of::<usize>() as i64));
    info.insert("max_align".to_string(), Value::from_i48(std::mem::align_of::<u128>() as i64));
    Value::from_map(info)
}

/// Get repr(transparent) layout information.
///
/// # Arguments
/// - `type_name` (str): The single non-zero-sized field type
///
/// # Returns
/// Map containing transparent layout info
///
/// # Example
/// ```xcl
/// mem.repr_transparent("i64")
/// ```
#[xcl_fn("mem.repr_transparent", signature = "(type_name: str) -> map")]
pub fn repr_transparent(type_name: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("type".to_string(), Value::from_str_boxed(type_name.to_string()));
    info.insert("layout".to_string(), layout(type_name));
    Value::from_map(info)
}
