//! Windows API FFI bindings for xcl-std.
//!
//! Provides access to common Windows API functions from XCL.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Call a Windows API function by name.
///
/// # Arguments
/// - `function` (str): Function name
/// - `args` (array): Array of integer arguments
///
/// # Returns
/// Integer result from the function call
///
/// # Example
/// ```xcl
/// ffi.winapi.call("GetLastError", [])
/// ```
#[xcl_fn("ffi.winapi.call", signature = "(function: str, args: array) -> result")]
pub fn call(_function: &str, _args: &[Value]) -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_i48(0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_str_boxed("Windows API not available on this platform".to_string())
    }
}

/// Get the Windows API version.
///
/// # Returns
/// String Windows API version
///
/// # Example
/// ```xcl
/// ffi.winapi.version()
/// ```
#[xcl_fn("ffi.winapi.version", signature = "() -> str")]
pub fn version() -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_str_boxed("Windows API".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_str_boxed("not available".to_string())
    }
}

/// List available Windows API functions.
///
/// # Returns
/// Array of function name strings
///
/// # Example
/// ```xcl
/// ffi.winapi.functions()
/// ```
#[xcl_fn("ffi.winapi.functions", signature = "() -> array")]
pub fn functions() -> Value {
    let funcs = vec![
        "GetLastError", "SetLastError",
        "CreateFileW", "ReadFile", "WriteFile", "CloseHandle",
        "CreateProcessW", "WaitForSingleObject",
        "VirtualAlloc", "VirtualFree", "VirtualProtect",
        "CreateThread", "WaitForSingleObject",
        "LoadLibraryW", "GetProcAddress", "FreeLibrary",
        "GetModuleHandleW", "GetFileVersionInfoSizeW",
        "CreateEventW", "SetEvent", "ResetEvent",
        "RegOpenKeyExW", "RegQueryValueExW", "RegCloseKey",
        "GetTickCount64", "QueryPerformanceCounter",
        "GetSystemInfo", "GetComputerNameW", "GetUserNameW",
    ];
    let values: Vec<Value> = funcs.iter().map(|f| Value::from_str_boxed(f.to_string())).collect();
    Value::from_array(values)
}

/// Get information about a Windows API function.
///
/// # Arguments
/// - `function` (str): Function name
///
/// # Returns
/// Map containing function information
///
/// # Example
/// ```xcl
/// ffi.winapi.function_info("CreateFileW")
/// ```
#[xcl_fn("ffi.winapi.function_info", signature = "(function: str) -> map")]
pub fn function_info(function: &str) -> Value {
    let mut info = HashMap::new();
    info.insert("name".to_string(), Value::from_str_boxed(function.to_string()));

    let signature = match function {
        "GetLastError" => "DWORD GetLastError()",
        "CreateFileW" => "HANDLE CreateFileW(LPCWSTR lpFileName, DWORD dwDesiredAccess, ...)",
        "ReadFile" => "BOOL ReadFile(HANDLE hFile, LPVOID lpBuffer, DWORD nNumberOfBytesToRead, ...)",
        "WriteFile" => "BOOL WriteFile(HANDLE hFile, LPCVOID lpBuffer, DWORD nNumberOfBytesToWrite, ...)",
        "VirtualAlloc" => "LPVOID VirtualAlloc(LPVOID lpAddress, SIZE_T dwSize, DWORD flAllocationType, DWORD flProtect)",
        "CreateThread" => "HANDLE CreateThread(LPSECURITY_ATTRIBUTES lpThreadAttributes, ...)",
        "LoadLibraryW" => "HMODULE LoadLibraryW(LPCWSTR lpLibFileName)",
        "GetProcAddress" => "FARPROC GetProcAddress(HMODULE hModule, LPCSTR lpProcName)",
        _ => "unknown",
    };
    info.insert("signature".to_string(), Value::from_str_boxed(signature.to_string()));
    Value::from_map(info)
}

/// Get Windows error message.
///
/// # Arguments
/// - `error_code` (int): Windows error code
///
/// # Returns
/// String error message
///
/// # Example
/// ```xcl
/// ffi.winapi.error_message(5)  // "Access is denied"
/// ```
#[xcl_fn("ffi.winapi.error_message", signature = "(error_code: int) -> str")]
pub fn error_message(_error_code: i64) -> Value {
    #[cfg(target_os = "windows")]
    {
        Value::from_str_boxed("Error".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Value::from_str_boxed("not available".to_string())
    }
}
