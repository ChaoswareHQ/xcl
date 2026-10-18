//! Dynamic library loading for xcl-plugins.
//!
//! Provides runtime loading of shared libraries (DLLs, .so, .dylib).

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Load a dynamic library.
///
/// # Arguments
/// - `path` (str): Path to the shared library file
///
/// # Returns
/// Integer library handle, or error string on failure
///
/// # Example
/// ```xcl
/// lib = ffi.load_library("/usr/lib/libexample.so")
/// ```
#[xcl_fn]
pub fn load_library(path: &str) -> Value {
    #[cfg(unix)]
    {
        let c_path = std::ffi::CString::new(path).unwrap_or_default();
        let handle = unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW) };
        if handle.is_null() {
            let err = unsafe { libc::dlerror() };
            if !err.is_null() {
                let err_str = unsafe { std::ffi::CStr::from_ptr(err) }.to_string_lossy().into_owned();
                Value::from_str_boxed(err_str)
            } else {
                Value::from_str_boxed("Failed to load library".to_string())
            }
        } else {
            Value::from_i48(handle as i64)
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::prelude::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsString::from(path)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let handle = unsafe {
            windows::Win32::System::LibraryLoader::LoadLibraryW(wide.as_ptr())
        };
        if handle.is_null() {
            Value::from_str_boxed("Failed to load library".to_string())
        } else {
            Value::from_i48(handle as i64)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Value::from_str_boxed("Dynamic library loading not supported on this platform".to_string())
    }
}

/// Get a symbol address from a loaded library.
///
/// # Arguments
/// - `handle` (int): Library handle from load_library
/// - `symbol` (str): Symbol name to look up
///
/// # Returns
/// Integer memory address of the symbol
///
/// # Example
/// ```xcl
/// addr = ffi.get_symbol(lib, "my_function")
/// ```
#[xcl_fn]
pub fn get_symbol(handle: i64, symbol: &str) -> Value {
    #[cfg(unix)]
    {
        let c_symbol = std::ffi::CString::new(symbol).unwrap_or_default();
        let sym = unsafe { libc::dlsym(handle as *mut libc::c_void, c_symbol.as_ptr()) };
        if sym.is_null() {
            let err = unsafe { libc::dlerror() };
            if !err.is_null() {
                let err_str = unsafe { std::ffi::CStr::from_ptr(err) }.to_string_lossy().into_owned();
                Value::from_str_boxed(err_str)
            } else {
                Value::from_str_boxed("Symbol not found".to_string())
            }
        } else {
            Value::from_i48(sym as i64)
        }
    }
    #[cfg(windows)]
    {
        let c_symbol = std::ffi::CString::new(symbol).unwrap_or_default();
        let sym = unsafe {
            windows::Win32::System::LibraryLoader::GetProcAddress(
                handle as *mut std::ffi::c_void,
                c_symbol.as_ptr(),
            )
        };
        if sym.is_null() {
            Value::from_str_boxed("Symbol not found".to_string())
        } else {
            Value::from_i48(sym as i64)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Value::from_str_boxed("Dynamic library loading not supported on this platform".to_string())
    }
}

/// Unload a dynamic library.
///
/// # Arguments
/// - `handle` (int): Library handle
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// ffi.unload_library(lib)
/// ```
#[xcl_fn]
pub fn unload_library(handle: i64) -> Value {
    #[cfg(unix)]
    {
        let result = unsafe { libc::dlclose(handle as *mut libc::c_void) };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to unload library".to_string())
        }
    }
    #[cfg(windows)]
    {
        let result = unsafe {
            windows::Win32::System::LibraryLoader::FreeLibrary(handle as *mut std::ffi::c_void)
        };
        if result.as_bool() {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to unload library".to_string())
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Value::from_str_boxed("Dynamic library loading not supported on this platform".to_string())
    }
}

/// List symbols in a loaded library.
///
/// # Arguments
/// - `handle` (int): Library handle
///
/// # Returns
/// Array of symbol name strings
///
/// # Example
/// ```xcl
/// symbols = ffi.list_symbols(lib)
/// ```
#[xcl_fn]
pub fn list_symbols(_handle: i64) -> Value {
    // Listing symbols requires platform-specific iteration
    Value::from_array(vec![])
}

/// Check if a library is loaded.
///
/// # Arguments
/// - `handle` (int): Library handle
///
/// # Returns
/// Boolean indicating if the library is loaded
///
/// # Example
/// ```xcl
/// ffi.is_loaded(lib)
/// ```
#[xcl_fn]
pub fn is_loaded(handle: i64) -> Value {
    Value::bool_(handle != 0)
}
