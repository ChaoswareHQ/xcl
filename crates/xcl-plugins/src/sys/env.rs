//! Environment variable and process arguments interface for xcl-plugins.
//!
//! Provides access to environment variables and command-line arguments.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get an environment variable by name.
///
/// # Arguments
/// - `name` (str): Environment variable name
///
/// # Returns
/// String value, or nil if not set
///
/// # Example
/// ```xcl
/// sys.env_get("HOME")
/// ```
#[xcl_fn("sys.env_get", signature = "(name: str) -> str")]
pub fn env_get(name: &str) -> Value {
    match std::env::var(name) {
        Ok(val) => Value::from_str_boxed(val),
        Err(_) => Value::nil(),
    }
}

/// Set an environment variable.
///
/// # Arguments
/// - `name` (str): Environment variable name
/// - `value` (str): Value to set
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.env_set("MY_VAR", "my_value")
/// ```
#[xcl_fn("sys.env_set", signature = "(name: str, value: str) -> result")]
pub fn env_set(name: &str, value: &str) -> Value {
    // In Rust 2024, set_var requires unsafe
    unsafe {
        std::env::set_var(name, value);
    }
    Value::from_i48(0)
}

/// Remove an environment variable.
///
/// # Arguments
/// - `name` (str): Environment variable name
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.env_remove("MY_VAR")
/// ```
#[xcl_fn("sys.env_remove", signature = "(name: str) -> result")]
pub fn env_remove(name: &str) -> Value {
    unsafe {
        std::env::remove_var(name);
    }
    Value::from_i48(0)
}

/// List all environment variables.
///
/// # Returns
/// Map of all environment variable names to values
///
/// # Example
/// ```xcl
/// envs = sys.env_list()
/// ```
#[xcl_fn("sys.env_list", signature = "() -> map")]
pub fn env_list() -> Value {
    let mut envs = HashMap::new();
    for (key, val) in std::env::vars() {
        envs.insert(key, Value::from_str_boxed(val));
    }
    Value::from_map(envs)
}

/// Get command-line arguments.
///
/// # Returns
/// Array of string arguments
///
/// # Example
/// ```xcl
/// args = sys.args()
/// ```
#[xcl_fn("sys.args", signature = "() -> array")]
pub fn args() -> Value {
    let args: Vec<Value> = std::env::args()
        .map(|arg| Value::from_str_boxed(arg))
        .collect();
    Value::from_array(args)
}

/// Get the program name (argv[0]).
///
/// # Returns
/// String program name
///
/// # Example
/// ```xcl
/// sys.program_name()
/// ```
#[xcl_fn("sys.program_name", signature = "() -> str")]
pub fn program_name() -> Value {
    let name = std::env::args().next().unwrap_or_default();
    Value::from_str_boxed(name)
}

/// Check if an environment variable is set.
///
/// # Arguments
/// - `name` (str): Environment variable name
///
/// # Returns
/// Boolean indicating if the variable is set
///
/// # Example
/// ```xcl
/// sys.env_exists("HOME")
/// ```
#[xcl_fn("sys.env_exists", signature = "(name: str) -> bool")]
pub fn env_exists(name: &str) -> Value {
    Value::bool_(std::env::var(name).is_ok())
}

/// Get the number of command-line arguments.
///
/// # Returns
/// Integer count of arguments
///
/// # Example
/// ```xcl
/// sys.args_count()
/// ```
#[xcl_fn("sys.args_count", signature = "() -> int")]
pub fn args_count() -> Value {
    Value::from_i48(std::env::args().count() as i64)
}
