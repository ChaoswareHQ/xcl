//! Symbol and linkage information for xcl-std.
//!
//! Provides information about symbol mangling and linkage.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the platform's symbol mangling scheme.
///
/// # Returns
/// String mangling scheme name
///
/// # Example
/// ```xcl
/// abi.mangling()
/// ```
#[xcl_fn("abi.mangling", signature = "() -> str")]
pub fn mangling() -> Value {
    let scheme = if cfg!(target_os = "windows") {
        "Microsoft x64"
    } else if cfg!(target_os = "macos") {
        "Apple x86_64"
    } else {
        "GNU"
    };
    Value::from_str_boxed(scheme.to_string())
}

/// Get symbol visibility options.
///
/// # Returns
/// Array of visibility option names
///
/// # Example
/// ```xcl
/// abi.visibility_options()
/// ```
#[xcl_fn("abi.visibility_options", signature = "() -> array")]
pub fn visibility_options() -> Value {
    let options = vec!["default", "hidden", "protected", "internal"];
    let values: Vec<Value> = options.iter().map(|o| Value::from_str_boxed(o.to_string())).collect();
    Value::from_array(values)
}

/// Get linkage options.
///
/// # Returns
/// Array of linkage option names
///
/// # Example
/// ```xcl
/// abi.linkage_options()
/// ```
#[xcl_fn("abi.linkage_options", signature = "() -> array")]
pub fn linkage_options() -> Value {
    let options = vec!["extern", "extern_weak", "internal", "private", "weak"];
    let values: Vec<Value> = options.iter().map(|o| Value::from_str_boxed(o.to_string())).collect();
    Value::from_array(values)
}

/// Get no_mangle symbol information.
///
/// # Arguments
/// - `name` (str): Symbol name
///
/// # Returns
/// Map containing symbol information
///
/// # Example
/// ```xcl
/// abi.no_mangle("my_function")
/// ```
#[xcl_fn("abi.no_mangle", signature = "(name: str) -> map")]
pub fn no_mangle(name: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(name.to_string()));
    info.insert("mangled".to_string(), Value::from_str_boxed(name.to_string()));
    info.insert("visibility".to_string(), Value::from_str_boxed("default".to_string()));
    Value::from_map(info)
}

/// Get export symbol information.
///
/// # Arguments
/// - `name` (str): Symbol name
///
/// # Returns
/// Map containing export information
///
/// # Example
/// ```xcl
/// abi.export_info("my_function")
/// ```
#[xcl_fn("abi.export_info", signature = "(name: str) -> map")]
pub fn export_info(name: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(name.to_string()));
    info.insert("exported".to_string(), Value::bool_(true));
    info.insert("linkage".to_string(), Value::from_str_boxed("extern".to_string()));
    Value::from_map(info)
}

/// Get platform ABI information.
///
/// # Returns
/// Map containing platform ABI details
///
/// # Example
/// ```xcl
/// abi.platform_info()
/// ```
#[xcl_fn("abi.platform_info", signature = "() -> map")]
pub fn platform_info() -> Value {
    let mut info = HashMap::new();
    info.insert("arch".to_string(), Value::from_str_boxed(std::env::consts::ARCH.to_string()));
    info.insert("os".to_string(), Value::from_str_boxed(std::env::consts::OS.to_string()));
    info.insert("env".to_string(), Value::from_str_boxed(std::env::consts::ENV.to_string()));
    info.insert("family".to_string(), Value::from_str_boxed(std::env::consts::FAMILY.to_string()));
    info.insert("endian".to_string(), Value::from_str_boxed(std::env::consts::ENDIAN.to_string()));
    info.insert("pointer_width".to_string(), Value::from_str_boxed(std::env::consts::WORD.to_string()));
    info.insert("data_model".to_string(), data_model());
    info.insert("mangling".to_string(), mangling());
    info.insert("default_calling".to_string(), default_calling());
    Value::from_map(info)
}

/// Check if a symbol name is valid.
///
/// # Arguments
/// - `name` (str): Symbol name to check
///
/// # Returns
/// Boolean indicating if the name is valid
///
/// # Example
/// ```xcl
/// abi.valid_symbol("my_function")
/// ```
#[xcl_fn("abi.valid_symbol", signature = "(name: str) -> bool")]
pub fn valid_symbol(name: &str) -> Value {
    if name.is_empty() {
        return Value::bool_(false);
    }
    let first = name.chars().next().unwrap();
    let valid = first.is_ascii_alphabetic() || first == '_'
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    Value::bool_(valid)
}

fn data_model() -> Value {
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

fn default_calling() -> Value {
    let conv = if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        "win64"
    } else if cfg!(target_os = "windows") {
        "stdcall"
    } else if cfg!(target_arch = "x86_64") {
        "sysv64"
    } else if cfg!(target_arch = "aarch64") {
        "aapcs64"
    } else {
        "C"
    };
    Value::from_str_boxed(conv.to_string())
}
