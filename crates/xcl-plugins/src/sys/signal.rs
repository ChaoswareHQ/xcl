//! Signal handling for xcl-plugins.
//!
//! Provides Unix signal management: install handlers, block, unblock, and pending signals.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Common signal constants.
const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
const SIGSEGV: i32 = 11;
const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;

/// Get the name of a signal by number.
///
/// # Arguments
/// - `signum` (int): Signal number
///
/// # Returns
/// String signal name (e.g., "SIGINT", "SIGKILL")
///
/// # Example
/// ```xcl
/// sys.signal_name(2)  // Returns "SIGINT"
/// ```
#[xcl_fn]
pub fn signal_name(signum: i64) -> Value {
    let name = match signum as i32 {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        10 => "SIGUSR1",
        11 => "SIGSEGV",
        12 => "SIGUSR2",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        16 => "SIGSTKFLT",
        17 => "SIGCHLD",
        18 => "SIGCONT",
        19 => "SIGSTOP",
        20 => "SIGTSTP",
        21 => "SIGTTIN",
        22 => "SIGTTOU",
        23 => "SIGURG",
        24 => "SIGXCPU",
        25 => "SIGXFSZ",
        26 => "SIGVTALRM",
        27 => "SIGPROF",
        28 => "SIGWINCH",
        29 => "SIGIO",
        30 => "SIGPWR",
        31 => "SIGSYS",
        _ => "UNKNOWN",
    };
    Value::from_str_boxed(name.to_string())
}

/// Block a signal from being delivered.
///
/// # Arguments
/// - `signum` (int): Signal number to block
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.signal_block(2)  // Block SIGINT
/// ```
#[xcl_fn]
pub fn signal_block(signum: i64) -> Value {
    #[cfg(unix)]
    {
        let mut set = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut set) };
        unsafe { libc::sigaddset(&mut set, signum as i32) };

        let result = unsafe { libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to block signal".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Signal blocking is not available on this platform".to_string())
    }
}

/// Unblock a previously blocked signal.
///
/// # Arguments
/// - `signum` (int): Signal number to unblock
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.signal_unblock(2)  // Unblock SIGINT
/// ```
#[xcl_fn]
pub fn signal_unblock(signum: i64) -> Value {
    #[cfg(unix)]
    {
        let mut set = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut set) };
        unsafe { libc::sigaddset(&mut set, signum as i32) };

        let result = unsafe { libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut()) };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to unblock signal".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Signal unblocking is not available on this platform".to_string())
    }
}

/// Check if a signal is pending.
///
/// # Arguments
/// - `signum` (int): Signal number to check
///
/// # Returns
/// Boolean indicating if the signal is pending
///
/// # Example
/// ```xcl
/// sys.signal_pending(2)  // Check if SIGINT is pending
/// ```
#[xcl_fn]
pub fn signal_pending(signum: i64) -> Value {
    #[cfg(unix)]
    {
        let mut pending = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::sigpending(&mut pending) };
        if result == 0 {
            let is_pending = unsafe { libc::sigismember(&pending, signum as i32) };
            Value::bool_(is_pending > 0)
        } else {
            Value::bool_(false)
        }
    }
    #[cfg(not(unix))]
    {
        Value::bool_(false)
    }
}

/// Pause the current thread until a signal is received.
///
/// # Returns
/// Integer signal number received, or -1 on error
///
/// # Example
/// ```xcl
/// sys.signal_wait()  // Wait for any signal
/// ```
#[xcl_fn]
pub fn signal_wait() -> Value {
    #[cfg(unix)]
    {
        let result = unsafe { libc::sigsuspend(std::ptr::null()) };
        Value::from_i48(result as i64)
    }
    #[cfg(not(unix))]
    {
        Value::from_i48(-1)
    }
}
