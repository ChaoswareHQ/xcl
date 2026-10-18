//! Filesystem module — File and directory operations for XCL.
//!
//! Provides file read/write, directory listing, path manipulation, and metadata.
//!
//! # Usage in XCL
//!
//! ```xcl
//! # Read entire file
//! $content = fs.read "config.json"
//!
//! # Write to file
//! fs.write "output.txt" "Hello, world!"
//!
//! # Check if path exists
//! $exists = fs.exists "config.json"
//!
//! # List directory contents
//! $files = fs.list "src/"
//!
//! # Get file metadata
//! $meta = fs.metadata "file.txt"
//!
//! # Create directory
//! fs.mkdir "new_dir"
//!
//! # Delete file
//! fs.remove "old_file.txt"
//!
//! # Move file
//! fs.move_ "old.txt" "new.txt"
//!
//! # Get current working directory
//! $cwd = fs.cwd
//!
//! # Get temp directory
//! $tmp = fs.temp_dir
//!
//! # Join paths
//! $path = fs.join ["src", "main.xcl"]
//!
//! # Get file extension
//! $ext = fs.extension "file.txt"
//! ```

use std::collections::HashMap;
use xcl_vm::Value;
use xcl_macros::{Xcl, xcl_impl};

/// Filesystem operations type.
#[derive(Xcl)]
pub struct Fs;

#[xcl_impl(auto_import)]
impl Fs {
    /// Read entire file content as a string.
    pub fn read(path: String) -> String {
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    /// Write content to a file, creating or overwriting it.
    pub fn write(path: String, content: String) -> bool {
        std::fs::write(&path, &content).is_ok()
    }

    /// Append content to a file, creating it if it doesn't exist.
    pub fn append(path: String, content: String) -> bool {
        match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(content.as_bytes()).is_ok()
            }
            Err(_) => false,
        }
    }

    /// Check if a path exists.
    pub fn exists(path: String) -> bool {
        std::path::Path::new(&path).exists()
    }

    /// Check if a path is a file.
    pub fn is_file(path: String) -> bool {
        std::path::Path::new(&path).is_file()
    }

    /// Check if a path is a directory.
    pub fn is_dir(path: String) -> bool {
        std::path::Path::new(&path).is_dir()
    }

    /// List directory contents, returning an array of entry names.
    pub fn list(path: String) -> Vec<Value> {
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                let mut names = Vec::new();
                for entry in entries {
                    if let Ok(e) = entry {
                        let name = e.file_name().to_string_lossy().to_string();
                        names.push(Value::from_str_boxed(name));
                    }
                }
                names
            }
            Err(_) => Vec::new(),
        }
    }

    /// Create a directory (and parents if recursive is true).
    pub fn mkdir(path: String, recursive: bool) -> bool {
        if recursive {
            std::fs::create_dir_all(&path).is_ok()
        } else {
            std::fs::create_dir(&path).is_ok()
        }
    }

    /// Get file metadata as a map with size, modified, is_file, is_dir.
    pub fn metadata(path: String) -> Value {
        match std::fs::metadata(&path) {
            Ok(meta) => {
                let mut map = HashMap::new();
                map.insert("size".to_string(), Value::from_i48(meta.len() as i64));
                map.insert("is_file".to_string(), Value::bool(meta.is_file()));
                map.insert("is_dir".to_string(), Value::bool(meta.is_dir()));
                if let Ok(modified) = meta.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        map.insert("modified".to_string(), Value::from_i48(duration.as_secs() as i64));
                    }
                }
                if let Ok(created) = meta.created() {
                    if let Ok(duration) = created.duration_since(std::time::UNIX_EPOCH) {
                        map.insert("created".to_string(), Value::from_i48(duration.as_secs() as i64));
                    }
                }
                Value::from_map_boxed(map)
            }
            Err(_) => Value::from_map_boxed(HashMap::new()),
        }
    }

    /// Delete a file.
    pub fn remove(path: String) -> bool {
        std::fs::remove_file(&path).is_ok()
    }

    /// Remove a directory (use recursive to remove non-empty dirs).
    pub fn remove_dir(path: String, recursive: bool) -> bool {
        if recursive {
            std::fs::remove_dir_all(&path).is_ok()
        } else {
            std::fs::remove_dir(&path).is_ok()
        }
    }

    /// Copy a file from source to destination.
    pub fn copy(src: String, dst: String) -> bool {
        std::fs::copy(&src, &dst).is_ok()
    }

    /// Move or rename a file.
    pub fn move_(src: String, dst: String) -> bool {
        std::fs::rename(&src, &dst).is_ok()
    }

    /// Get the current working directory.
    pub fn cwd() -> String {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Change the current working directory.
    pub fn chdir(path: String) -> bool {
        std::env::set_current_dir(&path).is_ok()
    }

    /// Get the system temp directory.
    pub fn temp_dir() -> String {
        std::env::temp_dir().to_string_lossy().to_string()
    }

    /// Join path components together.
    pub fn join(parts: Vec<Value>) -> String {
        let mut path = std::path::PathBuf::new();
        for part in parts {
            if let Some(s) = part.as_str() {
                path.push(s);
            }
        }
        path.to_string_lossy().to_string()
    }

    /// Get the file extension.
    pub fn extension(path: String) -> String {
        std::path::Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Get the file name without extension.
    pub fn basename(path: String) -> String {
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Get the directory portion of a path.
    pub fn dirname(path: String) -> String {
        std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Get the absolute path.
    pub fn absolute(path: String) -> String {
        match std::fs::canonicalize(&path) {
            Ok(abs) => abs.to_string_lossy().to_string(),
            Err(_) => path,
        }
    }

    /// Read file as raw bytes, returned as array of integers.
    pub fn read_bytes(path: String) -> Vec<Value> {
        match std::fs::read(&path) {
            Ok(bytes) => {
                bytes.iter().map(|b| Value::from_i48(*b as i64)).collect()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Write raw bytes to a file.
    pub fn write_bytes(path: String, bytes: Vec<Value>) -> bool {
        let byte_vec: Vec<u8> = bytes.iter()
            .map(|v| v.as_i48() as u8)
            .collect();
        std::fs::write(&path, &byte_vec).is_ok()
    }
}
