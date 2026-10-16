//! SIMD operations for xcl-plugins.
//!
//! Provides portable SIMD intrinsics for vectorized computation.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Check if SIMD is available.
///
/// # Arguments
/// - `feature` (str): SIMD feature name (e.g., "sse2", "avx", "neon")
///
/// # Returns
/// Boolean indicating if the feature is available
///
/// # Example
/// ```xcl
/// cpu.simd_available("sse2")
/// ```
#[xcl_fn("cpu.simd_available", signature = "(feature: str) -> bool")]
pub fn simd_available(feature: &str) -> Value {
    let available = match feature {
        "sse2" => cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86"),
        "avx" => cfg!(target_arch = "x86_64"),
        "avx2" => cfg!(target_arch = "x86_64"),
        "neon" => cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm"),
        "wasmsimd" => cfg!(target_arch = "wasm32"),
        _ => false,
    };
    Value::bool_(available)
}

/// Get SIMD vector width in bytes.
///
/// # Arguments
/// - `feature` (str): SIMD feature name
///
/// # Returns
/// Integer vector width in bytes
///
/// # Example
/// ```xcl
/// cpu.simd_width("sse2")  // Returns 16
/// ```
#[xcl_fn("cpu.simd_width", signature = "(feature: str) -> int")]
pub fn simd_width(feature: &str) -> Value {
    let width = match feature {
        "sse" | "sse2" | "sse3" | "ssse3" => 16,
        "avx" | "avx2" => 32,
        "avx512" => 64,
        "neon" => 16,
        "sve" => 2048,
        _ => 0,
    };
    Value::from_i48(width as i64)
}

/// Get SIMD vector element count.
///
/// # Arguments
/// - `feature` (str): SIMD feature name
/// - `element_size` (int): Size of each element in bytes
///
/// # Returns
/// Integer number of elements that fit in a vector
///
/// # Example
/// ```xcl
/// cpu.simd_elements("sse2", 4)  // Returns 4 (four 32-bit floats)
/// ```
#[xcl_fn("cpu.simd_elements", signature = "(feature: str, element_size: int) -> int")]
pub fn simd_elements(feature: &str, element_size: i64) -> Value {
    if element_size == 0 {
        return Value::from_i48(0);
    }
    let width = simd_width(feature).as_i64().unwrap_or(0);
    Value::from_i48(width / element_size)
}

/// List available SIMD features.
///
/// # Returns
/// Array of available SIMD feature names
///
/// # Example
/// ```xcl
/// cpu.simd_features()
/// ```
#[xcl_fn("cpu.simd_features", signature = "() -> array")]
pub fn simd_features() -> Value {
    let mut features = Vec::new();
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86") {
        features.push("sse2");
        features.push("sse3");
        features.push("ssse3");
        features.push("avx");
        features.push("avx2");
    }
    if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm") {
        features.push("neon");
    }
    if cfg!(target_arch = "wasm32") {
        features.push("wasmsimd");
    }
    let values: Vec<Value> = features.iter().map(|f| Value::from_str_boxed(f.to_string())).collect();
    Value::from_array(values)
}

/// Get SIMD information.
///
/// # Returns
/// Map containing SIMD capabilities
///
/// # Example
/// ```xcl
/// cpu.simd_info()
/// ```
#[xcl_fn("cpu.simd_info", signature = "() -> map")]
pub fn simd_info() -> Value {
    let mut info = HashMap::new();
    info.insert("arch".to_string(), Value::from_str_boxed(std::env::consts::ARCH.to_string()));
    info.insert("features".to_string(), simd_features());
    info.insert("max_width".to_string(), Value::from_i48(64));
    Value::from_map(info)
}

/// Vector add (conceptual).
///
/// # Arguments
/// - `a` (array): First vector of floats
/// - `b` (array): Second vector of floats
///
/// # Returns
/// Array containing element-wise sum
///
/// # Example
/// ```xcl
/// cpu.simd_add([1.0, 2.0], [3.0, 4.0])  // Returns [4.0, 6.0]
/// ```
#[xcl_fn("cpu.simd_add", signature = "(a: array, b: array) -> array")]
pub fn simd_add(a: &[Value], b: &[Value]) -> Value {
    let len = a.len().min(b.len());
    let result: Vec<Value> = (0..len)
        .map(|i| {
            let av = a[i].as_f64().unwrap_or(0.0);
            let bv = b[i].as_f64().unwrap_or(0.0);
            Value::from_f64(av + bv)
        })
        .collect();
    Value::from_array(result)
}

/// Vector multiply (conceptual).
///
/// # Arguments
/// - `a` (array): First vector of floats
/// - `b` (array): Second vector of floats
///
/// # Returns
/// Array containing element-wise product
///
/// # Example
/// ```xcl
/// cpu.simd_mul([2.0, 3.0], [4.0, 5.0])  // Returns [8.0, 15.0]
/// ```
#[xcl_fn("cpu.simd_mul", signature = "(a: array, b: array) -> array")]
pub fn simd_mul(a: &[Value], b: &[Value]) -> Value {
    let len = a.len().min(b.len());
    let result: Vec<Value> = (0..len)
        .map(|i| {
            let av = a[i].as_f64().unwrap_or(0.0);
            let bv = b[i].as_f64().unwrap_or(0.0);
            Value::from_f64(av * bv)
        })
        .collect();
    Value::from_array(result)
}

/// Dot product of two vectors.
///
/// # Arguments
/// - `a` (array): First vector of floats
/// - `b` (array): Second vector of floats
///
/// # Returns
/// Float dot product result
///
/// # Example
/// ```xcl
/// cpu.simd_dot([1.0, 2.0, 3.0], [4.0, 5.0, 6.0])  // Returns 32.0
/// ```
#[xcl_fn("cpu.simd_dot", signature = "(a: array, b: array) -> float")]
pub fn simd_dot(a: &[Value], b: &[Value]) -> Value {
    let len = a.len().min(b.len());
    let sum: f64 = (0..len)
        .map(|i| a[i].as_f64().unwrap_or(0.0) * b[i].as_f64().unwrap_or(0.0))
        .sum();
    Value::from_f64(sum)
}
