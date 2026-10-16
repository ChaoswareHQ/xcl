//! Compiler detection for C backends.
//!
//! Detects installed C compilers (gcc, clang, msvc) and their supported
//! C standard versions. This allows the translator to verify that the
//! target C version is actually available on the system.
//!
//! # Usage
//!
//! ```ignore
//! use xcl_plugins::translate::compiler::{detect_c_compilers, CompilerInfo};
//!
//! let compilers = detect_c_compilers();
//! for compiler in compilers {
//!     println!("{} supports C{} through C{}",
//!         compiler.name, compiler.min_version, compiler.max_version);
//! }
//! ```

use std::process::Command;

/// Detected C compiler information.
#[derive(Clone, Debug)]
pub struct CompilerInfo {
    /// Compiler name (gcc, clang, cl).
    pub name: String,
    /// Compiler version string.
    pub version: String,
    /// Path to the compiler executable.
    pub path: String,
    /// Minimum supported C standard version.
    pub min_version: String,
    /// Maximum supported C standard version.
    pub max_version: String,
    /// Supported C standard versions.
    pub supported_versions: Vec<String>,
}

impl CompilerInfo {
    /// Check if this compiler supports a given C version.
    pub fn supports_version(&self, version: &str) -> bool {
        self.supported_versions.contains(&version.to_string())
    }

    /// Get the compiler flag for a given C version.
    pub fn flag_for_version(&self, version: &str) -> Option<String> {
        if self.supports_version(version) {
            match self.name.as_str() {
                "gcc" | "clang" => Some(format!("-std=c{}", version)),
                "cl" => Some(format!("/std:c{}", version)),
                _ => None,
            }
        } else {
            None
        }
    }
}

/// Detect installed C compilers.
///
/// Searches for gcc, clang, and MSVC (cl.exe) on the system
/// and returns information about each found compiler.
pub fn detect_c_compilers() -> Vec<CompilerInfo> {
    let mut compilers = Vec::new();

    // Detect GCC
    if let Some(gcc) = detect_gcc() {
        compilers.push(gcc);
    }

    // Detect Clang
    if let Some(clang) = detect_clang() {
        compilers.push(clang);
    }

    // Detect MSVC (Windows only)
    #[cfg(windows)]
    if let Some(cl) = detect_msvc() {
        compilers.push(cl);
    }

    compilers
}

/// Detect GCC compiler.
fn detect_gcc() -> Option<CompilerInfo> {
    let output = Command::new("gcc")
        .arg("--version")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    let version = parse_gcc_version(&version_output)?;

    Some(CompilerInfo {
        name: "gcc".to_string(),
        version: version.clone(),
        path: "gcc".to_string(),
        min_version: "99".to_string(),
        max_version: if version.as_str() >= "13.0" {
            "23".to_string()
        } else if version.as_str() >= "11.0" {
            "17".to_string()
        } else if version.as_str() >= "5.0" {
            "11".to_string()
        } else {
            "99".to_string()
        },
        supported_versions: get_gcc_supported_versions(&version),
    })
}

/// Detect Clang compiler.
fn detect_clang() -> Option<CompilerInfo> {
    let output = Command::new("clang")
        .arg("--version")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    let version = parse_clang_version(&version_output)?;

    Some(CompilerInfo {
        name: "clang".to_string(),
        version: version.clone(),
        path: "clang".to_string(),
        min_version: "99".to_string(),
        max_version: if version.as_str() >= "17.0" {
            "23".to_string()
        } else if version.as_str() >= "11.0" {
            "17".to_string()
        } else if version.as_str() >= "3.3" {
            "11".to_string()
        } else {
            "99".to_string()
        },
        supported_versions: get_clang_supported_versions(&version),
    })
}

/// Detect MSVC compiler (Windows only).
#[cfg(windows)]
fn detect_msvc() -> Option<CompilerInfo> {
    let output = Command::new("cl")
        .arg("/")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    let version = parse_msvc_version(&version_output)?;

    Some(CompilerInfo {
        name: "cl".to_string(),
        version: version.clone(),
        path: "cl".to_string(),
        min_version: "11".to_string(),
        max_version: if version.as_str() >= "19.30" {
            "23".to_string()
        } else if version.as_str() >= "19.10" {
            "17".to_string()
        } else {
            "11".to_string()
        },
        supported_versions: get_msvc_supported_versions(&version),
    })
}

/// Parse GCC version from --version output.
fn parse_gcc_version(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("gcc") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for part in &parts {
                if part.matches('.').count() >= 1 && part.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return Some(part.to_string());
                }
            }
        }
    }
    None
}

/// Parse Clang version from --version output.
fn parse_clang_version(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("clang") || line.contains("version") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for part in &parts {
                if part.matches('.').count() >= 1 && part.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return Some(part.to_string());
                }
            }
        }
    }
    None
}

/// Parse MSVC version from cl / output.
#[cfg(windows)]
fn parse_msvc_version(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("version") || line.contains("Version") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for part in &parts {
                if part.matches('.').count() >= 1 && part.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return Some(part.to_string());
                }
            }
        }
    }
    None
}

/// Get GCC supported C versions based on version number.
fn get_gcc_supported_versions(version: &str) -> Vec<String> {
    let mut versions = vec!["99".to_string()];
    if version >= "3.3" {
        versions.push("11".to_string());
    }
    if version >= "5.0" {
        versions.push("17".to_string());
    }
    if version >= "13.0" {
        versions.push("23".to_string());
    }
    versions
}

/// Get Clang supported C versions based on version number.
fn get_clang_supported_versions(version: &str) -> Vec<String> {
    let mut versions = vec!["99".to_string()];
    if version >= "3.3" {
        versions.push("11".to_string());
    }
    if version >= "11.0" {
        versions.push("17".to_string());
    }
    if version >= "17.0" {
        versions.push("23".to_string());
    }
    versions
}

/// Get MSVC supported C versions based on version number.
#[cfg(windows)]
fn get_msvc_supported_versions(version: &str) -> Vec<String> {
    let mut versions = vec!["11".to_string()];
    if version >= "19.10" {
        versions.push("17".to_string());
    }
    if version >= "19.30" {
        versions.push("23".to_string());
    }
    versions
}

/// Find the best available C version for the target.
///
/// Returns the highest C version supported by both the target
/// specification and the installed compilers.
pub fn find_best_c_version(target_version: &str) -> Option<String> {
    let compilers = detect_c_compilers();
    if compilers.is_empty() {
        return Some(target_version.to_string()); // Default to target if no compiler found
    }

    // Check if any compiler supports the target version
    for compiler in &compilers {
        if compiler.supports_version(target_version) {
            return Some(target_version.to_string());
        }
    }

    // Find the highest supported version
    let mut best = None;
    let version_order = ["99", "11", "17", "23"];

    for version in &version_order {
        if *version <= target_version {
            for compiler in &compilers {
                if compiler.supports_version(version) {
                    best = Some(version.to_string());
                    break;
                }
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_info_supports_version() {
        let compiler = CompilerInfo {
            name: "gcc".to_string(),
            version: "13.0".to_string(),
            path: "gcc".to_string(),
            min_version: "99".to_string(),
            max_version: "23".to_string(),
            supported_versions: vec!["99".to_string(), "11".to_string(), "17".to_string(), "23".to_string()],
        };

        assert!(compiler.supports_version("99"));
        assert!(compiler.supports_version("23"));
        assert!(!compiler.supports_version("95"));
    }

    #[test]
    fn test_compiler_info_flag_for_version() {
        let compiler = CompilerInfo {
            name: "gcc".to_string(),
            version: "13.0".to_string(),
            path: "gcc".to_string(),
            min_version: "99".to_string(),
            max_version: "23".to_string(),
            supported_versions: vec!["99".to_string(), "23".to_string()],
        };

        assert_eq!(compiler.flag_for_version("99"), Some("-std=c99".to_string()));
        assert_eq!(compiler.flag_for_version("23"), Some("-std=c23".to_string()));
        assert_eq!(compiler.flag_for_version("95"), None);
    }

    #[test]
    fn test_detect_c_compilers_returns_vec() {
        let _compilers = detect_c_compilers();
        // May be empty if no compiler installed — just verify it runs without panicking
    }
}
