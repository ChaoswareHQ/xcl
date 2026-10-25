//! Engine C ABI — the whole surface for embedding `xcl_rt.dll`.
//!
//! This crate is the shipped artifact, not the engine library: it owns the
//! global allocator and exposes the C ABI. A Rust host that wants its own
//! allocator links `xcl-rt` directly instead.
//!
//! A host creates an engine, loads a `.xclb` file, runs it, and reads the error
//! text if something failed. There is deliberately nothing else: no compiler,
//! no diagnostics. The engine runs what it is given.
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

use mimalloc::MiMalloc;
use xcl_rt::Value;
use xcl_rt::bytecode::{Bytecode, ENGINE_ABI};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// Engine ABI version. Bumped when the C surface changes.
pub const ENGINE_ABI_VERSION: u32 = ENGINE_ABI as u32;

/// Opaque engine handle exposed to C.
pub struct XclEngine {
    bytecode: Option<Bytecode>,
    quiet: bool,
    script_args: Vec<String>,
    error: CString,
}

impl XclEngine {
    fn new() -> Self {
        Self {
            bytecode: None,
            quiet: false,
            script_args: Vec::new(),
            error: CString::default(),
        }
    }

    fn set_error(&mut self, message: &str) {
        self.error = CString::new(message.replace('\0', " ")).unwrap_or_default();
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
/// # Safety
/// `engine` must be a live pointer and `path` a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_load(engine: *mut XclEngine, path: *const c_char) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 1;
    };
    if path.is_null() {
        engine.set_error("null path");
        return 1;
    }
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        engine.set_error("path is not valid UTF-8");
        return 1;
    };

    match Bytecode::from_file(Path::new(path)) {
        Ok(bytecode) => {
            engine.bytecode = Some(bytecode);
            engine.error = CString::default();
            0
        }
        Err(error) => {
            engine.set_error(&error.to_string());
            1
        }
    }
}

/// Runs the loaded bytecode. Returns 0 on success, non-zero on error.
///
/// Stdout is flushed before returning, so a host that exits immediately after
/// the call does not lose `print` output.
///
/// # Safety
/// `engine` must be a live pointer with bytecode loaded by [`xcl_rt_load`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_run(engine: *mut XclEngine) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 1;
    };
    let Some(bytecode) = engine.bytecode.as_ref() else {
        engine.set_error("no bytecode loaded; call xcl_rt_load first");
        flush_streams();
        return 1;
    };

    let mut vm = xcl_rt::new_vm();
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
        engine.set_error(&format!("runtime error: {message}"));
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
/// # Safety
/// `engine` must be a live pointer; `argv` must point to `argc` valid
/// NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_rt_set_script_args(
    engine: *mut XclEngine,
    argc: usize,
    argv: *const *const c_char,
) {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return;
    };
    if argc > 0 && argv.is_null() {
        return;
    }

    let mut args = Vec::with_capacity(argc);
    for index in 0..argc {
        let pointer = unsafe { *argv.add(index) };
        if pointer.is_null() {
            continue;
        }
        if let Ok(arg) = unsafe { CStr::from_ptr(pointer) }.to_str() {
            args.push(arg.to_string());
        }
    }
    engine.script_args = args;
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
