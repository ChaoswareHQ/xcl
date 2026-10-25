//! End-to-end tests for the lint surface of the `xcl` binary.
//!
//! These run the real binary, so they cover argument parsing, exit codes, and
//! which stream each format goes to — the parts a unit test cannot see.

#![cfg(feature = "lint")]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;

const BIN: &str = env!("CARGO_BIN_EXE_xcl");

/// Locates the toolchain library, building it when the test binary runs alone.
fn runtime_path() -> &'static PathBuf {
    static TOOLCHAIN: OnceLock<PathBuf> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let name = if cfg!(windows) {
            "xcl_toolchain.dll"
        } else if cfg!(target_os = "macos") {
            "libxcl_toolchain.dylib"
        } else {
            "libxcl_toolchain.so"
        };
        ensure_library("xcl-toolchains", name)
    })
}

/// Locates the engine library, building it when the test binary runs alone.
fn engine_path() -> &'static PathBuf {
    static ENGINE: OnceLock<PathBuf> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let name = if cfg!(windows) {
            "xcl_rt.dll"
        } else if cfg!(target_os = "macos") {
            "libxcl_rt.dylib"
        } else {
            "libxcl_rt.so"
        };
        ensure_library("xcl-rt-dll", name)
    })
}

/// Builds `package` if `name` is missing from the target directory.
fn ensure_library(package: &str, name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("test executable");
    let dir = exe
        .parent()
        .and_then(|path| path.parent())
        .expect("target/debug");
    let path = dir.join(name);
    if !path.exists() {
        let profile = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("debug");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut build = Command::new(cargo);
        build.args(["build", "-p", package]);
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
        let status = build.status().expect("build library");
        assert!(status.success(), "cargo build -p {package} failed");
    }
    assert!(path.exists(), "library not found at {}", path.display());
    path
}

/// A script file in the temp directory, removed when the test ends.
struct TempScript {
    path: PathBuf,
}

impl TempScript {
    fn new(name: &str, source: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("xcl-cli-{}-{}.xcl", std::process::id(), name));
        fs::write(&path, source).expect("write temp script");
        Self { path }
    }

    fn as_arg(&self) -> &str {
        self.path.to_str().expect("temp path is UTF-8")
    }
}

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// An external plugin directory: `plugin.toml` plus an entry script.
struct TempPlugin {
    dir: PathBuf,
}

impl TempPlugin {
    fn new(name: &str, manifest: &str, entry: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!("xcl-plugin-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create plugin dir");
        fs::write(dir.join("plugin.toml"), manifest).expect("write plugin.toml");
        fs::write(dir.join("main.xcl"), entry).expect("write entry script");
        Self { dir }
    }

    fn as_arg(&self) -> &str {
        self.dir.to_str().expect("temp path is UTF-8")
    }
}

impl Drop for TempPlugin {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("XCL_TOOLCHAIN", runtime_path())
        .env("XCL_RT", engine_path())
        .output()
        .expect("run xcl")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const WITH_LINTS: &str = "$unused = 1\nprint \"done\"\n";

#[test]
fn lint_mode_reports_a_warning_and_exits_zero() {
    let script = TempScript::new("warn", WITH_LINTS);
    let output = run(&["--lint", script.as_arg()]);

    assert!(output.status.success(), "warnings alone must not fail");
    let err = stderr(&output);
    assert!(err.contains("warning[W0001]"), "{err}");
    assert!(err.contains("1 warning emitted"), "{err}");
    assert!(
        stdout(&output).is_empty(),
        "human diagnostics belong on stderr"
    );
}

#[test]
fn deny_warnings_turns_the_report_into_a_failure() {
    let script = TempScript::new("deny", WITH_LINTS);
    let output = run(&["-D", "warnings", "--lint", script.as_arg()]);

    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("error[W0001]"), "{err}");
    assert!(err.contains("1 error emitted"), "{err}");
}

#[test]
fn deny_warnings_long_form_matches_the_short_form() {
    let script = TempScript::new("deny-long", WITH_LINTS);
    let output = run(&["--deny-warnings", "--lint", script.as_arg()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("error[W0001]"));
}

#[test]
fn json_format_goes_to_stdout_one_object_per_line() {
    let script = TempScript::new("json", WITH_LINTS);
    let output = run(&["--lint", "--error-format=json", script.as_arg()]);

    assert!(output.status.success());
    let out = stdout(&output);
    let line = out.lines().next().expect("one diagnostic");
    assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
    assert!(line.contains("\"code\":{\"code\":\"W0001\""), "{line}");
    assert!(line.contains("\"file_name\""), "{line}");
    assert!(
        stderr(&output).is_empty(),
        "json mode must keep stderr clean on warnings"
    );
}

#[test]
fn a_syntax_error_fails_lint_mode_even_without_lints() {
    let script = TempScript::new("syntax", "$x = )\n");
    let output = run(&["--lint", script.as_arg()]);

    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("error[E0002]"), "{err}");
    assert!(
        !err.contains("W0001"),
        "lints must not run on a program that did not parse: {err}"
    );
}

#[test]
fn allow_by_group_silences_that_group_only() {
    let source = "$unused = 1\nfor $i in 0..3 {\n}\n";
    let script = TempScript::new("allow-group", source);
    let output = run(&["-A", "style", "--lint", script.as_arg()]);

    let err = stderr(&output);
    assert!(!err.contains("W0001"), "style must be silenced: {err}");
    assert!(err.contains("W0003"), "suspicious must still report: {err}");
}

#[test]
fn rules_can_be_selected_by_their_kebab_case_name() {
    let script = TempScript::new("kebab", WITH_LINTS);
    let output = run(&["-A", "unused-binding", "--lint", script.as_arg()]);

    assert!(output.status.success());
    assert!(!stderr(&output).contains("W0001"), "{}", stderr(&output));
}

#[test]
fn an_unknown_lint_target_fails_immediately() {
    let script = TempScript::new("unknown-target", WITH_LINTS);
    let output = run(&["-A", "nonsense", "--lint", script.as_arg()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown lint: `nonsense`"));
}

#[test]
fn list_lints_prints_the_catalogue() {
    let output = run(&["--list-lints"]);

    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("W0001"), "{out}");
    assert!(out.contains("unused_binding"), "{out}");
    assert!(out.contains("Groups:"), "{out}");
}

#[test]
fn explain_accepts_a_code_or_a_name() {
    for target in ["W0001", "w0001", "unused_binding"] {
        let output = run(&["--explain", target]);
        assert!(output.status.success(), "{target} should be known");
        let out = stdout(&output);
        assert!(out.contains("# W0001 (unused_binding)"), "{target}: {out}");
    }
}

#[test]
fn explain_rejects_an_unknown_code() {
    let output = run(&["--explain", "E9999"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown lint"));
}

#[test]
fn lint_mode_accepts_an_eval_expression() {
    let output = run(&["--lint", "-e", "$unused = 1"]);

    assert!(output.status.success());
    assert!(stderr(&output).contains("warning[W0001]"));
}

#[test]
fn running_a_clean_program_still_prints_its_value() {
    let output = run(&["-e", "print \"hi\""]);

    assert!(output.status.success());
    assert_eq!(stdout(&output).trim(), "hi");
    assert!(
        stderr(&output).is_empty(),
        "running is not linting: {}",
        stderr(&output)
    );
}

#[test]
fn list_features_reports_the_compiled_std() {
    let output = run(&["--list-features"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("std"), "{out}");
    assert!(!out.contains("std_util"), "{out}");
}

#[test]
fn list_command_shows_plugins_and_their_domains() {
    let output = run(&["list"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("PLUGIN"), "{out}");
    assert!(out.contains("std"), "{out}");
    assert!(out.contains("util"), "{out}");
    assert!(out.contains("PERMISSIONS"), "{out}");
    assert!(out.contains("Active profile: trusted"), "{out}");
    assert!(out.contains("print, input"), "{out}");
}

#[test]
fn abi_command_lists_the_c_exports() {
    let output = run(&["abi"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("XCL toolchain ABI v7"), "{out}");
    assert!(out.contains("xcl_toolchain_new"), "{out}");
    assert!(out.contains("xcl_toolchain_set_features"), "{out}");
    assert!(out.contains("xcl_toolchain_set_profile"), "{out}");
    assert!(out.contains("xcl_toolchain_list_plugins"), "{out}");
    assert!(out.contains("xcl_toolchain_manifest"), "{out}");
}

#[test]
fn manifest_command_prints_generated_toml() {
    let output = run(&["manifest"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("[[plugins]]"), "{out}");
    assert!(out.contains("name = \"std\""), "{out}");
    assert!(out.contains("name = \"util.print\""), "{out}");
    assert!(out.contains("name = \"util.input\""), "{out}");
    assert!(out.contains("permissions = [\"console\"]"), "{out}");
    assert!(out.contains("[manifest]"), "{out}");
}

#[test]
fn the_std_plugin_can_be_installed_explicitly() {
    let output = run(&[
        "--no-default-features",
        "--features",
        "std",
        "-e",
        "print \"util\"",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("util"), "{}", stdout(&output));

    // The `util` domain is not a separate feature: one plugin, one switch.
    let output = run(&["--no-default-features", "--features", "util", "-e", "1"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown feature `util`"));
}

#[test]
fn a_sandbox_profile_still_runs_the_console() {
    let output = run(&["--profile", "sandbox", "-e", "print \"sandboxed\""]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "sandboxed");
}

#[test]
fn deny_all_refuses_a_plugin_that_needs_the_console() {
    let output = run(&["--profile", "deny-all", "-e", "print \"nope\""]);

    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let err = stderr(&output);
    assert!(err.contains("`deny-all` profile"), "{err}");
    assert!(err.contains("`std`"), "{err}");
    assert!(err.contains("console"), "{err}");
    assert!(err.contains("--profile trusted"), "{err}");
}

#[test]
fn an_unknown_profile_names_the_alternatives() {
    let output = run(&["--profile", "paranoid", "-e", "1"]);

    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("unknown profile `paranoid`"), "{err}");
    assert!(err.contains("sandbox"), "{err}");
    assert!(err.contains("trusted"), "{err}");
}

#[test]
fn a_directory_with_a_manifest_runs_its_entry_script() {
    let plugin = TempPlugin::new(
        "minimal",
        "[plugin]\nname = \"greet\"\n",
        "print \"hello from plugin\"\n",
    );
    let output = run(&[plugin.as_arg()]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "hello from plugin");
}

#[test]
fn an_explicit_profile_beats_the_manifest() {
    let plugin = TempPlugin::new(
        "trusted-run",
        "[plugin]\nname = \"greet\"\nprofile = \"trusted\"\n",
        "print \"ok\"\n",
    );

    let output = run(&["--profile", "deny-all", plugin.as_arg()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("`deny-all` profile"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_broken_manifest_fails_with_the_file_and_line() {
    let plugin = TempPlugin::new(
        "broken",
        "[plugin]\nname = \"greet\"\nwat = 1\n",
        "print \"unreachable\"\n",
    );
    let output = run(&[plugin.as_arg()]);

    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("plugin.toml:3"), "{err}");
    assert!(err.contains("unknown key `wat`"), "{err}");
}

#[test]
fn a_missing_manifest_explains_what_a_plugin_directory_needs() {
    let mut dir = std::env::temp_dir();
    dir.push(format!("xcl-empty-plugin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create dir");

    let output = Command::new(BIN)
        .arg(&dir)
        .env("XCL_TOOLCHAIN", runtime_path())
        .output()
        .expect("run xcl");
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("plugin.toml"), "{err}");
    assert!(err.contains("plugin directory"), "{err}");
}

#[test]
fn feature_names_are_exact() {
    let output = run(&["--features", "std_util", "-e", "print \"hi\""]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown feature `std_util`"));
}

#[test]
fn no_default_features_leaves_a_core_only_runtime() {
    let output = run(&["--no-default-features", "-e", "1 + 2"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "3");

    // `print` comes from the `std` feature, so it must be absent here.
    let output = run(&["--no-default-features", "-e", "print \"hi\""]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("hi"), "{}", stdout(&output));
}

#[test]
fn an_unknown_feature_fails_before_running() {
    let output = run(&["--features", "nope", "-e", "1"]);

    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("unknown feature `nope`"), "{err}");
}

#[test]
fn compile_writes_bytecode_next_to_the_source() {
    let script = TempScript::new("compile-sdk", "print \"compiled\"\n");
    let compiled = script.path.with_extension("xclb");

    let output = run(&["--compile", script.as_arg()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        compiled.exists(),
        "--compile must write {} next to the source",
        compiled.display()
    );

    // The toolchain path loads the file without compiling, so std is available.
    let output = run(&[compiled.to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "compiled");

    let _ = fs::remove_file(&compiled);
}

#[test]
fn the_bare_engine_runs_core_bytecode() {
    let script = TempScript::new("engine", "1 + 2\n");
    let compiled = script.path.with_extension("xclb");

    let output = run(&["--compile", script.as_arg()]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = run(&["--rt", compiled.to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "3");

    let _ = fs::remove_file(&compiled);
}

#[test]
fn the_engine_refuses_a_corrupted_file() {
    let script = TempScript::new("corrupt", "1 + 2\n");
    let compiled = script.path.with_extension("xclb");
    let output = run(&["--compile", script.as_arg()]);
    assert!(output.status.success(), "{}", stderr(&output));

    let mut bytes = fs::read(&compiled).expect("read bytecode");
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    fs::write(&compiled, &bytes).expect("write corrupted bytecode");

    let output = run(&["--rt", compiled.to_str().expect("utf-8 path")]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("corrupted"), "{err}");

    let _ = fs::remove_file(&compiled);
}

#[test]
fn abi_rt_lists_the_engine_exports() {
    let output = run(&["abi", "--rt"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("XCL Engine ABI v1"), "{out}");
    assert!(out.contains("xcl_rt_abi_version"), "{out}");
    assert!(out.contains("xcl_rt_load"), "{out}");
    assert!(out.contains("xcl_rt_run"), "{out}");
    assert!(!out.contains("xcl_toolchain_"), "{out}");
}

#[test]
fn compile_and_rt_are_mutually_exclusive() {
    let output = run(&["--compile", "--rt", "anything.xcl"]);

    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("--compile cannot be used with --rt"), "{err}");
}
