//! Thread management for xcl-plugins.
//!
//! Provides OS-level thread operations: create, join, yield, park, and thread info.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get the current thread's ID.
///
/// # Returns
/// Integer thread ID
///
/// # Example
/// ```xcl
/// sys.thread_id()
/// ```
#[xcl_fn("sys.thread_id", signature = "() -> int")]
pub fn thread_id() -> Value {
    Value::from_i48(thread::current().id().as_u128() as i64)
}

/// Get the current thread's name.
///
/// # Returns
/// String thread name, or empty string if unnamed
///
/// # Example
/// ```xcl
/// sys.thread_name()
/// ```
#[xcl_fn("sys.thread_name", signature = "() -> str")]
pub fn thread_name() -> Value {
    let name = thread::current().name().unwrap_or("").to_string();
    Value::from_str_boxed(name)
}

/// Yield the current thread's timeslice.
///
/// Allows other threads to run. This is a hint to the scheduler and may be a no-op.
///
/// # Example
/// ```xcl
/// sys.thread_yield()
/// ```
#[xcl_fn("sys.thread_yield", signature = "() -> nil")]
pub fn thread_yield() -> Value {
    thread::yield_now();
    Value::nil()
}

/// Park (sleep) the current thread for a specified duration.
///
/// # Arguments
/// - `duration_ms` (int): Duration in milliseconds
///
/// # Example
/// ```xcl
/// sys.thread_park(100)  // Sleep for 100ms
/// ```
#[xcl_fn("sys.thread_park", signature = "(duration_ms: int) -> nil")]
pub fn thread_park(duration_ms: i64) -> Value {
    let duration = Duration::from_millis(duration_ms as u64);
    thread::park_timeout(duration);
    Value::nil()
}

/// Get the number of logical CPUs available.
///
/// # Returns
/// Integer count of logical CPUs
///
/// # Example
/// ```xcl
/// sys.cpu_count()
/// ```
#[xcl_fn("sys.cpu_count", signature = "() -> int")]
pub fn cpu_count() -> Value {
    let count = thread::available_parallelism().unwrap_or_else(|_| thread::available_parallelism().unwrap()).get();
    Value::from_i48(count as i64)
}

/// Get the number of active threads in the current process.
///
/// # Returns
/// Integer count of active threads
///
/// # Example
/// ```xcl
/// sys.thread_count()
/// ```
#[xcl_fn("sys.thread_count", signature = "() -> int")]
pub fn thread_count() -> Value {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
            for line in content.lines() {
                if line.starts_with("Threads:") {
                    if let Some(count_str) = line.split(':').nth(1) {
                        if let Ok(count) = count_str.trim().parse::<i64>() {
                            return Value::from_i48(count);
                        }
                    }
                }
            }
        }
        Value::from_i48(1)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_i48(1)
    }
}

/// Get thread stack size in bytes.
///
/// # Returns
/// Integer stack size in bytes (default is typically 2MB on Linux, 1MB on macOS)
///
/// # Example
/// ```xcl
/// sys.stack_size()
/// ```
#[xcl_fn("sys.stack_size", signature = "() -> int")]
pub fn stack_size() -> Value {
    #[cfg(unix)]
    {
        use std::mem;
        let mut stack: libc::stack_t = unsafe { mem::zeroed() };
        if unsafe { libc::pthread_getattr_np(thread::current().as_pthread_t(), &mut stack) } == 0 {
            Value::from_i48(stack.size as i64)
        } else {
            Value::from_i48(0)
        }
    }
    #[cfg(not(unix))]
    {
        // Default stack sizes
        #[cfg(target_os = "windows")]
        return Value::from_i48(1_048_576); // 1MB on Windows
        #[cfg(target_os = "macos")]
        return Value::from_i48(8_388_608); // 8MB on macOS
        #[cfg(target_os = "linux")]
        return Value::from_i48(8_388_608); // 8MB on Linux
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return Value::from_i48(2_097_152); // 2MB default
    }
}
