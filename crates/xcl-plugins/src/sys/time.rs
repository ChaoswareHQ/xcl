//! System time and clock interface for xcl-plugins.
//!
//! Provides monotonic clock, wall clock, timers, and time conversion.

use std::time::{SystemTime, UNIX_EPOCH, Instant};
use xcl_macros::xcl_fn;
use xcl_vm::Value;

// Static reference instant for measuring elapsed time
use std::sync::LazyLock;
static START_INSTANT: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Get the current monotonic clock time in nanoseconds.
///
/// Monotonic time always increases and is not affected by system clock changes.
/// Use this for measuring elapsed time and timeouts.
///
/// # Returns
/// Integer nanoseconds since an unspecified point in the past
///
/// # Example
/// ```xcl
/// sys.monotonic_ns()
/// ```
#[xcl_fn]
pub fn monotonic_ns() -> Value {
    let now = Instant::now();
    Value::from_i48(now.duration_since(*START_INSTANT).as_nanos() as i64)
}

/// Get the current monotonic clock time in milliseconds.
///
/// # Returns
/// Integer milliseconds since an unspecified point in the past
///
/// # Example
/// ```xcl
/// sys.monotonic_ms()
/// ```
#[xcl_fn]
pub fn monotonic_ms() -> Value {
    let now = Instant::now();
    Value::from_i48(now.duration_since(*START_INSTANT).as_millis() as i64)
}

/// Get the current wall clock time as Unix timestamp in seconds.
///
/// # Returns
/// Integer seconds since Unix epoch (1970-01-01 00:00:00 UTC)
///
/// # Example
/// ```xcl
/// sys.unix_time()
/// ```
#[xcl_fn]
pub fn unix_time() -> Value {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    Value::from_i48(duration.as_secs() as i64)
}

/// Get the current wall clock time as Unix timestamp in milliseconds.
///
/// # Returns
/// Integer milliseconds since Unix epoch
///
/// # Example
/// ```xcl
/// sys.unix_time_ms()
/// ```
#[xcl_fn]
pub fn unix_time_ms() -> Value {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    Value::from_i48(duration.as_millis() as i64)
}

/// Get the current wall clock time as Unix timestamp in nanoseconds.
///
/// # Returns
/// Integer nanoseconds since Unix epoch
///
/// # Example
/// ```xcl
/// sys.unix_time_ns()
/// ```
#[xcl_fn]
pub fn unix_time_ns() -> Value {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    Value::from_i48(duration.as_nanos() as i64)
}

/// Get the clock resolution in nanoseconds.
///
/// # Returns
/// Integer nanoseconds per tick
///
/// # Example
/// ```xcl
/// sys.clock_resolution()
/// ```
#[xcl_fn]
pub fn clock_resolution() -> Value {
    let duration = std::time::Duration::from_nanos(1);
    Value::from_i48(duration.as_nanos() as i64)
}

/// Sleep for a specified number of nanoseconds.
///
/// # Arguments
/// - `nanos` (int): Number of nanoseconds to sleep
///
/// # Example
/// ```xcl
/// sys.sleep_ns(1_000_000)  // Sleep for 1ms
/// ```
#[xcl_fn]
pub fn sleep_ns(nanos: i64) -> Value {
    std::thread::sleep(std::time::Duration::from_nanos(nanos as u64));
    Value::nil()
}

/// Get system uptime in seconds.
///
/// # Returns
/// Integer seconds since system boot
///
/// # Example
/// ```xcl
/// sys.uptime()
/// ```
#[xcl_fn]
pub fn uptime() -> Value {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
            if let Some(uptime_str) = content.split('.').next() {
                if let Ok(uptime) = uptime_str.trim().parse::<i64>() {
                    return Value::from_i48(uptime);
                }
            }
        }
        Value::from_i48(0)
    }
    #[cfg(target_os = "macos")]
    {
        let mut timeval: libc::timeval = unsafe { std::mem::zeroed() };
        if unsafe { libc::gettimeofday(&mut timeval, std::ptr::null_mut()) } == 0 {
            Value::from_i48(timeval.tv_sec as i64)
        } else {
            Value::from_i48(0)
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Value::from_i48(0)
    }
}
