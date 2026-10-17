//! Universal utility domain for cross-platform operations.
//!
//! Provides functions for OS detection, environment variables, and path operations
//! that work across Linux, macOS, Android, Windows, and BSD.

use std::collections::HashMap;
use std::path::Path;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx, errors::universal};
use crate::core::result::{ok_result, err_result};

/// Get the name of the current operating system.
///
/// Returns a string identifying the OS: "linux", "macos", "windows", "android", or "bsd".
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.name() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $os = os.name
/// io.print "Running on: $os"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
///
/// ## NOTES
///
/// - On Android, returns "android".
/// - On BSD variants (FreeBSD, OpenBSD, NetBSD), returns "bsd".
/// - On macOS, returns "macos".
/// - On Windows, returns "windows".
/// - On Linux, returns "linux".
#[xcl_fn(name = "os.name", ret = "str")]
pub fn os_name(_ctx: &mut DomainCtx) -> Value {
    let os = std::env::consts::OS;
    let name = match os {
        "linux" => {
            // Check for Android
            if let Ok(libc_path) = std::fs::read_to_string("/system/build.prop")
                && libc_path.contains("ro.product.manufacturer")
            {
                return Value::from_str_boxed("android".to_string());
            }
            "linux"
        }
        "macos" => "macos",
        "windows" => "windows",
        "freebsd" | "openbsd" | "netbsd" => "bsd",
        _ => os,
    };
    Value::from_str_boxed(name.to_string())
}

/// Get the version of the current operating system.
///
/// Returns a string with the OS version information.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.version() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $version = os.version
/// io.print "OS version: $version"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "os.version", ret = "str")]
pub fn os_version(_ctx: &mut DomainCtx) -> Value {
    let version = match std::env::consts::OS {
        "linux" => {
            // Try to read /etc/os-release
            if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
                for line in content.lines() {
                    if line.starts_with("PRETTY_NAME=") {
                        let name = line.trim_start_matches("PRETTY_NAME=")
                            .trim_matches('"')
                            .to_string();
                        return Value::from_str_boxed(name);
                    }
                }
            }
            "unknown".to_string()
        }
        "macos" => {
            // Try sw_vers on macOS
            if let Ok(output) = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
            {
                let version = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string();
                if !version.is_empty() {
                    return Value::from_str_boxed(version);
                }
            }
            "unknown".to_string()
        }
        "windows" => {
            // On Windows, return the release info
            format!("Windows (arch: {})", std::env::consts::ARCH)
        }
        "freebsd" | "openbsd" | "netbsd" => {
            // Try uname -r on BSD
            if let Ok(output) = std::process::Command::new("uname")
                .arg("-r")
                .output()
            {
                let version = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string();
                if !version.is_empty() {
                    return Value::from_str_boxed(version);
                }
            }
            "unknown".to_string()
        }
        _ => "unknown".to_string(),
    };
    Value::from_str_boxed(version)
}

/// Get the CPU architecture of the current system.
///
/// Returns a string identifying the architecture: "x86_64", "aarch64", "arm", "x86", etc.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.arch() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $arch = os.arch
/// io.print "Architecture: $arch"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "os.arch", ret = "str")]
pub fn os_arch(_ctx: &mut DomainCtx) -> Value {
    Value::from_str_boxed(std::env::consts::ARCH.to_string())
}

/// Get a combined platform identifier.
///
/// Returns a string combining OS and architecture, e.g., "linux-x86_64", "macos-aarch64".
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.platform() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $platform = os.platform
/// io.print "Platform: $platform"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "os.platform", ret = "str")]
pub fn os_platform(_ctx: &mut DomainCtx) -> Value {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    Value::from_str_boxed(format!("{os}-{arch}"))
}

/// Get the family of the current operating system.
///
/// Returns "unix" for Unix-like systems (Linux, macOS, BSD, Android) or "windows" for Windows.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.family() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $family = os.family
/// if $family == "unix" {
///     io.print "Unix-like system"
/// }
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "os.family", ret = "str")]
pub fn os_family(_ctx: &mut DomainCtx) -> Value {
    let family = match std::env::consts::FAMILY {
        "unix" => "unix",
        "windows" => "windows",
        _ => "other",
    };
    Value::from_str_boxed(family.to_string())
}

/// Get an environment variable by name.
///
/// Returns the value of the environment variable, or nil if not set.
///
/// ## SYNTAX
///
/// ```xcl
/// fn env.get($key: str) -> str | nil
/// ```
///
/// ## PARAMETERS
///
/// `$key`: The name of the environment variable.
///
/// ## EXAMPLES
///
/// ```xcl
/// $home = env.get "HOME"
/// io.print "Home directory: $home"
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str` or `nil`
#[xcl_fn(name = "env.get", args = "key: str", ret = "str | nil")]
pub fn env_get(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    match std::env::var(key) {
        Ok(val) => Value::from_str_boxed(val),
        Err(_) => Value::nil(),
    }
}

/// Set an environment variable.
///
/// Sets the value of an environment variable. Returns true on success, false on failure.
///
/// ## SYNTAX
///
/// ```xcl
/// fn env.set($key: str; $value: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$key`: The name of the environment variable.
///
/// `$value`: The value to set.
///
/// ## EXAMPLES
///
/// ```xcl
/// env.set "MY_VAR" "my_value"
/// $val = env.get "MY_VAR"
/// io.print $val
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "env.set", args = "key: str; value: str", ret = "bool")]
pub fn env_set(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("").to_string();
    let value = ctx.arg(1).as_str().unwrap_or("").to_string();
    unsafe { std::env::set_var(&key, &value); }
    Value::bool_(true)
}

/// Remove an environment variable.
///
/// Removes an environment variable. Returns true if the variable existed, false otherwise.
///
/// ## SYNTAX
///
/// ```xcl
/// fn env.remove($key: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$key`: The name of the environment variable to remove.
///
/// ## EXAMPLES
///
/// ```xcl
/// env.remove "MY_VAR"
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "env.remove", args = "key: str", ret = "bool")]
pub fn env_remove(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    let had_value = std::env::var(key).is_ok();
    unsafe { std::env::remove_var(key); }
    Value::bool_(had_value)
}

/// List all environment variables.
///
/// Returns a map of all environment variable names to their values.
///
/// ## SYNTAX
///
/// ```xcl
/// fn env.list() -> dict
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $envs = env.list
/// io.print "Number of env vars: ${arr.len $envs}"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `dict`
#[xcl_fn(name = "env.list", ret = "dict")]
pub fn env_list(_ctx: &mut DomainCtx) -> Value {
    let mut map = HashMap::new();
    for (key, value) in std::env::vars() {
        map.insert(key, Value::from_str_boxed(value));
    }
    Value::from_map_boxed(map)
}

/// Join path segments together.
///
/// Joins multiple path segments using the platform-specific path separator.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.join($parts: arr) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$parts`: An array of path segments to join.
///
/// ## EXAMPLES
///
/// ```xcl
/// $path = path.join ["usr" "local" "bin"]
/// io.print $path  # /usr/local/bin on Unix, \usr\local\bin on Windows
/// ```
///
/// ## INPUT TYPES
///
/// - `arr`
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.join", args = "parts: arr", ret = "str")]
pub fn path_join(ctx: &mut DomainCtx) -> Value {
    let parts_val = ctx.arg(0);
    let parts: Vec<String> = if let Some(arr) = parts_val.as_array() {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
    } else {
        Vec::new()
    };

    let mut path = std::path::PathBuf::new();
    for part in parts {
        path.push(part);
    }
    Value::from_str_boxed(path.to_string_lossy().to_string())
}

/// Expand a path, replacing ~ with the home directory and environment variables.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.expand($path: str) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to expand.
///
/// ## EXAMPLES
///
/// ```xcl
/// $expanded = path.expand "~/Documents"
/// io.print $expanded  # /home/user/Documents on Unix
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.expand", args = "path: str", ret = "str")]
pub fn path_expand(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    
    // Handle ~ expansion
    let expanded = if let Some(rest) = path_str.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            format!("{home}/{rest}")
        } else if let Ok(home) = std::env::var("USERPROFILE") {
            format!("{home}\\{rest}")
        } else {
            path_str.to_string()
        }
    } else if path_str == "~" {
        std::env::var("HOME").unwrap_or_else(|_| {
            std::env::var("USERPROFILE").unwrap_or_else(|_| "~".to_string())
        })
    } else {
        // Try to expand environment variables in the form ${VAR} or $VAR
        let mut result = String::new();
        let mut chars = path_str.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '$' {
                if let Some('&') = chars.peek() {
                    chars.next(); // consume $
                    if let Some('{') = chars.peek() {
                        chars.next(); // consume {
                        let mut var_name = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == '}' {
                                chars.next(); // consume }
                                break;
                            }
                            var_name.push(ch);
                            chars.next();
                        }
                        if let Ok(val) = std::env::var(&var_name) {
                            result.push_str(&val);
                        }
                    } else {
                        // $VAR format (no braces)
                        let mut var_name = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch.is_alphanumeric() || ch == '_' {
                                var_name.push(ch);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Ok(val) = std::env::var(&var_name) {
                            result.push('$');
                            result.push_str(&var_name);
                            result.push_str(&val);
                        } else {
                            result.push(c);
                        }
                    }
                } else {
                    result.push(c);
                }
            } else {
                result.push(c);
            }
        }
        result
    };
    
    Value::from_str_boxed(expanded)
}

/// Get the directory name of a path.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.dirname($path: str) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to get the directory name of.
///
/// ## EXAMPLES
///
/// ```xcl
/// $dir = path.dirname "/usr/local/bin"
/// io.print $dir  # /usr/local
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.dirname", args = "path: str", ret = "str")]
pub fn path_dirname(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    let path = Path::new(path_str);
    let parent = path.parent().unwrap_or(Path::new("."));
    Value::from_str_boxed(parent.to_string_lossy().to_string())
}

/// Get the base name (file name) of a path.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.basename($path: str) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to get the base name of.
///
/// ## EXAMPLES
///
/// ```xcl
/// $name = path.basename "/usr/local/bin/python"
/// io.print $name  # python
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.basename", args = "path: str", ret = "str")]
pub fn path_basename(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    let path = Path::new(path_str);
    let file_name = path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.to_string());
    Value::from_str_boxed(file_name)
}

/// Get the file extension of a path.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.extension($path: str) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to get the extension of.
///
/// ## EXAMPLES
///
/// ```xcl
/// $ext = path.extension "file.txt"
/// io.print $ext  # txt
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.extension", args = "path: str", ret = "str")]
pub fn path_extension(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    let path = Path::new(path_str);
    let ext = path.extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    Value::from_str_boxed(ext)
}

/// Check if a path exists.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.exists($path: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to check.
///
/// ## EXAMPLES
///
/// ```xcl
/// if path.exists "/etc/hosts" {
///     io.print "File exists"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "path.exists", args = "path: str", ret = "bool")]
pub fn path_exists(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    Value::bool_(Path::new(path_str).exists())
}

/// Check if a path is a file.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.is_file($path: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to check.
///
/// ## EXAMPLES
///
/// ```xcl
/// if path.is_file "/etc/hosts" {
///     io.print "It's a file"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "path.is_file", args = "path: str", ret = "bool")]
pub fn path_is_file(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    Value::bool_(Path::new(path_str).is_file())
}

/// Check if a path is a directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.is_dir($path: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to check.
///
/// ## EXAMPLES
///
/// ```xcl
/// if path.is_dir "/usr/local" {
///     io.print "It's a directory"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "path.is_dir", args = "path: str", ret = "bool")]
pub fn path_is_dir(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    Value::bool_(Path::new(path_str).is_dir())
}

/// Get the canonical (absolute) path of a path.
///
/// Resolves all symlinks and relative components.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.canonicalize($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to canonicalize.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.canonicalize "./file.txt"
/// if $result.is_ok {
///     io.print "Absolute path: ${$result.value}"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.canonicalize", args = "path: str", ret = "Result")]
pub fn path_canonicalize(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::canonicalize(path_str) {
        Ok(canonical) => {
            ok_result(Value::from_str_boxed(canonical.to_string_lossy().to_string()))
        }
        Err(e) => {
            err_result(universal::OTHER, &format!("path.canonicalize: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0))
        }
    }
}

/// Get the current working directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.cwd() -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.cwd
/// if $result.is_ok {
///     io.print "Current directory: ${$result.value}"
/// }
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.cwd", ret = "Result")]
pub fn path_cwd(_ctx: &mut DomainCtx) -> Value {
    match std::env::current_dir() {
        Ok(dir) => ok_result(Value::from_str_boxed(dir.to_string_lossy().to_string())),
        Err(e) => err_result(universal::OTHER, &format!("path.cwd: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Change the current working directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.chdir($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The directory to change to.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.chdir "/tmp"
/// if $result.is_ok {
///     io.print "Changed directory"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.chdir", args = "path: str", ret = "Result")]
pub fn path_chdir(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::env::set_current_dir(path_str) {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, &format!("path.chdir: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Get the home directory of the current user.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.home() -> str | nil
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $home = path.home
/// io.print "Home: $home"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str` or `nil`
#[xcl_fn(name = "path.home", ret = "str | nil")]
pub fn path_home(_ctx: &mut DomainCtx) -> Value {
    if let Ok(home) = std::env::var("HOME") {
        Value::from_str_boxed(home)
    } else if let Ok(home) = std::env::var("USERPROFILE") {
        Value::from_str_boxed(home)
    } else {
        Value::nil()
    }
}

/// Get the temporary directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.temp() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $temp = path.temp
/// io.print "Temp directory: $temp"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.temp", ret = "str")]
pub fn path_temp(_ctx: &mut DomainCtx) -> Value {
    let temp = std::env::temp_dir();
    Value::from_str_boxed(temp.to_string_lossy().to_string())
}

/// Get the path separator for the current platform.
///
/// Returns "/" on Unix-like systems and "\" on Windows.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.separator() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $sep = path.separator
/// io.print "Separator: $sep"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "path.separator", ret = "str")]
pub fn path_separator(_ctx: &mut DomainCtx) -> Value {
    let sep = std::path::MAIN_SEPARATOR;
    Value::from_str_boxed(sep.to_string())
}

/// Split a path into components.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.split($path: str) -> arr
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to split.
///
/// ## EXAMPLES
///
/// ```xcl
/// $parts = path.split "/usr/local/bin"
/// io.print $parts  # ["", "usr", "local", "bin"]
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `arr`
#[xcl_fn(name = "path.split", args = "path: str", ret = "arr")]
pub fn path_split(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    let sep = std::path::MAIN_SEPARATOR;
    let parts: Vec<Value> = path_str.split(sep)
        .map(|s| Value::from_str_boxed(s.to_string()))
        .collect();
    Value::from_array_boxed(parts)
}

/// Check if a path is absolute.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.is_absolute($path: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to check.
///
/// ## EXAMPLES
///
/// ```xcl
/// if path.is_absolute "/usr/local" {
///     io.print "Absolute path"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "path.is_absolute", args = "path: str", ret = "bool")]
pub fn path_is_absolute(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    Value::bool_(Path::new(path_str).is_absolute())
}

/// Get information about a file or directory.
///
/// Returns a map with metadata about the path.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.stat($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to stat.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.stat "/etc/hosts"
/// if $result.is_ok {
///     $stat = $result.value
///     io.print "Size: ${$stat.size}"
///     io.print "Is file: ${$stat.is_file}"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.stat", args = "path: str", ret = "Result")]
pub fn path_stat(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::metadata(path_str) {
        Ok(metadata) => {
            let mut map = HashMap::new();
            map.insert("size".to_string(), Value::from_i48(metadata.len() as i64));
            map.insert("is_file".to_string(), Value::bool_(metadata.is_file()));
            map.insert("is_dir".to_string(), Value::bool_(metadata.is_dir()));
            map.insert("is_symlink".to_string(), Value::bool_(metadata.file_type().is_symlink()));
            
            // Modified time
            if let Ok(modified) = metadata.modified()
                && let Ok(duration) = modified.duration_since(std::time::SystemTime::UNIX_EPOCH)
            {
                map.insert("modified".to_string(), Value::from_i48(duration.as_secs() as i64));
            }
            
            // Created time (not available on all platforms)
            if let Ok(created) = metadata.created()
                && let Ok(duration) = created.duration_since(std::time::SystemTime::UNIX_EPOCH)
            {
                map.insert("created".to_string(), Value::from_i48(duration.as_secs() as i64));
            }
            
            ok_result(Value::from_map_boxed(map))
        }
        Err(e) => err_result(universal::OTHER, &format!("path.stat: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// List the contents of a directory.
///
/// Returns an array of file/directory names in the directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.ls($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The directory to list.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.ls "/usr/local"
/// if $result.is_ok {
///     $files = $result.value
///     for $file in $files {
///         io.print $file
///     }
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.ls", args = "path: str", ret = "Result")]
pub fn path_ls(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::read_dir(path_str) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name();
                names.push(Value::from_str_boxed(name.to_string_lossy().to_string()));
            }
            ok_result(Value::from_array_boxed(names))
        }
        Err(e) => err_result(universal::OTHER, &format!("path.ls: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Create a directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.mkdir($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The directory path to create.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.mkdir "/tmp/mydir"
/// if $result.is_ok {
///     io.print "Directory created"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.mkdir", args = "path: str", ret = "Result")]
pub fn path_mkdir(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::create_dir(path_str) {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, &format!("path.mkdir: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Create a directory and all parent directories.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.mkdir_p($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The directory path to create.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.mkdir_p "/tmp/a/b/c"
/// if $result.is_ok {
///     io.print "Directories created"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.mkdir_p", args = "path: str", ret = "Result")]
pub fn path_mkdir_p(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::create_dir_all(path_str) {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, &format!("path.mkdir_p: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Remove a file or empty directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.remove($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to remove.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.remove "/tmp/file.txt"
/// if $result.is_ok {
///     io.print "Removed"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.remove", args = "path: str", ret = "Result")]
pub fn path_remove(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    let path = Path::new(path_str);
    
    let result = if path.is_dir() {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    };
    
    match result {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, &format!("path.remove: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Remove a file or directory recursively.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.remove_all($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to remove.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.remove_all "/tmp/mydir"
/// if $result.is_ok {
///     io.print "Removed recursively"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.remove_all", args = "path: str", ret = "Result")]
pub fn path_remove_all(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::remove_dir_all(path_str) {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, &format!("path.remove_all: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Copy a file.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.copy($source: str; $dest: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$source`: The source file path.
///
/// `$dest`: The destination file path.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.copy "/tmp/source.txt" "/tmp/dest.txt"
/// if $result.is_ok {
///     io.print "File copied"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.copy", args = "source: str; dest: str", ret = "Result")]
pub fn path_copy(ctx: &mut DomainCtx) -> Value {
    let source = ctx.arg(0).as_str().unwrap_or("");
    let dest = ctx.arg(1).as_str().unwrap_or("");
    match std::fs::copy(source, dest) {
        Ok(bytes) => ok_result(Value::from_i48(bytes as i64)),
        Err(e) => err_result(universal::OTHER, &format!("path.copy: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Rename or move a file/directory.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.rename($source: str; $dest: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$source`: The source path.
///
/// `$dest`: The destination path.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.rename "/tmp/old.txt" "/tmp/new.txt"
/// if $result.is_ok {
///     io.print "File renamed"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.rename", args = "source: str; dest: str", ret = "Result")]
pub fn path_rename(ctx: &mut DomainCtx) -> Value {
    let source = ctx.arg(0).as_str().unwrap_or("");
    let dest = ctx.arg(1).as_str().unwrap_or("");
    match std::fs::rename(source, dest) {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, &format!("path.rename: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Create a symbolic link.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.symlink($target: str; $link: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$target`: The target path the symlink points to.
///
/// `$link`: The path where the symlink will be created.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.symlink "/usr/local/bin/python3" "/usr/local/bin/python"
/// if $result.is_ok {
///     io.print "Symlink created"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.symlink", args = "target: str; link: str", ret = "Result")]
pub fn path_symlink(ctx: &mut DomainCtx) -> Value {
    let target = ctx.arg(0).as_str().unwrap_or("");
    let link = ctx.arg(1).as_str().unwrap_or("");
    
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(target, link) {
            Ok(_) => ok_result(Value::unit()),
            Err(e) => err_result(universal::OTHER, &format!("path.symlink: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
        }
    }
    
    #[cfg(windows)]
    {
        // On Windows, we need to know if it's a file or directory symlink
        // Default to file symlink
        match std::os::windows::fs::symlink_file(target, link) {
            Ok(_) => ok_result(Value::unit()),
            Err(e) => err_result(universal::OTHER, &format!("path.symlink: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
        }
    }
    
    #[cfg(not(any(unix, windows)))]
    {
        err_result(universal::OTHER, "path.symlink: not supported on this platform", 0)
    }
}

/// Read the target of a symbolic link.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.readlink($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The symbolic link path.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = path.readlink "/usr/local/bin/python"
/// if $result.is_ok {
///     io.print "Points to: ${$result.value}"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "path.readlink", args = "path: str", ret = "Result")]
pub fn path_readlink(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    match std::fs::read_link(path_str) {
        Ok(target) => ok_result(Value::from_str_boxed(target.to_string_lossy().to_string())),
        Err(e) => err_result(universal::OTHER, &format!("path.readlink: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Check if a path is a symbolic link.
///
/// ## SYNTAX
///
/// ```xcl
/// fn path.is_symlink($path: str) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The path to check.
///
/// ## EXAMPLES
///
/// ```xcl
/// if path.is_symlink "/usr/local/bin/python" {
///     io.print "It's a symlink"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `bool`
#[xcl_fn(name = "path.is_symlink", args = "path: str", ret = "bool")]
pub fn path_is_symlink(ctx: &mut DomainCtx) -> Value {
    let path_str = ctx.arg(0).as_str().unwrap_or("");
    Value::bool_(std::fs::symlink_metadata(path_str)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false))
}

/// Get the system's host name.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.hostname() -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = os.hostname
/// if $result.is_ok {
///     io.print "Hostname: ${$result.value}"
/// }
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "os.hostname", ret = "Result")]
pub fn os_hostname(_ctx: &mut DomainCtx) -> Value {
    match std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::env::var("HOST"))
    {
        Ok(hostname) => ok_result(Value::from_str_boxed(hostname)),
        Err(_) => {
            // Try to get hostname from /etc/hostname on Unix systems
            #[cfg(unix)]
            {
                if let Ok(content) = std::fs::read_to_string("/etc/hostname") {
                    let hostname = content.trim().to_string();
                    if !hostname.is_empty() {
                        return ok_result(Value::from_str_boxed(hostname));
                    }
                }
            }
            err_result(universal::OTHER, "os.hostname: could not determine hostname", 0)
        }
    }
}

/// Get the number of CPU cores available.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.cpus() -> int
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $cpus = os.cpus
/// io.print "CPU cores: $cpus"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `int`
#[xcl_fn(name = "os.cpus", ret = "int")]
pub fn os_cpus(_ctx: &mut DomainCtx) -> Value {
    let ncpus = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);
    Value::from_i48(ncpus as i64)
}

/// Get the amount of free memory in bytes.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.free_memory() -> int
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $free = os.free_memory
/// io.print "Free memory: $free bytes"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `int`
#[xcl_fn(name = "os.free_memory", ret = "int")]
pub fn os_free_memory(_ctx: &mut DomainCtx) -> Value {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemAvailable:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<u64>() {
                            return Value::from_i48((kb * 1024) as i64);
                        }
                    }
                }
            }
        }
        Value::from_i48(0)
    }
    
    #[cfg(target_os = "macos")]
    {
        // Use sysctl to get memory info
        // For simplicity, return 0 as a placeholder
        Value::from_i48(0)
    }
    
    #[cfg(target_os = "windows")]
    {
        // Use GlobalMemoryStatusEx
        // For simplicity, return 0 as a placeholder
        Value::from_i48(0)
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Value::from_i48(0)
    }
}

/// Get the total memory in bytes.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.total_memory() -> int
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $total = os.total_memory
/// io.print "Total memory: $total bytes"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `int`
#[xcl_fn(name = "os.total_memory", ret = "int")]
pub fn os_total_memory(_ctx: &mut DomainCtx) -> Value {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<u64>() {
                            return Value::from_i48((kb * 1024) as i64);
                        }
                    }
                }
            }
        }
        Value::from_i48(0)
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        Value::from_i48(0)
    }
}

/// Get the current user's name.
///
/// ## SYNTAX
///
/// ```xcl
/// fn os.user() -> str | nil
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $user = os.user
/// io.print "Current user: $user"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str` or `nil`
#[xcl_fn(name = "os.user", ret = "str | nil")]
pub fn os_user(_ctx: &mut DomainCtx) -> Value {
    if let Ok(user) = std::env::var("USER") {
        Value::from_str_boxed(user)
    } else if let Ok(user) = std::env::var("USERNAME") {
        Value::from_str_boxed(user)
    } else {
        Value::nil()
    }
}

/// Get the current time as a Unix timestamp (seconds since epoch).
///
/// ## SYNTAX
///
/// ```xcl
/// fn time.now() -> int
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $now = time.now
/// io.print "Current time: $now"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `int`
#[xcl_fn(name = "time.now", ret = "int")]
pub fn time_now(_ctx: &mut DomainCtx) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Value::from_i48(now as i64)
}

/// Get the current time as a Unix timestamp in milliseconds.
///
/// ## SYNTAX
///
/// ```xcl
/// fn time.now_ms() -> int
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $now = time.now_ms
/// io.print "Current time (ms): $now"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `int`
#[xcl_fn(name = "time.now_ms", ret = "int")]
pub fn time_now_ms(_ctx: &mut DomainCtx) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Value::from_i48(now as i64)
}

/// Sleep for a specified number of milliseconds.
///
/// ## SYNTAX
///
/// ```xcl
/// fn time.sleep($milliseconds: int) -> nil
/// ```
///
/// ## PARAMETERS
///
/// `$milliseconds`: The number of milliseconds to sleep.
///
/// ## EXAMPLES
///
/// ```xcl
/// time.sleep 1000  # Sleep for 1 second
/// ```
///
/// ## INPUT TYPES
///
/// - `int`
#[xcl_fn(name = "time.sleep", args = "milliseconds: int", ret = "nil")]
pub fn time_sleep(ctx: &mut DomainCtx) -> Value {
    let ms = ctx.arg(0).as_i48() as u64;
    std::thread::sleep(std::time::Duration::from_millis(ms));
    Value::unit()
}

/// Generate a random UUID v4.
///
/// ## SYNTAX
///
/// ```xcl
/// fn util.uuid() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $id = util.uuid
/// io.print "UUID: $id"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
#[xcl_fn(name = "util.uuid", ret = "str")]
pub fn util_uuid(_ctx: &mut DomainCtx) -> Value {
    let mut bytes = [0u8; 16];
    for b in &mut bytes {
        *b = rand::random();
    }
    
    // Set version 4
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // Set variant
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    
    let uuid = format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        u64::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], 0, 0])
    );
    
    Value::from_str_boxed(uuid)
}

/// Execute a shell command.
///
/// Runs a command through the system shell and returns the output.
///
/// ## SYNTAX
///
/// ```xcl
/// fn util.exec($command: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$command`: The shell command to execute.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = util.exec "ls -la /tmp"
/// if $result.is_ok {
///     io.print $result.value
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "util.exec", args = "command: str", ret = "Result")]
pub fn util_exec(ctx: &mut DomainCtx) -> Value {
    let command = ctx.arg(0).as_str().unwrap_or("");
    
    let (shell, shell_arg) = if cfg!(windows) {
        ("cmd.exe", "/C")
    } else {
        ("/bin/sh", "-c")
    };
    
    match std::process::Command::new(shell)
        .arg(shell_arg)
        .arg(command)
        .output()
    {
        Ok(output) => {
            let mut result = HashMap::new();
            result.insert("exit_code".to_string(), Value::from_i48(output.status.code().unwrap_or(-1) as i64));
            result.insert("stdout".to_string(), Value::from_str_boxed(String::from_utf8_lossy(&output.stdout).to_string()));
            result.insert("stderr".to_string(), Value::from_str_boxed(String::from_utf8_lossy(&output.stderr).to_string()));
            ok_result(Value::from_map_boxed(result))
        }
        Err(e) => err_result(universal::OTHER, &format!("util.exec: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}
