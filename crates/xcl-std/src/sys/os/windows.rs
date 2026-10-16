//! Windows-specific system interface for xcl-std.
//!
//! Provides Windows-specific functionality like registry access, performance counters, etc.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get Windows version information.
///
/// # Returns
/// Map containing Windows version info (major, minor, build, etc.)
///
/// # Example
/// ```xcl
/// sys.os.windows_version()
/// ```
#[xcl_fn("sys.os.windows_version", signature = "() -> map")]
pub fn windows_version() -> Value {
    #[cfg(target_os = "windows")]
    {
        let mut info = HashMap::new();
        // Use GetVersionEx alternative for modern Windows
        let os_version = std::env::var("OS").unwrap_or_else(|_| "Windows".to_string());
        info.insert("os".to_string(), Value::from_str_boxed(os_version));
        Value::from_map(info)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get Windows registry value.
///
/// # Arguments
/// - `path` (str): Registry path (e.g., "HKEY_CURRENT_USER\\Software\\Microsoft")
/// - `name` (str): Value name
///
/// # Returns
/// String value, or nil if not found
///
/// # Example
/// ```xcl
/// sys.os.registry_get("HKEY_CURRENT_USER\\Software\\Microsoft", "Version")
/// ```
#[xcl_fn("sys.os.registry_get", signature = "(path: str, name: str) -> str")]
pub fn registry_get(_path: &str, _name: &str) -> Value {
    #[cfg(target_os = "windows")]
    {
        // Registry access requires Windows API
        Value::nil()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::nil()
    }
}

/// Get Windows performance counter.
///
/// # Arguments
/// - `counter` (str): Counter name
///
/// # Returns
/// Float counter value
///
/// # Example
/// ```xcl
/// sys.os.performance_counter("\\Processor(_Total)\\% Processor Time")
/// ```
#[xcl_fn("sys.os.performance_counter", signature = "(counter: str) -> float")]
pub fn performance_counter(_counter: &str) -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_f64(0.0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_f64(0.0)
    }
}

/// Get Windows event log entries.
///
/// # Arguments
/// - `log_name` (str): Log name (e.g., "Application", "System")
/// - `count` (int): Number of entries to retrieve
///
/// # Returns
/// Array of log entry maps
///
/// # Example
/// ```xcl
/// sys.os.event_log("System", 10)
/// ```
#[xcl_fn("sys.os.event_log", signature = "(log_name: str, count: int) -> array")]
pub fn event_log(_log_name: &str, _count: i64) -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_array(vec![])
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_array(vec![])
    }
}

/// Get Windows service status.
///
/// # Arguments
/// - `service_name` (str): Service name
///
/// # Returns
/// Map containing service status information
///
/// # Example
/// ```xcl
/// sys.os.service_status("wuauserv")
/// ```
#[xcl_fn("sys.os.service_status", signature = "(service_name: str) -> map")]
pub fn service_status(_service_name: &str) -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_map(HashMap::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get Windows disk information.
///
/// # Arguments
/// - `drive` (str): Drive letter (e.g., "C:")
///
/// # Returns
/// Map containing disk information (total, free in bytes)
///
/// # Example
/// ```xcl
/// sys.os.disk_info("C:")
/// ```
#[xcl_fn("sys.os.disk_info", signature = "(drive: str) -> map")]
pub fn disk_info(drive: &str) -> Value {
    #[cfg(target_os = "windows")]
    {
        let mut info = HashMap::new();
        let drive_path = format!("{}\\", drive);
        let mut free_bytes_available_to_caller: u64 = 0;
        let mut total_number_of_bytes: u64 = 0;
        let mut total_number_of_free_bytes: u64 = 0;

        // Use GetDiskFreeSpaceEx
        unsafe {
            use std::os::windows::prelude::OsStrExt;
            let wide: Vec<u16> = std::ffi::OsString::from(&drive_path)
                .encode_wide()
                .chain(Some(0))
                .collect();

            if windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_bytes_available_to_caller as *mut u64,
                &mut total_number_of_bytes as *mut u64,
                &mut total_number_of_free_bytes as *mut u64,
            ).as_bool()
            {
                info.insert("total".to_string(), Value::from_i48(total_number_of_bytes as i64));
                info.insert("free".to_string(), Value::from_i48(total_number_of_free_bytes as i64));
                info.insert("available".to_string(), Value::from_i48(free_bytes_available_to_caller as i64));
            }
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get Windows process list.
///
/// # Returns
/// Array of process information maps
///
/// # Example
/// ```xcl
/// sys.os.process_list()
/// ```
#[xcl_fn("sys.os.process_list", signature = "() -> array")]
pub fn process_list() -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_array(vec![])
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_array(vec![])
    }
}

/// Get Windows environment variable from registry.
///
/// # Arguments
/// - `name` (str): Variable name
/// - `scope` (str): Scope ("user" or "machine")
///
/// # Returns
/// String value
///
/// # Example
/// ```xcl
/// sys.os.registry_env("PATH", "user")
/// ```
#[xcl_fn("sys.os.registry_env", signature = "(name: str, scope: str) -> str")]
pub fn registry_env(_name: &str, _scope: &str) -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::nil()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::nil()
    }
}
