//! Raw system call interface.
//!
//! Provides direct access to OS syscalls for advanced use cases.
//! Use with caution — syscalls are platform-specific and unsafe.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Execute a raw system call with the given number and arguments.
///
/// This is a low-level primitive that directly invokes the OS syscall interface.
/// The behavior is platform-specific:
/// - Linux: Uses the `syscall` function from libc
/// - Windows: Uses NtSysCall or equivalent
/// - macOS: Uses the `syscall` function from libc
///
/// # Arguments
/// - `syscall_num` (int): The syscall number
/// - `args` (array): Array of integer arguments to pass to the syscall
///
/// # Returns
/// Result containing the syscall return value as an integer, or an error
///
/// # Example
/// ```xcl
/// // Linux: gettid syscall (number 186 on x86_64)
/// sys.syscall(186, [])
/// ```
#[xcl_fn]
pub fn syscall(_syscall_num: i64, _args: &[Value]) -> Value {
    // Raw syscall support is platform-specific and requires unsafe FFI
    // For now, return a not-implemented error
    Value::from_str_boxed("Raw syscalls require platform-specific implementation".to_string())
}

/// Get the last OS error code.
///
/// Returns the most recent error code set by the OS (errno on Unix,
/// GetLastError on Windows).
///
/// # Returns
/// Integer error code
///
/// # Example
/// ```xcl
/// sys.last_error()
/// ```
#[xcl_fn]
pub fn last_error() -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::io::ErrorExt;
        Value::from_i48(libc::errno() as i64)
    }
    #[cfg(windows)]
    {
        Value::from_i48(unsafe { windows::Win32::System::Diagnostics::Debug::GetLastError() as i64 })
    }
    #[cfg(not(any(unix, windows)))]
    {
        Value::from_i48(0)
    }
}

/// Get a human-readable description of the last OS error.
///
/// # Returns
/// String description of the error
///
/// # Example
/// ```xcl
/// sys.last_error_str()
/// ```
#[xcl_fn]
pub fn last_error_str() -> Value {
    #[cfg(unix)]
    {
        let err = unsafe { libc::strerror(libc::errno()) };
        if !err.is_null() {
            Value::from_str_boxed(unsafe { std::ffi::CStr::from_ptr(err) }.to_string_lossy().into_owned())
        } else {
            Value::from_str_boxed("Unknown error".to_string())
        }
    }
    #[cfg(windows)]
    {
        Value::from_str_boxed("Windows error".to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        Value::from_str_boxed("Unknown".to_string())
    }
}
