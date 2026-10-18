//! macOS-specific system interface for xcl-plugins.
//!
//! Provides macOS-specific functionality like launchd, Spotlight, etc.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get macOS version information.
///
/// # Returns
/// Map containing macOS version info (major, minor, patch)
///
/// # Example
/// ```xcl
/// sys.os.macos_version()
/// ```
#[xcl_fn]
pub fn macos_version() -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = HashMap::new();
        let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
        if unsafe { libc::uname(&mut uts) } == 0 {
            let release = unsafe {
                std::ffi::CStr::from_ptr(uts.release.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            };
            info.insert("release".to_string(), Value::from_str_boxed(release));
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get macOS system profiler information.
///
/// # Arguments
/// - `type_name` (str): Type of info (e.g., "SPHardwareDataType", "SPSoftwareDataType")
///
/// # Returns
/// String containing system profiler output
///
/// # Example
/// ```xcl
/// sys.os.system_profiler("SPHardwareDataType")
/// ```
#[xcl_fn]
pub fn system_profiler(type_name: &str) -> Value {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        match Command::new("system_profiler")
            .arg(type_name)
            .arg("-xml")
            .output()
        {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout).to_string();
                Value::from_str_boxed(result)
            }
            Err(e) => Value::from_str_boxed(format!("Error: {}", e)),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_str_boxed("Not available on this platform".to_string())
    }
}

/// Get macOS energy information.
///
/// # Returns
/// Map containing energy information (battery percentage, etc.)
///
/// # Example
/// ```xcl
/// sys.os.energy_info()
/// ```
#[xcl_fn]
pub fn energy_info() -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = HashMap::new();
        use std::process::Command;
        match Command::new("pmset")
            .arg("-g")
            .arg("bat")
            .output()
        {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout);
                info.insert("status".to_string(), Value::from_str_boxed(result.to_string()));
            }
            Err(_) => {}
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get macOS launchd service status.
///
/// # Arguments
/// - `service_name` (str): Service name (e.g., "com.apple.Safari")
///
/// # Returns
/// Map containing service status
///
/// # Example
/// ```xcl
/// sys.os.launchd_status("com.apple.Safari")
/// ```
#[xcl_fn]
pub fn launchd_status(service_name: &str) -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = HashMap::new();
        use std::process::Command;
        match Command::new("launchctl")
            .arg("list")
            .arg(service_name)
            .output()
        {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout);
                info.insert("output".to_string(), Value::from_str_boxed(result.to_string()));
            }
            Err(e) => {
                info.insert("error".to_string(), Value::from_str_boxed(e.to_string()));
            }
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get macOS disk information.
///
/// # Arguments
/// - `mount_point` (str): Mount point (e.g., "/")
///
/// # Returns
/// Map containing disk information (total, free in bytes)
///
/// # Example
/// ```xcl
/// sys.os.disk_info("/")
/// ```
#[xcl_fn]
pub fn disk_info(mount_point: &str) -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = HashMap::new();
        let mut statvfs: libc::statvfs = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::statvfs(mount_point.as_ptr() as *const i8, &mut statvfs)
        };

        if result == 0 {
            let block_size = statvfs.f_frsize as u64;
            info.insert("total".to_string(), Value::from_i48((statvfs.f_blocks as u64 * block_size) as i64));
            info.insert("free".to_string(), Value::from_i48((statvfs.f_bfree as u64 * block_size) as i64));
            info.insert("available".to_string(), Value::from_i48((statvfs.f_bavail as u64 * block_size) as i64));
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get macOS memory pressure information.
///
/// # Returns
/// Map containing memory pressure info
///
/// # Example
/// ```xcl
/// sys.os.memory_pressure()
/// ```
#[xcl_fn]
pub fn memory_pressure() -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = HashMap::new();
        let mut mem_pressure: libc::vm_pressure_info_data_t = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<libc::vm_pressure_info_data_t>() as libc::mach_msg_type_number_t;

        if unsafe {
            libc::vm_pressure_info(
                libc::VM_PRESSURE_PEEK,
                0,
                &mut mem_pressure,
                &mut size,
            )
        } == 0
        {
            info.insert("level".to_string(), Value::from_i48(mem_pressure.pli_level as i64));
            info.insert("default_purge_threshold".to_string(), Value::from_i48(mem_pressure.pli_default_purge_threshold as i64));
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get macOS Spotlight metadata for a file.
///
/// # Arguments
/// - `path` (str): File path
///
/// # Returns
/// Map containing metadata
///
/// # Example
/// ```xcl
/// sys.os.spotlight_metadata("/path/to/file")
/// ```
#[xcl_fn]
pub fn spotlight_metadata(path: &str) -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = HashMap::new();
        use std::process::Command;
        match Command::new("mdls")
            .arg("-raw")
            .arg(path)
            .output()
        {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout);
                for line in result.lines() {
                    if let Some((key, val)) = line.split_once('=') {
                        info.insert(key.trim().to_string(), Value::from_str_boxed(val.trim().to_string()));
                    }
                }
            }
            Err(e) => {
                info.insert("error".to_string(), Value::from_str_boxed(e.to_string()));
            }
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get macOS network interface information.
///
/// # Returns
/// Array of network interface maps
///
/// # Example
/// ```xcl
/// sys.os.net_interfaces()
/// ```
#[xcl_fn]
pub fn net_interfaces() -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut interfaces = Vec::new();
        use std::process::Command;
        match Command::new("ifconfig")
            .arg("-l")
            .output()
        {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout);
                for iface in result.split_whitespace() {
                    let mut info = HashMap::new();
                    info.insert("name".to_string(), Value::from_str_boxed(iface.to_string()));
                    interfaces.push(Value::from_map(info));
                }
            }
            Err(_) => {}
        }
        Value::from_array(interfaces)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Value::from_array(vec![])
    }
}
