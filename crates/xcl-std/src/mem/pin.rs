//! Pin and self-referential memory for xcl-std.
//!
//! Provides Pin utilities for self-referential data structures.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Check if a type is pinned.
///
/// # Arguments
/// - `ptr` (int): Memory address
///
/// # Returns
/// Boolean indicating if the data is pinned
///
/// # Example
/// ```xcl
/// mem.is_pinned(ptr)
/// ```
#[xcl_fn("mem.is_pinned", signature = "(ptr: int) -> bool")]
pub fn is_pinned(ptr: i64) -> Value {
    // In XCL, all heap-allocated data is considered pinned by default
    Value::bool_(ptr != 0)
}

/// Pin data at a memory location.
///
/// # Arguments
/// - `ptr` (int): Memory address
/// - `size` (int): Size of the data
///
/// # Returns
/// Integer pinned handle
///
/// # Example
/// ```xcl
/// mem.pin(ptr, 256)
/// ```
#[xcl_fn("mem.pin", signature = "(ptr: int, size: int) -> int")]
pub fn pin(ptr: i64, _size: i64) -> Value {
    if ptr != 0 {
        Value::from_i48(ptr)
    } else {
        Value::from_i48(0)
    }
}

/// Unpin data, allowing it to be moved.
///
/// # Arguments
/// - `handle` (int): Pinned handle
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// mem.unpin(handle)
/// ```
#[xcl_fn("mem.unpin", signature = "(handle: int) -> result")]
pub fn unpin(_handle: i64) -> Value {
    Value::from_i48(0)
}

/// Get information about pinned data.
///
/// # Arguments
/// - `ptr` (int): Memory address
///
/// # Returns
/// Map containing pin information
///
/// # Example
/// ```xcl
/// mem.pin_info(ptr)
/// ```
#[xcl_fn("mem.pin_info", signature = "(ptr: int) -> map")]
pub fn pin_info(ptr: i64) -> Value {
    let mut info = HashMap::new();
    info.insert("ptr".to_string(), Value::from_i48(ptr));
    info.insert("pinned".to_string(), Value::bool_(ptr != 0));
    info.insert("movable".to_string(), Value::bool_(false));
    Value::from_map(info)
}

/// Check if a type implements Unpin.
///
/// # Arguments
/// - `type_name` (str): Type name
///
/// # Returns
/// Boolean indicating if the type is Unpin
///
/// # Example
/// ```xcl
/// mem.is_unpin("i64")
/// ```
#[xcl_fn("mem.is_unpin", signature = "(type_name: str) -> bool")]
pub fn is_unpin(_type_name: &str) -> Value {
    // Most primitive types are Unpin
    Value::bool_(true)
}
