//! Calling conventions for xcl-plugins.
//!
//! Provides information about platform calling conventions.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the default calling convention for the platform.
///
/// # Returns
/// String calling convention name
///
/// # Example
/// ```xcl
/// abi.default_calling()
/// ```
#[xcl_fn("abi.default_calling", signature = "() -> str")]
pub fn default_calling() -> Value {
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

/// List available calling conventions.
///
/// # Returns
/// Array of calling convention names
///
/// # Example
/// ```xcl
/// abi.calling_conventions()
/// ```
#[xcl_fn("abi.calling_conventions", signature = "() -> array")]
pub fn calling_conventions() -> Value {
    let mut convs = vec![
        "C", "Rust", "system",
        "cdecl", "stdcall", "fastcall",
    ];
    if cfg!(target_arch = "x86_64") {
        convs.push("sysv64");
    }
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        convs.push("win64");
    }
    if cfg!(target_arch = "aarch64") {
        convs.push("aapcs64");
    }
    let values: Vec<Value> = convs.iter().map(|c| Value::from_str_boxed(c.to_string())).collect();
    Value::from_array(values)
}

/// Get calling convention information.
///
/// # Arguments
/// - `convention` (str): Calling convention name
///
/// # Returns
/// Map containing convention details
///
/// # Example
/// ```xcl
/// abi.calling_info("sysv64")
/// ```
#[xcl_fn("abi.calling_info", signature = "(convention: str) -> map")]
pub fn calling_info(convention: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(convention.to_string()));

    match convention {
        "sysv64" | "C" => {
            info.insert("int_regs".to_string(), Value::from_i48(6));
            info.insert("float_regs".to_string(), Value::from_i48(8));
            info.insert("stack_args".to_string(), Value::from_str_boxed("right_to_left".to_string()));
            info.insert("cleanup".to_string(), Value::from_str_boxed("caller".to_string()));
        }
        "win64" => {
            info.insert("int_regs".to_string(), Value::from_i48(4));
            info.insert("float_regs".to_string(), Value::from_i48(8));
            info.insert("stack_args".to_string(), Value::from_str_boxed("right_to_left".to_string()));
            info.insert("cleanup".to_string(), Value::from_str_boxed("caller".to_string()));
        }
        "stdcall" => {
            info.insert("int_regs".to_string(), Value::from_i48(0));
            info.insert("float_regs".to_string(), Value::from_i48(0));
            info.insert("stack_args".to_string(), Value::from_str_boxed("right_to_left".to_string()));
            info.insert("cleanup".to_string(), Value::from_str_boxed("callee".to_string()));
        }
        "fastcall" => {
            info.insert("int_regs".to_string(), Value::from_i48(2));
            info.insert("float_regs".to_string(), Value::from_i48(0));
            info.insert("stack_args".to_string(), Value::from_str_boxed("right_to_left".to_string()));
            info.insert("cleanup".to_string(), Value::from_str_boxed("callee".to_string()));
        }
        _ => {
            info.insert("int_regs".to_string(), Value::from_i48(0));
            info.insert("float_regs".to_string(), Value::from_i48(0));
        }
    }

    Value::from_map(info)
}

/// Get register names for the platform.
///
/// # Returns
/// Map of register categories to names
///
/// # Example
/// ```xcl
/// abi.registers()
/// ```
#[xcl_fn("abi.registers", signature = "() -> map")]
pub fn registers() -> Value {
    let mut info = HashMap::new();

    if cfg!(target_arch = "x86_64") {
        let general: Vec<Value> = vec![
            "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp",
            "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
        ].iter().map(|r| Value::from_str_boxed(r.to_string())).collect();
        info.insert("general".to_string(), Value::from_array(general));

        let float: Vec<Value> = vec![
            "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
        ].iter().map(|r| Value::from_str_boxed(r.to_string())).collect();
        info.insert("float".to_string(), Value::from_array(float));
    }

    if cfg!(target_arch = "aarch64") {
        let general: Vec<Value> = (0..31)
            .map(|i| Value::from_str_boxed(format!("x{}", i)))
            .collect();
        info.insert("general".to_string(), Value::from_array(general));

        let float: Vec<Value> = (0..32)
            .map(|i| Value::from_str_boxed(format!("d{}", i)))
            .collect();
        info.insert("float".to_string(), Value::from_array(float));
    }

    Value::from_map(info)
}

/// Check if a calling convention is available.
///
/// # Arguments
/// - `convention` (str): Calling convention name
///
/// # Returns
/// Boolean indicating availability
///
/// # Example
/// ```xcl
/// abi.has_calling("sysv64")
/// ```
#[xcl_fn("abi.has_calling", signature = "(convention: str) -> bool")]
pub fn has_calling(convention: &str) -> Value {
    let available = match convention {
        "C" | "Rust" | "system" | "cdecl" => true,
        "stdcall" | "fastcall" => cfg!(target_arch = "x86"),
        "sysv64" => cfg!(target_arch = "x86_64"),
        "win64" => cfg!(target_os = "windows") && cfg!(target_arch = "x86_64"),
        "aapcs64" => cfg!(target_arch = "aarch64"),
        _ => false,
    };
    Value::bool_(available)
}
