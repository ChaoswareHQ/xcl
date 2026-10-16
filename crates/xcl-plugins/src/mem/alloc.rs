//! Global allocator interface for xcl-plugins.
//!
//! Provides memory allocation and deallocation functions.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Allocate memory from the global allocator.
///
/// # Arguments
/// - `size` (int): Number of bytes to allocate
///
/// # Returns
/// Integer memory address (pointer), or 0 on failure
///
/// # Example
/// ```xcl
/// ptr = mem.alloc(256)
/// ```
#[xcl_fn("mem.alloc", signature = "(size: int) -> int")]
pub fn alloc(size: i64) -> Value {
    let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap_or_else(|_| {
        std::alloc::Layout::from_size_align(0, 8).unwrap()
    });
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        Value::from_i48(0)
    } else {
        Value::from_i48(ptr as i64)
    }
}

/// Allocate zeroed memory from the global allocator.
///
/// # Arguments
/// - `size` (int): Number of bytes to allocate
///
/// # Returns
/// Integer memory address (pointer), or 0 on failure
///
/// # Example
/// ```xcl
/// ptr = mem.alloc_zero(256)
/// ```
#[xcl_fn("mem.alloc_zero", signature = "(size: int) -> int")]
pub fn alloc_zero(size: i64) -> Value {
    let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap_or_else(|_| {
        std::alloc::Layout::from_size_align(0, 8).unwrap()
    });
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        Value::from_i48(0)
    } else {
        Value::from_i48(ptr as i64)
    }
}

/// Reallocate memory to a new size.
///
/// # Arguments
/// - `ptr` (int): Current memory address
/// - `new_size` (int): New size in bytes
///
/// # Returns
/// Integer new memory address, or 0 on failure
///
/// # Example
/// ```xcl
/// ptr = mem.realloc(ptr, 512)
/// ```
#[xcl_fn("mem.realloc", signature = "(ptr: int, new_size: int) -> int")]
pub fn realloc(ptr: i64, new_size: i64) -> Value {
    let layout = std::alloc::Layout::from_size_align(new_size as usize, 8).unwrap_or_else(|_| {
        std::alloc::Layout::from_size_align(0, 8).unwrap()
    });
    let new_ptr = unsafe { std::alloc::realloc(ptr as *mut u8, layout, new_size as usize) };
    if new_ptr.is_null() {
        Value::from_i48(0)
    } else {
        Value::from_i48(new_ptr as i64)
    }
}

/// Deallocate memory previously allocated by mem.alloc.
///
/// # Arguments
/// - `ptr` (int): Memory address to deallocate
/// - `size` (int): Size of the allocation (for proper deallocation)
///
/// # Example
/// ```xcl
/// mem.dealloc(ptr, 256)
/// ```
#[xcl_fn("mem.dealloc", signature = "(ptr: int, size: int) -> nil")]
pub fn dealloc(ptr: i64, size: i64) -> Value {
    if ptr != 0 {
        let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap_or_else(|_| {
            std::alloc::Layout::from_size_align(0, 8).unwrap()
        });
        unsafe { std::alloc::dealloc(ptr as *mut u8, layout) };
    }
    Value::nil()
}

/// Get allocator statistics.
///
/// # Returns
/// Map containing allocator statistics
///
/// # Example
/// ```xcl
/// mem.stats()
/// ```
#[xcl_fn("mem.stats", signature = "() -> map")]
pub fn stats() -> Value {
    let mut info = HashMap::new();
    info.insert("pointer_size".to_string(), Value::from_i48(std::mem::size_of::<usize>() as i64));
    info.insert("page_size".to_string(), Value::from_i48(page_size()));
    Value::from_map(info)
}

/// Get the system page size.
///
/// # Returns
/// Integer page size in bytes
///
/// # Example
/// ```xcl
/// mem.page_size()
/// ```
#[xcl_fn("mem.page_size", signature = "() -> int")]
pub fn page_size() -> i64 {
    #[cfg(unix)]
    {
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 }
    }
    #[cfg(windows)]
    {
        let mut info: windows::Win32::System::SystemInformation::SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { windows::Win32::System::SystemInformation::GetSystemInfo(&mut info) };
        info.dwPageSize as i64
    }
    #[cfg(not(any(unix, windows)))]
    {
        4096
    }
}

/// Check if a pointer is aligned.
///
/// # Arguments
/// - `ptr` (int): Memory address
/// - `alignment` (int): Required alignment
///
/// # Returns
/// Boolean indicating if the pointer is aligned
///
/// # Example
/// ```xcl
/// mem.is_aligned(ptr, 16)
/// ```
#[xcl_fn("mem.is_aligned", signature = "(ptr: int, alignment: int) -> bool")]
pub fn is_aligned(ptr: i64, alignment: i64) -> Value {
    Value::bool_(ptr % alignment == 0)
}
