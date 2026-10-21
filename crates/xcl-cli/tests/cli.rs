//! End-to-end tests for the lint surface of the `xcl` binary.
//!
//! These run the real binary, so they cover argument parsing, exit codes, and
//! which stream each format goes to — the parts a unit test cannot see.

#![cfg(feature = "lint")]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_xcl");

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
    Command::new(BIN).args(args).output().expect("run xcl")
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
