//! Loads the interpreter shared library at runtime.
//!
//! The launcher has no link-time dependency on the VM. It locates
//! `xcl_toolchain.dll` (or `libxcl_toolchain.so` / `.dylib`) and resolves the small
//! C ABI exported by `xcl-toolchains`. Keeping the handle alive for the process
//! lifetime keeps the loaded code and its allocator alive with it.

use std::ffi::{CString, c_char, c_void};
use std::path::{Path, PathBuf};

/// toolchain ABI version this launcher was built against.
const TOOLCHAIN_ABI_VERSION: u32 = 7;
/// Engine ABI version this launcher was built against.
pub const ENGINE_ABI_VERSION: u32 = 1;

/// C functions exported by the runtime.
pub struct RuntimeApi {
    // Dropped last: the symbols below point into this library.
    _library: platform::Library,

    pub new: unsafe extern "C" fn() -> *mut c_void,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub set_quiet: unsafe extern "C" fn(*mut c_void, i32),
    pub set_format: unsafe extern "C" fn(*mut c_void, i32),
    pub add_level: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32,
    pub set_script_args: unsafe extern "C" fn(*mut c_void, usize, *const *const c_char),
    pub set_features: unsafe extern "C" fn(*mut c_void, *const c_char, i32) -> i32,
    pub set_profile: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub list_plugins: unsafe extern "C" fn(*mut c_void) -> i32,
    pub manifest: unsafe extern "C" fn(*mut c_void) -> i32,
    pub abi_list: unsafe extern "C" fn(*mut c_void) -> i32,
    pub run_file: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub run_source: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> i32,
    pub decompile_file: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub decompile_source: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> i32,
    pub lint_file: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub lint_source: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> i32,
    pub list_lints: unsafe extern "C" fn(*mut c_void) -> i32,
    pub explain: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub compile_file: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32,
    pub run_bytecode: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
}

impl RuntimeApi {
    /// Loads and resolves the runtime.
    pub fn load() -> Result<Self, String> {
        let (path, library) = open_library()?;
        let _ = &path;

        macro_rules! api {
            ($name:literal, $ty:ty) => {{
                let symbol = CString::new($name).expect("static symbol name");
                let pointer = unsafe { library.symbol(&symbol) }
                    .ok_or_else(|| format!("runtime is missing `{}`", $name))?;
                // SAFETY: `xcl-toolchains` defines these names with these exact
                // signatures. The ABI version check below rejects stale builds.
                unsafe { std::mem::transmute::<*mut c_void, $ty>(pointer) }
            }};
        }

        let version = api!("xcl_toolchain_abi_version", unsafe extern "C" fn() -> u32);
        let version = unsafe { version() };
        if version != TOOLCHAIN_ABI_VERSION {
            return Err(format!(
                "toolchain ABI version {version}, expected {TOOLCHAIN_ABI_VERSION} ({})",
                path.display()
            ));
        }

        Ok(Self {
            new: api!("xcl_toolchain_new", unsafe extern "C" fn() -> *mut c_void),
            free: api!("xcl_toolchain_free", unsafe extern "C" fn(*mut c_void)),
            set_quiet: api!("xcl_toolchain_set_quiet", unsafe extern "C" fn(*mut c_void, i32)),
            set_format: api!("xcl_toolchain_set_format", unsafe extern "C" fn(*mut c_void, i32)),
            add_level: api!(
                "xcl_toolchain_add_level",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32
            ),
            set_script_args: api!(
                "xcl_toolchain_set_script_args",
                unsafe extern "C" fn(*mut c_void, usize, *const *const c_char)
            ),
            set_features: api!(
                "xcl_toolchain_set_features",
                unsafe extern "C" fn(*mut c_void, *const c_char, i32) -> i32
            ),
            set_profile: api!(
                "xcl_toolchain_set_profile",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            list_plugins: api!(
                "xcl_toolchain_list_plugins",
                unsafe extern "C" fn(*mut c_void) -> i32
            ),
            manifest: api!("xcl_toolchain_manifest", unsafe extern "C" fn(*mut c_void) -> i32),
            abi_list: api!("xcl_toolchain_abi_list", unsafe extern "C" fn(*mut c_void) -> i32),
            run_file: api!(
                "xcl_toolchain_run_file",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            run_source: api!(
                "xcl_toolchain_run_source",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> i32
            ),
            decompile_file: api!(
                "xcl_toolchain_decompile_file",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            decompile_source: api!(
                "xcl_toolchain_decompile_source",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> i32
            ),
            lint_file: api!(
                "xcl_toolchain_lint_file",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            lint_source: api!(
                "xcl_toolchain_lint_source",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> i32
            ),
            list_lints: api!(
                "xcl_toolchain_list_lints",
                unsafe extern "C" fn(*mut c_void) -> i32
            ),
            explain: api!(
                "xcl_toolchain_explain",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            compile_file: api!(
                "xcl_toolchain_compile_file",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32
            ),
            run_bytecode: api!(
                "xcl_toolchain_run_bytecode",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            _library: library,
        })
    }
}

fn file_name() -> &'static str {
    if cfg!(windows) {
        "xcl_toolchain.dll"
    } else if cfg!(target_os = "macos") {
        "libxcl_toolchain.dylib"
    } else {
        "libxcl_toolchain.so"
    }
}

/// Candidate locations, in priority order.
fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("XCL_TOOLCHAIN") {
        paths.push(PathBuf::from(explicit));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        paths.push(dir.join(file_name()));
        let deps = dir.join("deps");
        paths.push(deps.join(file_name()));
        // Cargo may hash a dependency cdylib; accept that spelling too.
        paths.extend(hashed_artifacts(&deps));
    }

    paths
}

fn hashed_artifacts(deps: &Path) -> Vec<PathBuf> {
    let prefix = if cfg!(windows) {
        "xcl_toolchain"
    } else {
        "libxcl_toolchain"
    };
    let suffix = if cfg!(windows) {
        ".dll"
    } else if cfg!(target_os = "macos") {
        ".dylib"
    } else {
        ".so"
    };

    let Ok(entries) = std::fs::read_dir(deps) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix))
        })
        .collect()
}

/// C functions exported by the engine (`xcl_rt.dll`).
pub struct EngineApi {
    // Dropped last: the symbols below point into this library.
    _library: platform::Library,

    pub new: unsafe extern "C" fn() -> *mut c_void,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub load: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub run: unsafe extern "C" fn(*mut c_void) -> i32,
    pub error: unsafe extern "C" fn(*const c_void) -> *const c_char,
    pub error_code: unsafe extern "C" fn(*const c_void) -> i32,
    pub set_quiet: unsafe extern "C" fn(*mut c_void, i32),
    pub set_script_args: unsafe extern "C" fn(*mut c_void, usize, *const *const c_char) -> i32,
}

impl EngineApi {
    /// Loads and resolves the engine.
    pub fn load() -> Result<Self, String> {
        let (path, library) = open_engine_library()?;
        let _ = &path;

        macro_rules! api {
            ($name:literal, $ty:ty) => {{
                let symbol = CString::new($name).expect("static symbol name");
                let pointer = unsafe { library.symbol(&symbol) }
                    .ok_or_else(|| format!("engine is missing `{}`", $name))?;
                unsafe { std::mem::transmute::<*mut c_void, $ty>(pointer) }
            }};
        }

        let version = api!("xcl_rt_abi_version", unsafe extern "C" fn() -> u32);
        let version = unsafe { version() };
        if version != ENGINE_ABI_VERSION {
            return Err(format!(
                "engine ABI version {version}, expected {ENGINE_ABI_VERSION} ({})",
                path.display()
            ));
        }

        Ok(Self {
            new: api!("xcl_rt_new", unsafe extern "C" fn() -> *mut c_void),
            free: api!("xcl_rt_free", unsafe extern "C" fn(*mut c_void)),
            load: api!(
                "xcl_rt_load",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32
            ),
            run: api!("xcl_rt_run", unsafe extern "C" fn(*mut c_void) -> i32),
            error: api!(
                "xcl_rt_error",
                unsafe extern "C" fn(*const c_void) -> *const c_char
            ),
            error_code: api!(
                "xcl_rt_error_code",
                unsafe extern "C" fn(*const c_void) -> i32
            ),
            set_quiet: api!("xcl_rt_set_quiet", unsafe extern "C" fn(*mut c_void, i32)),
            set_script_args: api!(
                "xcl_rt_set_script_args",
                unsafe extern "C" fn(*mut c_void, usize, *const *const c_char) -> i32
            ),
            _library: library,
        })
    }
}

fn engine_file_name() -> &'static str {
    if cfg!(windows) {
        "xcl_rt.dll"
    } else if cfg!(target_os = "macos") {
        "libxcl_rt.dylib"
    } else {
        "libxcl_rt.so"
    }
}

fn engine_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("XCL_RT") {
        paths.push(PathBuf::from(explicit));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        paths.push(dir.join(engine_file_name()));
        let deps = dir.join("deps");
        paths.push(deps.join(engine_file_name()));
        paths.extend(engine_hashed_artifacts(&deps));
    }

    paths
}

fn engine_hashed_artifacts(deps: &Path) -> Vec<PathBuf> {
    let prefix = if cfg!(windows) { "xcl_rt" } else { "libxcl_rt" };
    let suffix = if cfg!(windows) {
        ".dll"
    } else if cfg!(target_os = "macos") {
        ".dylib"
    } else {
        ".so"
    };

    let Ok(entries) = std::fs::read_dir(deps) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix))
        })
        .collect()
}

fn open_engine_library() -> Result<(PathBuf, platform::Library), String> {
    let paths = engine_candidates();
    let mut errors = Vec::new();
    for path in &paths {
        if !path.exists() {
            continue;
        }
        match platform::Library::open(path) {
            Ok(library) => return Ok((path.clone(), library)),
            Err(error) => errors.push(format!("{}: {}", path.display(), error)),
        }
    }

    let tried = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n  ");
    let mut message = format!(
        "cannot load the XCL engine `{}`\n  tried:\n  {}",
        engine_file_name(),
        tried
    );
    if !errors.is_empty() {
        message.push_str("\n  errors:\n  ");
        message.push_str(&errors.join("\n  "));
    }
    message.push_str("\n  = help: build it with `cargo build -p xcl-rt` or set XCL_RT");
    Err(message)
}

fn open_library() -> Result<(PathBuf, platform::Library), String> {
    let paths = candidates();
    let mut errors = Vec::new();
    for path in &paths {
        if !path.exists() {
            continue;
        }
        match platform::Library::open(path) {
            Ok(library) => return Ok((path.clone(), library)),
            Err(error) => errors.push(format!("{}: {}", path.display(), error)),
        }
    }

    let tried = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n  ");
    let mut message = format!(
        "cannot load the XCL runtime `{}`\n  tried:\n  {}",
        file_name(),
        tried
    );
    if !errors.is_empty() {
        message.push_str("\n  errors:\n  ");
        message.push_str(&errors.join("\n  "));
    }
    message.push_str("\n  = help: build it with `cargo build -p xcl-toolchains` or set XCL_TOOLCHAIN");
    Err(message)
}

#[cfg(windows)]
mod platform {
    use std::ffi::{CStr, c_char, c_void};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    pub struct Library(*mut c_void);

    impl Library {
        pub fn open(path: &Path) -> Result<Self, String> {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
            if handle.is_null() {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                Ok(Self(handle))
            }
        }

        /// # Safety
        /// `name` must be a valid NUL-terminated symbol name.
        pub unsafe fn symbol(&self, name: &CStr) -> Option<*mut c_void> {
            let pointer = unsafe { GetProcAddress(self.0, name.as_ptr()) };
            (!pointer.is_null()).then_some(pointer)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            // Deliberately do not unload: Rust cdylibs register thread-local
            // destructors, and unloading the library while they are live can
            // leave dangling callbacks. The launcher is short-lived, so keeping
            // the code mapped until process exit is the safe choice.
            let _ = self.0;
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::Path;

    pub struct Library(*mut c_void);

    impl Library {
        pub fn open(path: &Path) -> Result<Self, String> {
            use std::os::unix::ffi::OsStrExt;
            let path = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| "path contains a NUL byte".to_string())?;
            let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) };
            if handle.is_null() {
                Err(last_error())
            } else {
                Ok(Self(handle))
            }
        }

        /// # Safety
        /// `name` must be a valid NUL-terminated symbol name.
        pub unsafe fn symbol(&self, name: &CStr) -> Option<*mut c_void> {
            let pointer = unsafe { libc::dlsym(self.0, name.as_ptr()) };
            (!pointer.is_null()).then_some(pointer)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            // Deliberately do not `dlclose`: Rust cdylibs register thread-local
            // destructors, and unloading the library while they are live can
            // leave dangling callbacks. The launcher is short-lived, so keeping
            // the code mapped until process exit is the safe choice.
            let _ = self.0;
        }
    }

    fn last_error() -> String {
        let pointer = unsafe { libc::dlerror() };
        if pointer.is_null() {
            "unknown dynamic-loader error".to_string()
        } else {
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        }
    }
}
