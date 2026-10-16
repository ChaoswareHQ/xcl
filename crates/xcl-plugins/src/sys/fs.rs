//! File descriptor and memory-mapped file operations for xcl-plugins.
//!
//! Provides low-level file descriptor operations and memory-mapped files.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Open a file descriptor with the given flags.
///
/// # Arguments
/// - `path` (str): Path to the file
/// - `flags` (int): Open flags (O_RDONLY=0, O_WRONLY=1, O_RDWR=2, O_CREAT=64, O_TRUNC=512)
///
/// # Returns
/// Integer file descriptor number
///
/// # Example
/// ```xcl
/// fd = sys.fd_open("/tmp/test.txt", 64)  // O_CREAT
/// ```
#[xcl_fn("sys.fd_open", signature = "(path: str, flags: int) -> result")]
pub fn fd_open(path: &str, flags: i64) -> Value {
    #[cfg(unix)]
    {
        let fd = unsafe { libc::open(path.as_ptr() as *const i8, flags as i32, 0o644) };
        if fd < 0 {
            Value::from_str_boxed(format!("Failed to open file: {}", path))
        } else {
            Value::from_i48(fd)
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("File descriptors are not available on this platform".to_string())
    }
}

/// Close a file descriptor.
///
/// # Arguments
/// - `fd` (int): File descriptor to close
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.fd_close(3)
/// ```
#[xcl_fn("sys.fd_close", signature = "(fd: int) -> result")]
pub fn fd_close(fd: i64) -> Value {
    #[cfg(unix)]
    {
        let result = unsafe { libc::close(fd as i32) };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to close file descriptor".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("File descriptors are not available on this platform".to_string())
    }
}

/// Read from a file descriptor.
///
/// # Arguments
/// - `fd` (int): File descriptor to read from
/// - `size` (int): Number of bytes to read
///
/// # Returns
/// String containing the bytes read
///
/// # Example
/// ```xcl
/// data = sys.fd_read(0, 1024)  // Read 1KB from stdin
/// ```
#[xcl_fn("sys.fd_read", signature = "(fd: int, size: int) -> result")]
pub fn fd_read(fd: i64, size: i64) -> Value {
    #[cfg(unix)]
    {
        let mut buf = vec![0u8; size as usize];
        let bytes_read = unsafe { libc::read(fd as i32, buf.as_mut_ptr() as *mut libc::c_void, size as usize) };
        if bytes_read < 0 {
            Value::from_str_boxed("Failed to read from file descriptor".to_string())
        } else {
            buf.truncate(bytes_read as usize);
            Value::from_str_boxed(String::from_utf8_lossy(&buf).to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("File descriptors are not available on this platform".to_string())
    }
}

/// Write to a file descriptor.
///
/// # Arguments
/// - `fd` (int): File descriptor to write to
/// - `data` (str): Data to write
///
/// # Returns
/// Integer number of bytes written
///
/// # Example
/// ```xcl
/// sys.fd_write(1, "Hello, World!")  // Write to stdout
/// ```
#[xcl_fn("sys.fd_write", signature = "(fd: int, data: str) -> result")]
pub fn fd_write(fd: i64, data: &str) -> Value {
    #[cfg(unix)]
    {
        let bytes_written = unsafe {
            libc::write(
                fd as i32,
                data.as_ptr() as *const libc::c_void,
                data.len() as usize,
            )
        };
        if bytes_written < 0 {
            Value::from_str_boxed("Failed to write to file descriptor".to_string())
        } else {
            Value::from_i48(bytes_written)
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("File descriptors are not available on this platform".to_string())
    }
}

/// Memory-map a file into memory.
///
/// # Arguments
/// - `path` (str): Path to the file
/// - `offset` (int): Byte offset in the file
/// - `length` (int): Number of bytes to map
///
/// # Returns
/// String containing the mapped file contents
///
/// # Example
/// ```xcl
/// data = sys.mmap("/tmp/large_file.bin", 0, 1024)
/// ```
#[xcl_fn("sys.mmap", signature = "(path: str, offset: int, length: int) -> result")]
pub fn mmap(path: &str, offset: i64, length: i64) -> Value {
    #[cfg(unix)]
    {
        let fd = unsafe { libc::open(path.as_ptr() as *const i8, libc::O_RDONLY) };
        if fd < 0 {
            return Value::from_str_boxed(format!("Failed to open file for mmap: {}", path));
        }

        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length as usize,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd,
                offset,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Value::from_str_boxed("Failed to mmap file".to_string());
        }

        let data = unsafe {
            std::slice::from_raw_parts(ptr as *const u8, length as usize)
        };

        let result = String::from_utf8_lossy(data).to_string();
        unsafe { libc::munmap(ptr, length as usize) };
        unsafe { libc::close(fd) };
        Value::from_str_boxed(result)
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("mmap is not available on this platform".to_string())
    }
}

/// Duplicate a file descriptor.
///
/// # Arguments
/// - `fd` (int): File descriptor to duplicate
///
/// # Returns
/// Integer new file descriptor
///
/// # Example
/// ```xcl
/// new_fd = sys.fd_dup(0)  // Duplicate stdin
/// ```
#[xcl_fn("sys.fd_dup", signature = "(fd: int) -> result")]
pub fn fd_dup(fd: i64) -> Value {
    #[cfg(unix)]
    {
        let new_fd = unsafe { libc::dup(fd as i32) };
        if new_fd < 0 {
            Value::from_str_boxed("Failed to duplicate file descriptor".to_string())
        } else {
            Value::from_i48(new_fd)
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("File descriptors are not available on this platform".to_string())
    }
}

/// Get file status information via file descriptor.
///
/// # Arguments
/// - `fd` (int): File descriptor
///
/// # Returns
/// Map containing file information (size, permissions, etc.)
///
/// # Example
/// ```xcl
/// sys.fd_stat(0)
/// ```
#[xcl_fn("sys.fd_stat", signature = "(fd: int) -> result")]
pub fn fd_stat(fd: i64) -> Value {
    #[cfg(unix)]
    {
        let mut stat = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::fstat(fd as i32, &mut stat) };
        if result == 0 {
            let mut info = std::collections::HashMap::new();
            info.insert("size".to_string(), Value::from_i48(stat.st_size as i64));
            info.insert("mode".to_string(), Value::from_i48(stat.st_mode as i64));
            info.insert("uid".to_string(), Value::from_i48(stat.st_uid as i64));
            info.insert("gid".to_string(), Value::from_i48(stat.st_gid as i64));
            info.insert("inode".to_string(), Value::from_i48(stat.st_ino as i64));
            Value::from_map(info)
        } else {
            Value::from_str_boxed("Failed to get file status".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("File descriptors are not available on this platform".to_string())
    }
}
