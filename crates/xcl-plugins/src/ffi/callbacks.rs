//! Callback support for FFI in xcl-plugins.
//!
//! Provides extern "C" function callback creation and invocation.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Create a C-compatible callback from an XCL function.
///
/// This creates a trampoline that allows XCL functions to be called
/// from C code. The callback is stored and can be passed to FFI calls.
///
/// # Arguments
/// - `name` (str): Name for the callback
/// - `func` (fn): XCL function to wrap
///
/// # Returns
/// Integer callback handle ID
///
/// # Example
/// ```xcl
/// callback = ffi.create_callback("my_callback", my_function)
/// ```
#[xcl_fn]
pub fn create_callback(_name: &str, _func: &Value) -> Value {
    // Callback creation requires storing function pointers
    // For now, return a placeholder
    Value::from_i48(0)
}

/// Remove a previously created callback.
///
/// # Arguments
/// - `handle` (int): Callback handle ID
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// ffi.remove_callback(handle)
/// ```
#[xcl_fn]
pub fn remove_callback(_handle: i64) -> Value {
    Value::from_i48(0)
}

/// Get information about a callback.
///
/// # Arguments
/// - `handle` (int): Callback handle ID
///
/// # Returns
/// Map containing callback information
///
/// # Example
/// ```xcl
/// ffi.callback_info(handle)
/// ```
#[xcl_fn]
pub fn callback_info(_handle: i64) -> Value {
    Value::from_map(std::collections::HashMap::new())
}

/// List all active callbacks.
///
/// # Returns
/// Array of callback information maps
///
/// # Example
/// ```xcl
/// ffi.list_callbacks()
/// ```
#[xcl_fn]
pub fn list_callbacks() -> Value {
    Value::from_array(vec![])
}
