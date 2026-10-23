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
            "xcl_rt.dll"
        } else if cfg!(target_os = "macos") {
            "libxcl_rt.dylib"
        } else {
            "libxcl_rt.so"
        };
        let path = dir.join(name);
        if !path.exists() {
            let profile = dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("debug");
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut build = Command::new(cargo);
            build.args(["build", "-p", "xcl-runtime"]);
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
            let status = build.status().expect("build xcl-runtime");
            assert!(status.success(), "cargo build -p xcl-runtime failed");
        }
        assert!(path.exists(), "runtime not found at {}", path.display());
        path
    })
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

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("XCL_RUNTIME", runtime_path())
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
    assert!(out.contains("print, input"), "{out}");
}

#[test]
fn abi_command_lists_the_c_exports() {
    let output = run(&["abi"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("XCL runtime ABI v4"), "{out}");
    assert!(out.contains("xcl_rt_new"), "{out}");
    assert!(out.contains("xcl_rt_set_features"), "{out}");
    assert!(out.contains("xcl_rt_list_plugins"), "{out}");
    assert!(out.contains("xcl_rt_manifest"), "{out}");
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
