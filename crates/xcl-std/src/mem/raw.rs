//! Raw pointer operations for xcl-std.
//!
//! Provides unsafe raw pointer manipulation functions.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Read a byte from a raw pointer.
///
/// # Arguments
/// - `ptr` (int): Memory address
///
/// # Returns
/// Integer byte value (0-255)
///
/// # Example
/// ```xcl
/// byte = mem.read_byte(ptr)
/// ```
#[xcl_fn("mem.read_byte", signature = "(ptr: int) -> int")]
pub fn read_byte(ptr: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let val = unsafe { *(ptr as *const u8) };
    Value::from_i48(val as i64)
}

/// Write a byte to a raw pointer.
///
/// # Arguments
/// - `ptr` (int): Memory address
/// - `value` (int): Byte value to write (0-255)
///
/// # Example
/// ```xcl
/// mem.write_byte(ptr, 42)
/// ```
#[xcl_fn("mem.write_byte", signature = "(ptr: int, value: int) -> nil")]
pub fn write_byte(ptr: i64, value: i64) -> Value {
    if ptr != 0 {
        unsafe { *(ptr as *mut u8) = value as u8 };
    }
    Value::nil()
}

/// Read an i64 from a raw pointer.
///
/// # Arguments
/// - `ptr` (int): Memory address
///
/// # Returns
/// Integer value
///
/// # Example
/// ```xcl
/// mem.read_i64(ptr)
/// ```
#[xcl_fn("mem.read_i64", signature = "(ptr: int) -> int")]
pub fn read_i64(ptr: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let val = unsafe { *(ptr as *const i64) };
    Value::from_i48(val)
}

/// Write an i64 to a raw pointer.
///
/// # Arguments
/// - `ptr` (int): Memory address
/// - `value` (int): Value to write
///
/// # Example
/// ```xcl
/// mem.write_i64(ptr, 12345)
/// ```
#[xcl_fn("mem.write_i64", signature = "(ptr: int, value: int) -> nil")]
pub fn write_i64(ptr: i64, value: i64) -> Value {
    if ptr != 0 {
        unsafe { *(ptr as *mut i64) = value };
    }
    Value::nil()
}

/// Read a f64 from a raw pointer.
///
/// # Arguments
/// - `ptr` (int): Memory address
///
/// # Returns
/// Float value
///
/// # Example
/// ```xcl
/// mem.read_f64(ptr)
/// ```
#[xcl_fn("mem.read_f64", signature = "(ptr: int) -> float")]
pub fn read_f64(ptr: i64) -> Value {
    if ptr == 0 {
        return Value::from_f64(0.0);
    }
    let val = unsafe { *(ptr as *const f64) };
    Value::from_f64(val)
}

/// Write a f64 to a raw pointer.
///
/// # Arguments
/// - `ptr` (int): Memory address
/// - `value` (float): Value to write
///
/// # Example
/// ```xcl
/// mem.write_f64(ptr, 3.14)
/// ```
#[xcl_fn("mem.write_f64", signature = "(ptr: int, value: float) -> nil")]
pub fn write_f64(ptr: i64, value: f64) -> Value {
    if ptr != 0 {
        unsafe { *(ptr as *mut f64) = value };
    }
    Value::nil()
}

/// Copy memory from one pointer to another.
///
/// # Arguments
/// - `dest` (int): Destination pointer
/// - `src` (int): Source pointer
/// - `size` (int): Number of bytes to copy
///
/// # Example
/// ```xcl
/// mem.copy(dest, src, 256)
/// ```
#[xcl_fn("mem.copy", signature = "(dest: int, src: int, size: int) -> nil")]
pub fn copy(dest: i64, src: i64, size: i64) -> Value {
    if dest != 0 && src != 0 && size > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(src as *const u8, dest as *mut u8, size as usize);
        }
    }
    Value::nil()
}

/// Set memory to a byte value.
///
/// # Arguments
/// - `ptr` (int): Memory address
/// - `value` (int): Byte value (0-255)
/// - `size` (int): Number of bytes to set
///
/// # Example
/// ```xcl
/// mem.set(ptr, 0, 256)  // Zero out 256 bytes
/// ```
#[xcl_fn("mem.set", signature = "(ptr: int, value: int, size: int) -> nil")]
pub fn set(ptr: i64, value: i64, size: i64) -> Value {
    if ptr != 0 && size > 0 {
        unsafe {
            std::ptr::write_bytes(ptr as *mut u8, value as u8, size as usize);
        }
    }
    Value::nil()
}

/// Compare two memory regions.
///
/// # Arguments
/// - `ptr1` (int): First memory address
/// - `ptr2` (int): Second memory address
/// - `size` (int): Number of bytes to compare
///
/// # Returns
/// Integer: negative if ptr1 < ptr2, 0 if equal, positive if ptr1 > ptr2
///
/// # Example
/// ```xcl
/// mem.compare(ptr1, ptr2, 256)
/// ```
#[xcl_fn("mem.compare", signature = "(ptr1: int, ptr2: int, size: int) -> int")]
pub fn compare(ptr1: i64, ptr2: i64, size: i64) -> Value {
    if ptr1 == 0 || ptr2 == 0 || size == 0 {
        return Value::from_i48(0);
    }
    let result = unsafe {
        std::cmp:: Ord::cmp(
            std::slice::from_raw_parts(ptr1 as *const u8, size as usize),
            std::slice::from_raw_parts(ptr2 as *const u8, size as usize),
        )
    };
    Value::from_i48(match result {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}

/// Get the size of a type by name.
///
/// # Arguments
/// - `type_name` (str): Type name (e.g., "u8", "i32", "f64", "ptr")
///
/// # Returns
/// Integer size in bytes
///
/// # Example
/// ```xcl
/// mem.size_of("i32")  // Returns 4
/// ```
#[xcl_fn("mem.size_of", signature = "(type_name: str) -> int")]
pub fn size_of(type_name: &str) -> Value {
    let size = match type_name {
        "u8" | "i8" => std::mem::size_of::<u8>(),
        "u16" | "i16" => std::mem::size_of::<u16>(),
        "u32" | "i32" | "f32" => std::mem::size_of::<u32>(),
        "u64" | "i64" | "f64" | "ptr" => std::mem::size_of::<u64>(),
        "u128" | "i128" => std::mem::size_of::<u128>(),
        "bool" => std::mem::size_of::<bool>(),
        _ => 0,
    };
    Value::from_i48(size as i64)
}

/// Get the alignment of a type by name.
///
/// # Arguments
/// - `type_name` (str): Type name
///
/// # Returns
/// Integer alignment in bytes
///
/// # Example
/// ```xcl
/// mem.align_of("f64")
/// ```
#[xcl_fn("mem.align_of", signature = "(type_name: str) -> int")]
pub fn align_of(type_name: &str) -> Value {
    let align = match type_name {
        "u8" | "i8" | "bool" => std::mem::align_of::<u8>(),
        "u16" | "i16" => std::mem::align_of::<u16>(),
        "u32" | "i32" | "f32" => std::mem::align_of::<u32>(),
        "u64" | "i64" | "f64" | "ptr" => std::mem::align_of::<u64>(),
        "u128" | "i128" => std::mem::align_of::<u128>(),
        _ => 0,
    };
    Value::from_i48(align as i64)
}
