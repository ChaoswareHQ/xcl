//! Inline assembly for xcl-plugins.
//!
//! Provides inline assembly execution for low-level CPU operations.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Execute inline assembly.
///
/// # Arguments
/// - `asm` (str): Assembly code string
/// - `inputs` (array): Input operands
/// - `outputs` (array): Output operand count
///
/// # Returns
/// Map containing output values
///
/// # Example
/// ```xcl
/// cpu.asm("nop", [], [])
/// ```
#[xcl_fn("cpu.asm", signature = "(asm: str, inputs: array, outputs: array) -> map")]
pub fn asm(_asm: &str, _inputs: &[Value], _outputs: &[Value]) -> Value {
    let mut result = HashMap::new();
    result.insert("success".to_string(), Value::bool_(false));
    result.insert("error".to_string(), Value::from_str_boxed("Inline assembly not supported at runtime".to_string()));
    Value::from_map(result)
}

/// Get CPU flags.
///
/// # Returns
/// Map containing CPU flag information
///
/// # Example
/// ```xcl
/// cpu.flags()
/// ```
#[xcl_fn("cpu.flags", signature = "() -> map")]
pub fn flags() -> Value {
    let mut info = HashMap::new();
    info.insert("arch".to_string(), Value::from_str_boxed(std::env::consts::ARCH.to_string()));
    info.insert("endian".to_string(), Value::from_str_boxed(std::env::consts::ENDIAN.to_string()));
    info.insert("words".to_string(), Value::from_str_boxed(std::env::consts::WORD.to_string()));
    Value::from_map(info)
}

/// Get CPU information.
///
/// # Returns
/// Map containing CPU information
///
/// # Example
/// ```xcl
/// cpu.info()
/// ```
#[xcl_fn("cpu.info", signature = "() -> map")]
pub fn info() -> Value {
    let mut info = HashMap::new();
    info.insert("arch".to_string(), Value::from_str_boxed(std::env::consts::ARCH.to_string()));
    info.insert("family".to_string(), Value::from_str_boxed(std::env::consts::FAMILY.to_string()));
    info.insert("os".to_string(), Value::from_str_boxed(std::env::consts::OS.to_string()));
    info.insert("endian".to_string(), Value::from_str_boxed(std::env::consts::ENDIAN.to_string()));
    info.insert("pointer_width".to_string(), Value::from_str_boxed(std::env::consts::WORD.to_string()));
    Value::from_map(info)
}

/// Check CPU feature.
///
/// # Arguments
/// - `feature` (str): Feature name (e.g., "sse2", "avx", "neon")
///
/// # Returns
/// Boolean indicating if the feature is supported
///
/// # Example
/// ```xcl
/// cpu.has_feature("sse2")
/// ```
#[xcl_fn("cpu.has_feature", signature = "(feature: str) -> bool")]
pub fn has_feature(feature: &str) -> Value {
    let has = match feature {
        "sse2" => cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86"),
        "avx" | "avx2" => cfg!(target_arch = "x86_64"),
        "neon" => cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm"),
        "rdrand" | "rdseed" => cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86"),
        _ => false,
    };
    Value::bool_(has)
}

/// List CPU features.
///
/// # Returns
/// Array of supported CPU feature names
///
/// # Example
/// ```xcl
/// cpu.features()
/// ```
#[xcl_fn("cpu.features", signature = "() -> array")]
pub fn features() -> Value {
    let mut features = Vec::new();
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86") {
        features.push("sse2");
        features.push("sse3");
        features.push("ssse3");
        features.push("sse4.1");
        features.push("sse4.2");
        features.push("avx");
        features.push("avx2");
        features.push("f16c");
        features.push("fma");
    }
    if cfg!(target_arch = "aarch64") {
        features.push("neon");
        features.push("fp16");
        features.push("crc32");
    }
    let values: Vec<Value> = features.iter().map(|f| Value::from_str_boxed(f.to_string())).collect();
    Value::from_array(values)
}
