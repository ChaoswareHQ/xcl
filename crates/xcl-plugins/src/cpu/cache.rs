//! Cache information for xcl-plugins.
//!
//! Provides CPU cache line size and prefetch hints.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the CPU cache line size.
///
/// # Returns
/// Integer cache line size in bytes (typically 64)
///
/// # Example
/// ```xcl
/// cpu.cache_line_size()
/// ```
#[xcl_fn("cpu.cache_line_size", signature = "() -> int")]
pub fn cache_line_size() -> Value {
    #[cfg(target_arch = "x86_64")]
    {
        // Read cache line size from CPUID
        let mut cache_line: u32 = 0;
        unsafe {
            core::arch::asm!(
                "cpuid",
                in("eax") 0x80000006,
                out("eax") _,
                out("ebx") _,
                out("ecx") _,
                out("edx") cache_line,
            );
        }
        // Cache line size is in the lower 8 bits of EDX
        Value::from_i48((cache_line & 0xFF) as i64)
    }
    #[cfg(target_arch = "aarch64")]
    {
        // On ARM64, typical cache line size is 64 bytes
        Value::from_i48(64)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Value::from_i48(64)
    }
}

/// Prefetch a memory address into cache.
///
/// # Arguments
/// - `ptr` (int): Memory address to prefetch
/// - `locality` (int): Cache locality (0=none, 1=low, 2=medium, 3=high)
/// - `read` (bool): true for read prefetch, false for write prefetch
///
/// # Example
/// ```xcl
/// cpu.prefetch(ptr, 3, true)
/// ```
#[xcl_fn("cpu.prefetch", signature = "(ptr: int, locality: int, read: bool) -> nil")]
pub fn prefetch(ptr: i64, locality: i64, read: bool) -> Value {
    if ptr == 0 {
        return Value::nil();
    }
    #[cfg(target_arch = "x86_64")]
    {
        let locality = locality.min(3).max(0) as i32;
        unsafe {
            if read {
                core::arch::asm!(
                    "prefetcht{{0}} [{1}]",
                    operands(locality),
                    in(reg) ptr as *const u8,
                    options(nostack, readonly),
                );
            } else {
                core::arch::asm!(
                    "prefetchw [{0}]",
                    in(reg) ptr as *const u8,
                    options(nostack, readonly),
                );
            }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        unsafe {
            core::arch::asm!(
                "prfm pldlstrm, [{0}]",
                in(reg) ptr as *const u8,
                options(nostack, readonly),
            );
        }
    }
    Value::nil()
}

/// Get cache hierarchy information.
///
/// # Returns
/// Map containing cache information (L1, L2, L3 sizes)
///
/// # Example
/// ```xcl
/// cpu.cache_info()
/// ```
#[xcl_fn("cpu.cache_info", signature = "() -> map")]
pub fn cache_info() -> Value {
    let mut info = HashMap::new();
    info.insert("cache_line_size".to_string(), cache_line_size());

    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in content.lines() {
                if let Some((key, val)) = line.split_once(':') {
                    let key = key.trim();
                    let val = val.trim();
                    if key == "cache size" {
                        if let Some(num_str) = val.split_whitespace().next() {
                            if let Ok(num) = num_str.parse::<i64>() {
                                info.insert("l2_size_kb".to_string(), Value::from_i48(num));
                            }
                        }
                    }
                }
            }
        }
    }

    // Default cache sizes
    if !info.contains_key("l1d_size_kb") {
        info.insert("l1d_size_kb".to_string(), Value::from_i48(32));
    }
    if !info.contains_key("l1i_size_kb") {
        info.insert("l1i_size_kb".to_string(), Value::from_i48(32));
    }
    if !info.contains_key("l2_size_kb") {
        info.insert("l2_size_kb".to_string(), Value::from_i48(256));
    }

    Value::from_map(info)
}

/// Align a pointer to cache line boundary.
///
/// # Arguments
/// - `ptr` (int): Memory address
///
/// # Returns
/// Integer aligned address
///
/// # Example
/// ```xcl
/// cpu.align_cache(ptr)
/// ```
#[xcl_fn("cpu.align_cache", signature = "(ptr: int) -> int")]
pub fn align_cache(ptr: i64) -> Value {
    let line_size = cache_line_size().as_i64().unwrap_or(64);
    let aligned = (ptr + line_size - 1) / line_size * line_size;
    Value::from_i48(aligned)
}

/// Check if two addresses are in the same cache line.
///
/// # Arguments
/// - `ptr1` (int): First memory address
/// - `ptr2` (int): Second memory address
///
/// # Returns
/// Boolean indicating if they share a cache line
///
/// # Example
/// ```xcl
/// cpu.same_cache_line(ptr1, ptr2)
/// ```
#[xcl_fn("cpu.same_cache_line", signature = "(ptr1: int, ptr2: int) -> bool")]
pub fn same_cache_line(ptr1: i64, ptr2: i64) -> Value {
    let line_size = cache_line_size().as_i64().unwrap_or(64);
    Value::bool_(ptr1 / line_size == ptr2 / line_size)
}
