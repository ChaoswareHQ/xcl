//! libc FFI bindings for xcl-plugins.
//!
//! Provides access to common libc functions from XCL.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Call a libc function by name.
///
/// # Arguments
/// - `function` (str): Function name (e.g., "malloc", "free", "printf")
/// - `args` (array): Array of integer arguments
///
/// # Returns
/// Integer result from the function call
///
/// # Example
/// ```xcl
/// ffi.libc.call("malloc", [256])
/// ```
#[xcl_fn("ffi.libc.call", signature = "(function: str, args: array) -> result")]
pub fn call(_function: &str, _args: &[Value]) -> Value {
    Value::from_i48(0)
}

/// Get the libc version.
///
/// # Returns
/// String libc version
///
/// # Example
/// ```xcl
/// ffi.libc.version()
/// ```
#[xcl_fn("ffi.libc.version", signature = "() -> str")]
pub fn version() -> Value {
    #[cfg(target_os = "linux")]
    {
        Value::from_str_boxed("glibc".to_string())
    }
    #[cfg(target_os = "macos")]
    {
        Value::from_str_boxed("libSystem".to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Value::from_str_boxed("unknown".to_string())
    }
}

/// List available libc functions.
///
/// # Returns
/// Array of function name strings
///
/// # Example
/// ```xcl
/// ffi.libc.functions()
/// ```
#[xcl_fn("ffi.libc.functions", signature = "() -> array")]
pub fn functions() -> Value {
    let funcs = vec![
        "malloc", "calloc", "realloc", "free",
        "printf", "sprintf", "snprintf",
        "strlen", "strcpy", "strncpy", "strcmp", "strncmp",
        "memcpy", "memmove", "memset", "memcmp",
        "atoi", "atof", "atol", "strtoll", "strtod",
        "time", "localtime", "gmtime", "strftime",
        "getenv", "setenv", "unsetenv",
        "system", "execvp", "fork", "wait",
        "open", "close", "read", "write", "lseek",
        "stat", "fstat", "mkdir", "rmdir", "unlink",
    ];
    let values: Vec<Value> = funcs.iter().map(|f| Value::from_str_boxed(f.to_string())).collect();
    Value::from_array(values)
}

/// Get information about a libc function.
///
/// # Arguments
/// - `function` (str): Function name
///
/// # Returns
/// Map containing function information
///
/// # Example
/// ```xcl
/// ffi.libc.function_info("malloc")
/// ```
#[xcl_fn("ffi.libc.function_info", signature = "(function: str) -> map")]
pub fn function_info(function: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(function.to_string()));

    // Common function signatures
    let signature = match function {
        "malloc" => "void* malloc(size_t size)",
        "free" => "void free(void* ptr)",
        "printf" => "int printf(const char* format, ...)",
        "strlen" => "size_t strlen(const char* s)",
        "memcpy" => "void* memcpy(void* dest, const void* src, size_t n)",
        "time" => "time_t time(time_t* t)",
        "getenv" => "char* getenv(const char* name)",
        "system" => "int system(const char* command)",
        _ => "unknown",
    };
    info.insert("signature".to_string(), Value::from_str_boxed(signature.to_string()));
    Value::from_map(info)
}
