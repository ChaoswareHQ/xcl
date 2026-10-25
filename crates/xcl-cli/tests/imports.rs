//! End-to-end tests for `binding = module.symbol` imports.
//!
//! These run the real binary against a temporary directory, so module lookup
//! relative to the importing file, nested imports, and diagnostics are covered.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

const BIN: &str = env!("CARGO_BIN_EXE_xcl");

/// Locates the interpreter DLL, building it when the test binary runs alone.
fn runtime_path() -> &'static PathBuf {
    static RUNTIME: OnceLock<PathBuf> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        let exe = std::env::current_exe().expect("test executable");
        let dir = exe
            .parent()
            .and_then(|path| path.parent())
            .expect("target/debug");
        let name = if cfg!(windows) {
            "xcl_toolchain.dll"
        } else if cfg!(target_os = "macos") {
            "libxcl_toolchain.dylib"
        } else {
            "libxcl_toolchain.so"
        };
        let path = dir.join(name);
        if !path.exists() {
            let profile = dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("debug");
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut build = Command::new(cargo);
            build.args(["build", "-p", "xcl-toolchains"]);
            match profile {
                // Cargo's `dev` profile writes to `target/debug`.
                "debug" => {}
                "release" => {
                    build.arg("--release");
                }
                other => {
                    build.args(["--profile", other]);
                }
            }
            let status = build.status().expect("build xcl-toolchains");
            assert!(status.success(), "cargo build -p xcl-toolchains failed");
        }
        assert!(path.exists(), "runtime not found at {}", path.display());
        path
    })
}

/// A temporary directory holding a main script and any modules it imports.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("xcl-imports-{}-{}", std::process::id(), name));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, source).expect("write temp script");
        path
    }

    fn main(&self, source: &str) -> PathBuf {
        self.write("main.xcl", source)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run(script: &Path) -> Output {
    Command::new(BIN)
        .arg(script)
        .env("XCL_TOOLCHAIN", runtime_path())
        .output()
        .expect("run xcl")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn imports_a_function_from_another_file() {
    let dir = TempDir::new("basic");
    dir.write("test.xcl", "hello() {\n    print \"hello from test\"\n}\n");
    let main = dir.main("hello = test.hello\n\nhello\n");

    let output = run(&main);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("hello from test"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn an_import_can_bind_a_different_local_name() {
    let dir = TempDir::new("alias");
    dir.write(
        "test.xcl",
        "greet($name) {\n    print \"hello, \" $name\n}\n",
    );
    let main = dir.main("greeting = test.greet\n\ngreeting \"world\"\n");

    let output = run(&main);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("hello, world"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn nested_imports_resolve_relative_to_each_file() {
    let dir = TempDir::new("nested");
    dir.write("lib.xcl", "add($a, $b) {\n    return $a + $b\n}\n");
    dir.write(
        "nested.xcl",
        "add = lib.add\n\ntwice($n) {\n    return add $n $n\n}\n",
    );
    let main = dir.main("twice = nested.twice\n\nprint (twice 21)\n");

    let output = run(&main);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("42"), "{}", stdout(&output));
}

#[test]
fn a_module_can_export_a_type() {
    let dir = TempDir::new("type");
    dir.write("shape.xcl", "Point = {\n    x: int\n    y: int\n}\n");
    let main = dir.main("Point = shape.Point\n\n$p = Point { x: 3, y: 4 }\nprint $p.x\n");

    let output = run(&main);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains('3'), "{}", stdout(&output));
}

#[test]
fn a_module_loaded_twice_is_only_included_once() {
    let dir = TempDir::new("twice");
    dir.write("a.xcl", "a($v) {\n    return 1\n}\n");
    dir.write("b.xcl", "a = a.a\n\nb($v) {\n    return a $v\n}\n");
    let main = dir.main("a = a.a\nb = b.b\n\nprint ((a 0) + (b 0))\n");

    let output = run(&main);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains('2'), "{}", stdout(&output));
}

#[test]
fn a_missing_module_is_a_clean_error() {
    let dir = TempDir::new("missing");
    let main = dir.main("hello = missing.hello\n\nhello\n");

    let output = run(&main);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("cannot find module `missing`"), "{err}");
}

#[test]
fn a_missing_symbol_is_a_clean_error() {
    let dir = TempDir::new("symbol");
    dir.write("test.xcl", "hello() {\n    return 1\n}\n");
    let main = dir.main("nope = test.nope\n");

    let output = run(&main);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(
        err.contains("has no function or type named `nope`"),
        "{err}"
    );
}

#[test]
fn a_circular_import_is_a_clean_error() {
    let dir = TempDir::new("cycle");
    dir.write("a.xcl", "b = b.b\n\na() {\n    return 1\n}\n");
    dir.write("b.xcl", "a = a.a\n\nb() {\n    return 2\n}\n");
    let main = dir.main("a = a.a\n");

    let output = run(&main);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("circular import"), "{err}");
}

#[test]
fn removing_import_lines_preserves_later_line_numbers() {
    let dir = TempDir::new("lines");
    dir.write("test.xcl", "hello() {\n    return 1\n}\n");
    let main = dir.main("hello = test.hello\n\nhello(\n");

    let output = run(&main);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(
        err.contains(":3:"),
        "the error must still point at line 3: {err}"
    );
}
