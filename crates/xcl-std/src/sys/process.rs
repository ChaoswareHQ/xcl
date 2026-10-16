//! Process management for xcl-std.
//!
//! Provides low-level process operations: fork, exec, spawn, and process information.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Fork the current process (Unix only).
///
/// Creates a duplicate of the current process. Returns 0 in the child process,
/// and the child's PID in the parent process.
///
/// # Returns
/// Result containing the PID (0 in child, child PID in parent), or an error
///
/// # Platform
/// Unix only (Linux, macOS, BSD, Android). Returns error on Windows.
///
/// # Example
/// ```xcl
/// result = sys.fork()
/// if result.ok
///     if result.value == 0
///         io.println("I am the child")
///     else
///         io.println("I am the parent, child PID: " + str(result.value))
/// ```
#[xcl_fn("sys.fork", signature = "() -> result")]
pub fn fork() -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            let pid = libc::fork();
            if pid == -1 {
                let err = std::io::Error::last_os_error();
                Value::from_str_boxed(format!("Fork failed: {}", err))
            } else {
                Value::from_i48(pid as i64)
            }
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("fork() is not available on this platform".to_string())
    }
}

/// Execute a new program, replacing the current process image (Unix only).
///
/// Replaces the current process with a new program. Does not return on success.
///
/// # Arguments
/// - `path` (str): Path to the executable
/// - `args` (array): Array of string arguments
///
/// # Returns
/// Only returns on error
///
/// # Example
/// ```xcl
/// sys.exec("/bin/echo", ["Hello", "World"])
/// ```
#[xcl_fn("sys.exec", signature = "(path: str, args: array) -> result")]
pub fn exec(path: &str, args: &[Value]) -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Command;

        let mut cmd = Command::new(path);
        for arg in args {
            if let Some(s) = arg.as_str() {
                cmd.arg(s);
            }
        }

        unsafe {
            let result = cmd.exec();
            Value::from_str_boxed(format!("exec failed: {:?}", result))
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("exec() is not available on this platform".to_string())
    }
}

/// Get information about a process by PID.
///
/// # Arguments
/// - `pid` (int): Process ID to query
///
/// # Returns
/// Map containing process information (pid, status, etc.)
///
/// # Example
/// ```xcl
/// sys.process_info(1234)
/// ```
#[xcl_fn("sys.process_info", signature = "(pid: int) -> result")]
pub fn process_info(pid: i64) -> Value {
    #[cfg(unix)]
    {
        let pid_path = format!("/proc/{}/status", pid);
        match std::fs::read_to_string(&pid_path) {
            Ok(content) => {
                let mut info = std::collections::HashMap::new();
                info.insert("pid".to_string(), Value::from_i48(pid));
                for line in content.lines() {
                    if let Some((key, val)) = line.split_once(':') {
                        info.insert(
                            key.trim().to_string(),
                            Value::from_str_boxed(val.trim().to_string()),
                        );
                    }
                }
                Value::from_map(info)
            }
            Err(e) => Value::from_str_boxed(format!("Failed to read process info: {}", e)),
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Process info via /proc is not available on this platform".to_string())
    }
}

/// Kill a process by PID with the default signal (SIGTERM).
///
/// # Arguments
/// - `pid` (int): Process ID to kill
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.kill(1234)
/// ```
#[xcl_fn("sys.kill", signature = "(pid: int) -> result")]
pub fn kill(pid: i64) -> Value {
    #[cfg(unix)]
    {
        unsafe {
            let result = libc::kill(pid as i32, libc::SIGTERM);
            if result == 0 {
                Value::from_i48(0)
            } else {
                Value::from_str_boxed(format!("Failed to kill process {}", pid))
            }
        }
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        match Command::new("taskkill")
            .args(&["/F", "/PID", &pid.to_string()])
            .status()
        {
            Ok(status) => Value::from_i48(if status.success() { 0 } else { 1 }),
            Err(e) => Value::from_str_boxed(format!("Failed to kill process: {}", e)),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Value::from_str_boxed("kill() is not available on this platform".to_string())
    }
}

/// Send a specific signal to a process.
///
/// # Arguments
/// - `pid` (int): Process ID
/// - `signal` (int): Signal number (e.g., 2 for SIGINT, 9 for SIGKILL, 15 for SIGTERM)
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.signal(1234, 9)  // SIGKILL
/// ```
#[xcl_fn("sys.signal", signature = "(pid: int, signal: int) -> result")]
pub fn signal(pid: i64, sig: i64) -> Value {
    #[cfg(unix)]
    {
        unsafe {
            let result = libc::kill(pid as i32, sig as i32);
            if result == 0 {
                Value::from_i48(0)
            } else {
                Value::from_str_boxed(format!("Failed to send signal {} to process {}", sig, pid))
            }
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Signals are not available on this platform".to_string())
    }
}
