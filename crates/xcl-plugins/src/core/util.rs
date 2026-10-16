//! Core utility functions for xcl-plugins.
//!
//! Provides basic utility functions used across the standard library.

use xcl_vm::Value;

/// Get the current process ID.
pub fn get_pid() -> Value {
    Value::from_i48(std::process::id() as i64)
}
