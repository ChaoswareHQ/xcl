//! Linux-specific system interface for xcl-plugins.
//!
//! Provides Linux-specific functionality like procfs access, cgroups, namespaces, etc.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Get Linux kernel version.
///
/// # Returns
/// String kernel version (e.g., "5.15.0-generic")
///
/// # Example
/// ```xcl
/// sys.os.kernel_version()
/// ```
#[xcl_fn("sys.os.kernel_version", signature = "() -> str")]
pub fn kernel_version() -> Value {
    #[cfg(target_os = "linux")]
    {
        let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
        if unsafe { libc::uname(&mut uts) } == 0 {
            let release = unsafe {
                std::ffi::CStr::from_ptr(uts.release.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            };
            Value::from_str_boxed(release)
        } else {
            Value::from_str_boxed("unknown".to_string())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_str_boxed("not available on this platform".to_string())
    }
}

/// Get system load averages.
///
/// # Returns
/// Array of three floats: 1min, 5min, 15min load averages
///
/// # Example
/// ```xcl
/// loads = sys.os.load_average()
/// ```
#[xcl_fn("sys.os.load_average", signature = "() -> array")]
pub fn load_average() -> Value {
    #[cfg(target_os = "linux")]
    {
        let mut loads = [0.0f64; 3];
        if unsafe { libc::getloadavg(&mut loads as *mut f64, 3) } > 0 {
            let arr = vec![
                Value::from_f64(loads[0]),
                Value::from_f64(loads[1]),
                Value::from_f64(loads[2]),
            ];
            Value::from_array(arr)
        } else {
            Value::from_array(vec![])
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_array(vec![])
    }
}

/// Get memory information from /proc/meminfo.
///
/// # Returns
/// Map containing memory information in kB (MemTotal, MemFree, Buffers, Cached, etc.)
///
/// # Example
/// ```xcl
/// mem = sys.os.meminfo()
/// ```
#[xcl_fn("sys.os.meminfo", signature = "() -> map")]
pub fn meminfo() -> Value {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/proc/meminfo") {
            Ok(content) => {
                let mut info = HashMap::new();
                for line in content.lines() {
                    if let Some((key, val)) = line.split_once(':') {
                        let key = key.trim();
                        let val = val.trim();
                        // Extract numeric value
                        if let Some(num_str) = val.split_whitespace().next() {
                            if let Ok(num) = num_str.parse::<i64>() {
                                info.insert(key.to_string(), Value::from_i48(num));
                            }
                        }
                    }
                }
                Value::from_map(info)
            }
            Err(_) => Value::from_map(HashMap::new()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get CPU information from /proc/cpuinfo.
///
/// # Returns
/// Map containing CPU information (model name, cores, etc.)
///
/// # Example
/// ```xcl
/// cpu = sys.os.cpuinfo()
/// ```
#[xcl_fn("sys.os.cpuinfo", signature = "() -> map")]
pub fn cpuinfo() -> Value {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/proc/cpuinfo") {
            Ok(content) => {
                let mut info = HashMap::new();
                let mut core_count = 0i64;
                for line in content.lines() {
                    if let Some((key, val)) = line.split_once(':') {
                        let key = key.trim();
                        let val = val.trim();
                        if key == "processor" {
                            core_count += 1;
                        } else if key == "model name" {
                            info.insert("model".to_string(), Value::from_str_boxed(val.to_string()));
                        } else if key == "cpu MHz" {
                            info.insert("mhz".to_string(), Value::from_str_boxed(val.to_string()));
                        } else if key == "cache size" {
                            info.insert("cache".to_string(), Value::from_str_boxed(val.to_string()));
                        }
                    }
                }
                info.insert("cores".to_string(), Value::from_i48(core_count));
                Value::from_map(info)
            }
            Err(_) => Value::from_map(HashMap::new()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get disk usage information.
///
/// # Arguments
/// - `path` (str): Path to check
///
/// # Returns
/// Map containing disk usage (total, free, available in bytes)
///
/// # Example
/// ```xcl
/// disk = sys.os.disk_usage("/")
/// ```
#[xcl_fn("sys.os.disk_usage", signature = "(path: str) -> map")]
pub fn disk_usage(path: &str) -> Value {
    #[cfg(target_os = "linux")]
    {
        let mut statvfs: libc::statvfs = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::statvfs(path.as_ptr() as *const i8, &mut statvfs)
        };

        let mut info = HashMap::new();
        if result == 0 {
            let block_size = statvfs.f_frsize as u64;
            info.insert("total".to_string(), Value::from_i48((statvfs.f_blocks as u64 * block_size) as i64));
            info.insert("free".to_string(), Value::from_i48((statvfs.f_bfree as u64 * block_size) as i64));
            info.insert("available".to_string(), Value::from_i48((statvfs.f_bavail as u64 * block_size) as i64));
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get cgroup information for the current process.
///
/// # Returns
/// Map containing cgroup information
///
/// # Example
/// ```xcl
/// sys.os.cgroup_info()
/// ```
#[xcl_fn("sys.os.cgroup_info", signature = "() -> map")]
pub fn cgroup_info() -> Value {
    #[cfg(target_os = "linux")]
    {
        let mut info = HashMap::new();
        if let Ok(content) = std::fs::read_to_string("/proc/self/cgroup") {
            for line in content.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 {
                    let subsystem = parts[0].trim();
                    let cgroup_path = parts[2].trim();
                    info.insert(
                        format!("cgroup_{}", subsystem),
                        Value::from_str_boxed(cgroup_path.to_string()),
                    );
                }
            }
        }
        Value::from_map(info)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_map(HashMap::new())
    }
}

/// Get network interface information.
///
/// # Returns
/// Array of maps, each containing interface information
///
/// # Example
/// ```xcl
/// sys.os.net_interfaces()
/// ```
#[xcl_fn("sys.os.net_interfaces", signature = "() -> array")]
pub fn net_interfaces() -> Value {
    #[cfg(target_os = "linux")]
    {
        let mut interfaces = Vec::new();
        if let Ok(content) = std::fs::read_to_string("/proc/net/dev") {
            for line in content.lines().skip(2) {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() == 2 {
                    let mut info = HashMap::new();
                    let name = parts[0].trim();
                    info.insert("name".to_string(), Value::from_str_boxed(name.to_string()));

                    let stats: Vec<&str> = parts[1].trim().split_whitespace().collect();
                    if stats.len() >= 9 {
                        if let Ok(rx_bytes) = stats[0].parse::<i64>() {
                            info.insert("rx_bytes".to_string(), Value::from_i48(rx_bytes));
                        }
                        if let Ok(tx_bytes) = stats[8].parse::<i64>() {
                            info.insert("tx_bytes".to_string(), Value::from_i48(tx_bytes));
                        }
                    }
                    interfaces.push(Value::from_map(info));
                }
            }
        }
        Value::from_array(interfaces)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_array(vec![])
    }
}
