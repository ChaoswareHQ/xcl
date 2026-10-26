//! Engine C ABI — the whole surface for embedding `xcl_rt.dll`.
//!
//! A host creates an engine, loads a `.xclb` file, runs it, and reads the error
//! text if something failed. There is deliberately nothing else: no compiler,
//! no diagnostics. The engine runs what it is given.
//!
//! # Design
//!
//! This ABI follows security and error-handling patterns inspired by the .NET
//! runtime (CoreCLR):
//!
//! - **Typed error codes**: Every failure returns a specific [`EngineErrorCode`]
//!   rather than a bare integer, mirroring .NET's `HRESULT` convention.
//! - **State machine**: The engine transitions through well-defined states
//!   ([`EngineState`]) — `Created` → `Loaded` → `Running` — preventing
//!   invalid operations (e.g., running without loading). This mirrors .NET's
//!   object-state validation in types like `StreamReader`.
//! - **Defensive boundaries**: Every FFI entry point validates pointers before
//!   dereferencing, sanitizes strings, and caps lengths — similar to .NET's
//!   `MarshalString` bounds checks.
//! - **Capability isolation**: The engine exposes only the minimal surface
//!   needed to run bytecode. No compiler, no diagnostics, no function-pointer
//!   tables cross the boundary.
//!
//! # Native functions
//!
//! The standard library ships inside `xcl-rt` and is registered by installing
//! the `std` feature. This C ABI has no registration entry point: a function
//! pointer table across the boundary would be a much larger and less safe
//! contract. A Rust host that wants its own natives links `xcl-rt` (the `rlib`)
//! and registers them on the [`VM`] it owns.
//!
//! # ABI version
//!
//! [`ENGINE_ABI_VERSION`] is independent of the toolchain ABI. A `.xclb` file
//! also records it, so an engine refuses bytecode it was not built to run.

use core::ffi::c_char;
use std::ffi::{CStr, CString};
use std::path::Path;

#[cfg(feature = "mimalloc")]
use mimalloc::MiMalloc;

use crate::Value;
use crate::bytecode::{Bytecode, ENGINE_ABI};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// Engine ABI version. Bumped when the C surface changes.
pub const ENGINE_ABI_VERSION: u32 = ENGINE_ABI as u32;

// ── Error codes ──────────────────────────────────────────────────────────────

/// Specific error categories returned by the engine ABI.
///
/// Modeled after .NET's `HRESULT` convention: each code is a positive `i32`
/// that a host can match on to determine the failure category.  Zero means
/// success and is not a variant here.
///
/// # Categories
///
/// | Range | Category |
/// |---|---|
/// | 1–9 | Argument / validation errors |
/// | 10–19 | State / lifecycle errors |
/// | 20–29 | Bytecode / file errors |
/// | 30–39 | Runtime execution errors |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EngineErrorCode {
    /// A required pointer argument is null.
    NullPointer = 1,
    /// A string argument is not valid UTF-8.
    InvalidUtf8 = 2,
    /// A string argument exceeds the maximum allowed length.
    StringTooLong = 3,
    /// An argument array length is inconsistent with its pointer.
    InvalidArgArray = 4,

    /// The engine is in an invalid state for the requested operation.
    InvalidState = 10,
    /// The engine has already been freed or is otherwise disposed.
    Disposed = 11,

    /// The `.xclb` file could not be read from disk.
    FileIo = 20,
    /// The file is not a valid `.xclb` bytecode file.
    BadBytecode = 21,
    /// The file was written by a different engine ABI version.
    AbiMismatch = 22,
    /// The file checksum failed — the bytecode is corrupted.
    ChecksumMismatch = 23,
    /// The file is too large to be loaded.
    FileTooLarge = 24,

    /// The bytecode executed but returned an error result.
    RuntimeError = 30,
    /// Execution was cancelled or timed out.
    ExecutionCancelled = 31,
}

impl EngineErrorCode {
    /// Convert to the `i32` return code used by C functions.
    #[inline]
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

// ── Engine state machine ─────────────────────────────────────────────────────

/// Lifecycle state of an [`XclEngine`].
///
/// Transitions:
/// - `Created` → `Loaded` via [`xcl_rt_load`]
/// - `Loaded` → `Running` via [`xcl_rt_run`]
/// - `Running` → `Loaded` after run completes (re-entrant)
/// - Any state → error state on failure (cleared on next success)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineState {
    /// Freshly created; no bytecode loaded yet.
    Created,
    /// Bytecode loaded and ready to run.
    Loaded,
    /// Currently executing bytecode.
    Running,
}

// ── Limits ───────────────────────────────────────────────────────────────────

/// Maximum length of a path string accepted by the ABI (2 KB).
const MAX_PATH_LEN: usize = 2 * 1024;

/// Maximum number of script arguments accepted by the ABI.
const MAX_SCRIPT_ARGS: usize = 4096;

/// Maximum length of a single script argument (4 KB).
const MAX_SCRIPT_ARG_LEN: usize = 4 * 1024;

/// Opaque engine handle exposed to C.
pub struct XclEngine {
    bytecode: Option<Bytecode>,
    state: EngineState,
    quiet: bool,
    script_args: Vec<String>,
    error_code: EngineErrorCode,
    error: CString,
}

impl Default for XclEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl XclEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            bytecode: None,
            state: EngineState::Created,
            quiet: false,
            script_args: Vec::new(),
            error_code: EngineErrorCode::InvalidState,
            error: CString::default(),
        }
    }

    /// Set a typed error, updating both the error code and the message.
    fn set_error(&mut self, code: EngineErrorCode, message: &str) {
        self.error_code = code;
        self.error = CString::new(message.replace('\0', " ")).unwrap_or_default();
    }

    /// Clear any previous error, returning to a clean state.
    fn clear_error(&mut self) {
        self.error_code = EngineErrorCode::InvalidState;
        self.error = CString::default();
    }
}

/// Returns the engine ABI version.
#[unsafe(no_mangle)]
pub extern "C" fn xcl_rt_abi_version() -> u32 {
    ENGINE_ABI_VERSION
}

/// Creates an engine. The pointer must be passed to [`xcl_rt_free`].
#[unsafe(no_mangle)]
pub extern "C" fn xcl_rt_new() -> *mut XclEngine {
    Box::into_raw(Box::new(XclEngine::new()))
}

/// Frees an engine. A null pointer is ignored.
///
/// # Safety
/// `engine` must come from [`xcl_rt_new`] and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_free(engine: *mut XclEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Loads a `.xclb` file. Returns 0 on success, non-zero on error.
///
/// The return value is a typed [`EngineErrorCode`] cast to `i32`, so a host
/// can distinguish between argument errors, file I/O errors, and bytecode
/// format errors.
///
/// # Safety
/// `engine` must be a live pointer and `path` a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_load(engine: *mut XclEngine, path: *const c_char) -> i32 {
    // Defensive: validate engine pointer before any access
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return EngineErrorCode::NullPointer.as_i32();
    };

    // Defensive: validate path pointer
    if path.is_null() {
        engine.set_error(EngineErrorCode::NullPointer, "path pointer is null");
        return EngineErrorCode::NullPointer.as_i32();
    }

    // Defensive: validate UTF-8 and length
    let cstr = unsafe { CStr::from_ptr(path) };
    let path_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            engine.set_error(EngineErrorCode::InvalidUtf8, "path is not valid UTF-8");
            return EngineErrorCode::InvalidUtf8.as_i32();
        }
    };

    // Defensive: cap path length to prevent excessive memory allocation
    if path_str.len() > MAX_PATH_LEN {
        engine.set_error(
            EngineErrorCode::StringTooLong,
            &format!("path exceeds maximum length {MAX_PATH_LEN}"),
        );
        return EngineErrorCode::StringTooLong.as_i32();
    }

    // Load bytecode with typed error mapping
    match Bytecode::from_file(Path::new(path_str)) {
        Ok(bytecode) => {
            engine.bytecode = Some(bytecode);
            engine.state = EngineState::Loaded;
            engine.clear_error();
            0
        }
        Err(error) => {
            // Map BytecodeError to typed EngineErrorCode
            let (code, message) = map_bytecode_error(&error);
            engine.set_error(code, &message);
            code.as_i32()
        }
    }
}

/// Maps a [`crate::bytecode::BytecodeError`] to a typed [`EngineErrorCode`] and message.
fn map_bytecode_error(error: &crate::bytecode::BytecodeError) -> (EngineErrorCode, String) {
    use crate::bytecode::BytecodeError;
    match error {
        BytecodeError::Io(msg) => (EngineErrorCode::FileIo, format!("cannot read file: {msg}")),
        BytecodeError::TooShort | BytecodeError::BadMagic | BytecodeError::BadFormatVersion(_)
        | BytecodeError::TruncatedSection | BytecodeError::InvalidUtf8
        | BytecodeError::UnknownValueTag(_) | BytecodeError::TrailingBytes => {
            (EngineErrorCode::BadBytecode, error.to_string())
        }
        BytecodeError::EngineAbiMismatch { file, expected } => {
            (EngineErrorCode::AbiMismatch, format!("engine ABI mismatch: file is {file}, expected {expected}"))
        }
        BytecodeError::ChecksumMismatch => {
            (EngineErrorCode::ChecksumMismatch, "bytecode checksum mismatch — file is corrupted".to_string())
        }
        BytecodeError::FileTooLarge => {
            (EngineErrorCode::FileTooLarge, "bytecode file exceeds size limit".to_string())
        }
        BytecodeError::UnserializableValue | BytecodeError::StringNotInterned | BytecodeError::TooDeep => {
            (EngineErrorCode::BadBytecode, error.to_string())
        }
    }
}

/// Runs the loaded bytecode. Returns 0 on success, non-zero on error.
///
/// The return value is a typed [`EngineErrorCode`] cast to `i32`.
///
/// Stdout is flushed before returning, so a host that exits immediately after
/// the call does not lose `print` output.
///
/// # Safety
/// `engine` must be a live pointer with bytecode loaded by [`xcl_rt_load`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_run(engine: *mut XclEngine) -> i32 {
    // Defensive: validate engine pointer
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return EngineErrorCode::NullPointer.as_i32();
    };

    // State machine: require Loaded state before running
    if engine.state != EngineState::Loaded {
        engine.set_error(
            EngineErrorCode::InvalidState,
            "no bytecode loaded; call xcl_rt_load first",
        );
        flush_streams();
        return EngineErrorCode::InvalidState.as_i32();
    }

    let Some(bytecode) = engine.bytecode.as_ref() else {
        engine.set_error(
            EngineErrorCode::InvalidState,
            "no bytecode loaded; call xcl_rt_load first",
        );
        flush_streams();
        return EngineErrorCode::InvalidState.as_i32();
    };

    // Transition to Running state
    engine.state = EngineState::Running;

    let mut vm = crate::new_vm();
    if !engine.script_args.is_empty() {
        let args: Vec<Value> = engine
            .script_args
            .iter()
            .map(|arg| Value::from_str_boxed(arg.clone()))
            .collect();
        vm.set_script_args("<bytecode>", args);
    }

    let result = vm.run(bytecode);
    if !engine.quiet && !result.is_nil() {
        print_value(result);
    }

    // Check for runtime error result
    let failed = result
        .as_map()
        .and_then(|map| map.get("tag").and_then(|tag| tag.as_str()))
        == Some("Err");
    if failed {
        let message = result
            .as_map()
            .and_then(|map| map.get("message").and_then(|value| value.as_str()))
            .unwrap_or("unknown runtime error")
            .to_string();
        engine.set_error(EngineErrorCode::RuntimeError, &message);
    } else {
        // Success — transition back to Loaded (re-entrant)
        engine.state = EngineState::Loaded;
        engine.clear_error();
    }

    flush_streams();
    i32::from(failed)
}

/// Returns the last error as a NUL-terminated string, owned by the engine.
///
/// The pointer stays valid until the next call that sets an error.
///
/// # Safety
/// `engine` must be a live pointer; the returned pointer must not be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_error(engine: *const XclEngine) -> *const c_char {
    match unsafe { engine.as_ref() } {
        Some(engine) => engine.error.as_ptr(),
        None => c"null engine pointer".as_ptr(),
    }
}

/// Returns the last error code as an `i32`.
///
/// Returns 0 when no error is recorded (success state).  Otherwise returns
/// one of the [`EngineErrorCode`] values so a host can programmatically
/// determine the failure category without parsing the error string.
///
/// # Safety
/// `engine` must be a live pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_error_code(engine: *const XclEngine) -> i32 {
    match unsafe { engine.as_ref() } {
        Some(engine) => {
            // Return 0 when error string is empty (no active error)
            if engine.error.is_empty() {
                0
            } else {
                engine.error_code.as_i32()
            }
        }
        None => EngineErrorCode::NullPointer.as_i32(),
    }
}

/// Sets whether the final value is printed after a run.
///
/// # Safety
/// `engine` must be a live pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_set_quiet(engine: *mut XclEngine, quiet: i32) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.quiet = quiet != 0;
    }
}

/// Copies the positional script arguments (`$1`, `$2`, …).
///
/// Returns 0 on success, non-zero on error (typed [`EngineErrorCode`]).
///
/// # Safety
/// `engine` must be a live pointer; `argv` must point to `argc` valid
/// NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_set_script_args(
    engine: *mut XclEngine,
    argc: usize,
    argv: *const *const c_char,
) -> i32 {
    // Defensive: validate engine pointer
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return EngineErrorCode::NullPointer.as_i32();
    };

    // Defensive: validate argument count
    if argc > MAX_SCRIPT_ARGS {
        engine.set_error(
            EngineErrorCode::InvalidArgArray,
            &format!("script arguments exceed maximum count {MAX_SCRIPT_ARGS}"),
        );
        return EngineErrorCode::InvalidArgArray.as_i32();
    }

    // Defensive: validate argv pointer
    if argc > 0 && argv.is_null() {
        engine.set_error(
            EngineErrorCode::InvalidArgArray,
            "argv pointer is null while argc > 0",
        );
        return EngineErrorCode::InvalidArgArray.as_i32();
    }

    let mut args = Vec::with_capacity(argc);
    for index in 0..argc {
        let pointer = unsafe { *argv.add(index) };
        if pointer.is_null() {
            continue;
        }
        let cstr = unsafe { CStr::from_ptr(pointer) };
        match cstr.to_str() {
            Ok(arg) => {
                // Defensive: cap individual argument length
                if arg.len() > MAX_SCRIPT_ARG_LEN {
                    engine.set_error(
                        EngineErrorCode::StringTooLong,
                        &format!("script argument {index} exceeds maximum length {MAX_SCRIPT_ARG_LEN}"),
                    );
                    return EngineErrorCode::StringTooLong.as_i32();
                }
                args.push(arg.to_string());
            }
            Err(_) => {
                engine.set_error(
                    EngineErrorCode::InvalidUtf8,
                    &format!("script argument {index} is not valid UTF-8"),
                );
                return EngineErrorCode::InvalidUtf8.as_i32();
            }
        }
    }
    engine.script_args = args;
    0
}

fn flush_streams() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

fn print_value(value: Value) {
    if let Some(text) = value.as_str() {
        println!("{text}");
    } else if value.is_int() {
        println!("{}", value.as_i48());
    } else if value.is_float() {
        println!("{}", value.as_f64());
    } else if value.is_bool() {
        println!("{}", value.as_bool());
    } else if let Some(map) = value.as_map() {
        println!("{{");
        for (key, item) in map {
            print!("  {key}: ");
            print_value(*item);
        }
        println!("}}");
    } else if let Some(array) = value.as_array() {
        print!("[");
        for (index, item) in array.iter().enumerate() {
            if index > 0 {
                print!(", ");
            }
            print_value(*item);
        }
        println!("]");
    } else if value.is_nil() {
        println!("none");
    } else {
        println!("<value>");
    }
}
