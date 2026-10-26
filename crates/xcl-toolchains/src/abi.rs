//! Loadable XCL toolchain C ABI.
//!
//! The `xcl` launcher is deliberately thin: it parses command-line options,
//! loads this shared library (`xcl_toolchain.dll` / `libxcl_toolchain.so` / `libxcl_toolchain.dylib`),
//! and forwards commands through the C ABI below.
//!
//! The ABI is intentionally narrow: the host passes UTF-8 paths/source and a
//! few configuration flags, and this library writes program output and
//! diagnostics to the standard streams. Return values are process exit codes
//! (`0` success, non-zero failure), so no allocated value crosses the boundary.

use std::ffi::{CStr, c_char};

use crate::host::{OutputFormat, Runtime, explain_code, flushed, print_lints, print_plugins};

/// ABI version understood by this toolchain. The launcher refuses a mismatch.
pub const ABI_VERSION: u32 = 7;

/// The C ABI this toolchain exports.
const ABI_EXPORTS: &[(&str, &str)] = &[
    (
        "xcl_toolchain_abi_version",
        "uint32_t xcl_toolchain_abi_version(void)",
    ),
    (
        "xcl_toolchain_abi_list",
        "int xcl_toolchain_abi_list(XclRuntime*)",
    ),
    ("xcl_toolchain_new", "XclRuntime* xcl_toolchain_new(void)"),
    ("xcl_toolchain_free", "void xcl_toolchain_free(XclRuntime*)"),
    (
        "xcl_toolchain_set_quiet",
        "void xcl_toolchain_set_quiet(XclRuntime*, int quiet)",
    ),
    (
        "xcl_toolchain_set_format",
        "void xcl_toolchain_set_format(XclRuntime*, int format)",
    ),
    (
        "xcl_toolchain_add_level",
        "int xcl_toolchain_add_level(XclRuntime*, const char* target, const char* level)",
    ),
    (
        "xcl_toolchain_set_script_args",
        "void xcl_toolchain_set_script_args(XclRuntime*, size_t argc, const char* const* argv)",
    ),
    (
        "xcl_toolchain_set_features",
        "int xcl_toolchain_set_features(XclRuntime*, const char* csv, int mode)",
    ),
    (
        "xcl_toolchain_set_profile",
        "int xcl_toolchain_set_profile(XclRuntime*, const char* name)",
    ),
    (
        "xcl_toolchain_list_plugins",
        "int xcl_toolchain_list_plugins(XclRuntime*)",
    ),
    (
        "xcl_toolchain_manifest",
        "int xcl_toolchain_manifest(XclRuntime*)",
    ),
    (
        "xcl_toolchain_run_file",
        "int xcl_toolchain_run_file(XclRuntime*, const char* path)",
    ),
    (
        "xcl_toolchain_run_source",
        "int xcl_toolchain_run_source(XclRuntime*, const char* name, const char* source, size_t len)",
    ),
    (
        "xcl_toolchain_decompile_file",
        "int xcl_toolchain_decompile_file(XclRuntime*, const char* path)",
    ),
    (
        "xcl_toolchain_decompile_source",
        "int xcl_toolchain_decompile_source(XclRuntime*, const char* name, const char* source, size_t len)",
    ),
    (
        "xcl_toolchain_lint_file",
        "int xcl_toolchain_lint_file(XclRuntime*, const char* path)",
    ),
    (
        "xcl_toolchain_lint_source",
        "int xcl_toolchain_lint_source(XclRuntime*, const char* name, const char* source, size_t len)",
    ),
    (
        "xcl_toolchain_list_lints",
        "int xcl_toolchain_list_lints(XclRuntime*)",
    ),
    (
        "xcl_toolchain_explain",
        "int xcl_toolchain_explain(XclRuntime*, const char* code)",
    ),
    (
        "xcl_toolchain_compile_file",
        "int xcl_toolchain_compile_file(XclRuntime*, const char* src, const char* out)",
    ),
    (
        "xcl_toolchain_run_bytecode",
        "int xcl_toolchain_run_bytecode(XclRuntime*, const char* path)",
    ),
];

fn print_abi() {
    println!("XCL toolchain ABI v{}", xcl_toolchain_abi_version());
    for (_, signature) in ABI_EXPORTS {
        println!("  {}", signature);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn xcl_toolchain_abi_version() -> u32 {
    ABI_VERSION
}

/// Prints the C ABI this SDK exports.
///
/// # Safety
/// `ctx` may be null; the function does not touch the runtime state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_abi_list(_ctx: *mut Runtime) -> i32 {
    print_abi();
    flushed(0)
}

/// Borrows the runtime behind an opaque pointer.
///
/// # Safety
/// `ctx` must be a pointer returned by [`xcl_toolchain_new`] that has not been
/// freed yet.
unsafe fn runtime<'a>(ctx: *mut Runtime) -> Option<&'a mut Runtime> {
    unsafe { ctx.as_mut() }
}

/// Borrows a NUL-terminated UTF-8 argument.
///
/// # Safety
/// `ptr` must be null or point to a valid NUL-terminated string.
unsafe fn text<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// Borrows a UTF-8 source buffer of `len` bytes.
///
/// # Safety
/// `ptr` must be valid for `len` bytes (or null when `len` is zero).
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if ptr.is_null() {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }).ok()
}

/// Creates a runtime instance.
///
/// # Safety
/// The returned pointer must be passed to [`xcl_toolchain_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_new() -> *mut Runtime {
    Box::into_raw(Box::new(Runtime::default()))
}

/// Frees a runtime instance. A null pointer is ignored.
///
/// # Safety
/// `ctx` must come from [`xcl_toolchain_new`] and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_free(ctx: *mut Runtime) {
    if !ctx.is_null() {
        unsafe { drop(Box::from_raw(ctx)) };
    }
}

/// Sets whether the result value is printed after a successful run.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_quiet(ctx: *mut Runtime, quiet: i32) {
    if let Some(runtime) = unsafe { runtime(ctx) } {
        runtime.quiet = quiet != 0;
    }
}

/// Sets the diagnostic format: `0` human, `1` JSON.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_format(ctx: *mut Runtime, format: i32) {
    if let Some(runtime) = unsafe { runtime(ctx) } {
        runtime.format = OutputFormat::from_i32(format);
    }
}

/// Adds a lint level override (`-A/-W/-D/-F`). Returns non-zero when the
/// target is unknown.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_add_level(
    ctx: *mut Runtime,
    target: *const c_char,
    level: *const c_char,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(target), Some(level)) = (unsafe { text(target) }, unsafe { text(level) }) else {
        return flushed(1);
    };
    flushed(runtime.add_level(target, level))
}

/// Copies the positional script arguments (`$1`, `$2`, ...).
///
/// # Safety
/// `argv` must point to `argc` valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_script_args(
    ctx: *mut Runtime,
    argc: usize,
    argv: *const *const c_char,
) {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return;
    };
    if argc > 0 && argv.is_null() {
        return;
    }
    let mut args = Vec::with_capacity(argc);
    for index in 0..argc {
        let pointer = unsafe { *argv.add(index) };
        if let Some(arg) = unsafe { text(pointer) } {
            args.push(arg.to_string());
        }
    }
    runtime.script_args = args;
}

/// Selects which compiled features to install.
///
/// `mode` is `0` defaults, `1` defaults plus `csv`, `2` only `csv` (plus
/// dependencies), and `3` every compiled feature. A null `csv` behaves like an
/// empty list. Returns non-zero and prints a diagnostic for an unknown feature.
///
/// # Safety
/// `ctx` must be a live runtime pointer; `csv` must be null or valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_features(
    ctx: *mut Runtime,
    csv: *const c_char,
    mode: i32,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return flushed(1);
    };
    let csv = if csv.is_null() {
        None
    } else {
        match unsafe { text(csv) } {
            Some(csv) => Some(csv),
            None => {
                eprintln!("xcl: error: invalid UTF-8 feature list");
                return flushed(1);
            }
        }
    };
    flushed(runtime.set_features(csv, mode))
}

/// Selects the capability profile.
///
/// `sandbox`, `automation`, `game-mod`, `trusted`, or `deny-all`. An explicit
/// profile overrides one named by an external plugin's `plugin.toml`. Returns
/// non-zero and prints a diagnostic for an unknown name.
///
/// # Safety
/// `ctx` must be a live runtime pointer; `name` must be null or valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_profile(ctx: *mut Runtime, name: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return flushed(1);
    };
    let Some(name) = (unsafe { text(name) }) else {
        eprintln!("xcl: error: invalid UTF-8 profile name");
        return flushed(1);
    };
    flushed(runtime.set_profile(name))
}

/// Prints the compiled plugin catalogue.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_list_plugins(ctx: *mut Runtime) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return flushed(1);
    };
    print_plugins(runtime);
    flushed(0)
}

/// Prints the generated plugin manifest (TOML) for the compiled registry.
///
/// The manifest is a projection of the registry populated by the registration
/// macros, so it always matches the code; it is not a hand-maintained file.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_manifest(_ctx: *mut Runtime) -> i32 {
    print!("{}", crate::manifest::manifest_toml());
    flushed(0)
}

/// Reads, resolves imports for, compiles, and runs a file.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_run_file(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.run_file(path))
}

/// Runs source text. `name` is used in diagnostics.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_run_source(
    ctx: *mut Runtime,
    name: *const c_char,
    source: *const u8,
    source_len: usize,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(name), Some(source)) = (unsafe { text(name) }, unsafe { bytes(source, source_len) })
    else {
        eprintln!("xcl: error: invalid UTF-8 source");
        return flushed(1);
    };
    flushed(runtime.run_source(name, source))
}

/// Disassembles a source file after resolving its imports.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_decompile_file(
    ctx: *mut Runtime,
    path: *const c_char,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.decompile_file(path))
}

/// Disassembles source text.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_decompile_source(
    ctx: *mut Runtime,
    name: *const c_char,
    source: *const u8,
    source_len: usize,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(name), Some(source)) = (unsafe { text(name) }, unsafe { bytes(source, source_len) })
    else {
        eprintln!("xcl: error: invalid UTF-8 source");
        return flushed(1);
    };
    flushed(runtime.decompile_source(name, source))
}

/// Lints a source file after removing import lines.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_lint_file(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.lint_file(path))
}

/// Lints source text.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_lint_source(
    ctx: *mut Runtime,
    name: *const c_char,
    source: *const u8,
    source_len: usize,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(name), Some(source)) = (unsafe { text(name) }, unsafe { bytes(source, source_len) })
    else {
        eprintln!("xcl: error: invalid UTF-8 source");
        return flushed(1);
    };
    flushed(runtime.lint_source(name, source))
}

/// Prints the lint catalogue.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_list_lints(_ctx: *mut Runtime) -> i32 {
    print_lints();
    flushed(0)
}

/// Prints one rule's documentation. Returns non-zero for an unknown code.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `code` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_explain(_ctx: *mut Runtime, code: *const c_char) -> i32 {
    let Some(code) = (unsafe { text(code) }) else {
        return flushed(1);
    };
    flushed(explain_code(code))
}

/// Compiles a source file to `.xclb` bytecode without running it.
///
/// `src` is the path to the `.xcl` source file. `out` is the output `.xclb`
/// path. Imports are resolved exactly like `run_file`. Returns `0` on
/// success, non-zero on error.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_compile_file(
    ctx: *mut Runtime,
    src: *const c_char,
    out: *const c_char,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(src), Some(out)) = (unsafe { text(src) }, unsafe { text(out) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.compile_file(src, out))
}

/// Decodes a `.xclb` bytecode file and runs it on a VM built from the
/// selected features. Does not touch the lexer/parser/compiler.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_run_bytecode(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.run_bytecode(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn abi_reports_its_version() {
        assert_eq!(xcl_toolchain_abi_version(), ABI_VERSION);
    }

    #[test]
    fn feature_selection_uses_exact_names() {
        let mut runtime = Runtime::default();
        assert_eq!(runtime.set_features(Some("std"), 2), 0);
        assert_eq!(runtime.set_features(Some("std_util"), 2), 1);
        assert_eq!(runtime.set_features(Some("missing_feature"), 2), 1);
    }

    #[test]
    fn exported_run_source_returns_exit_codes() {
        let ctx = unsafe { xcl_toolchain_new() };
        assert!(!ctx.is_null());
        unsafe { xcl_toolchain_set_quiet(ctx, 1) };

        let name = CString::new("<test>").expect("static name");
        let good = b"$x = 1 + 2";
        let status =
            unsafe { xcl_toolchain_run_source(ctx, name.as_ptr(), good.as_ptr(), good.len()) };
        assert_eq!(status, 0);

        let bad = b"$x = ";
        let status =
            unsafe { xcl_toolchain_run_source(ctx, name.as_ptr(), bad.as_ptr(), bad.len()) };
        assert_eq!(status, 1);

        unsafe { xcl_toolchain_free(ctx) };
    }

    #[test]
    fn freeing_null_is_ignored() {
        unsafe { xcl_toolchain_free(std::ptr::null_mut()) };
    }
}
