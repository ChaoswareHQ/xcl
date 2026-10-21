//! Static analysis for XCL: diagnostics, lints, and the report format.
//!
//! No dependencies beyond `xcl-vm` (for the AST and `Span`). Everything else —
//! JSON, rendering, the rule catalogue — is written here.
//!
//! # Lint levels vs diagnostic levels
//!
//! There are two distinct concepts:
//!
//! * **Lint levels** ([`LintLevel`]) control whether a lint fires at all.
//!   `Allow` suppresses the diagnostic entirely; `Forbid` makes the suppression
//!   sticky so later overrides cannot change it.
//! * **Diagnostic levels** ([`Level`]) control how a finding is reported once it
//!   fires. `Warning` and `Error` are the two reportable severities; `Note` and
//!   `Help` are context attached to a finding.
//!
//! The mapping is: `Warn` → `Level::Warning`, `Deny` → `Level::Error`.
//!
//! # Two consumers, one model
//!
//! A [`Diagnostic`] feeds two renderings that never have to agree on formatting:
//!
//! ```text
//! warning[W0001]: `$x` is assigned but never read
//!  --> script.xcl:2:1
//!   |
//! 2 | $x = 1
//!   | ^^ the value is never used
//!   |
//!   = help: remove the assignment or use `$x`
//! ```
//!
//! ```json
//! {"level":"warning","code":{"code":"W0001","explanation":"…"},"message":"…",
//!  "rendered":"…","spans":[{"file_name":"script.xcl",
//!  "byte_start":8,"byte_end":10,"line_start":2,"column_start":1,"is_primary":true,…}],
//!  "children":[{"level":"help","message":"…"}],"suggestions":[]}
//! ```
//!
//! The human form is for reading. The JSON form — one object per line, byte
//! offsets included — is the contract tools and agents use. Nothing parses the
//! human form.
//!
//! # Rule catalogue
//!
//! Every lint code has an entry in [`RULES`], reachable with [`explain`]. That
//! entry carries the reason the rule exists plus a bad and a good example, which
//! is what makes a diagnostic actionable without the reader knowing the rule.
//! Syntax errors are not lints: they use the parser's own codes (`E0001`…, listed
//! in [`xcl_vm::PARSE_ERROR_CODES`]) and have no catalogue entry.
//!
//! # Whole-program analysis
//!
//! Lints are independent and run over the whole program, so one pass reports
//! every finding rather than stopping at the first. Syntax errors are reported
//! the same way — [`check_source`] returns all of them — but lints do not run
//! when the program does not parse, because a half-built AST turns a lint into a
//! false positive. This mirrors rustc, which stops before analysis.
//!
//! # Example
//!
//! ```
//! use xcl_analysis::{diagnostics, SourceMap};
//!
//! let source = "$unused = 1";
//! let report = diagnostics("script.xcl", source);
//! assert_eq!(report.diagnostics.len(), 1);
//!
//! let map = SourceMap::new("script.xcl", source);
//! let human = report.render_human(&map);
//! assert!(human.contains("warning[W0001]"));
//! ```

pub mod diagnostic;
pub mod level;
pub mod lint;
pub mod rule;
pub mod source;

pub use diagnostic::{
    Applicability, Child, Diagnostic, Edit, Label, Report, Suggestion, json_string,
};
pub use level::{Group, Level, LintConfig, LintLevel, Override};
pub use lint::{LINTS, Lint, LintContext, walk};
pub use rule::{
    RULES, Resolution, Rule, is_known_target, normalize_target, render_catalogue, resolve_name,
    rule, rule_by_name,
};
pub use source::{SourceMap, SpanRange};

/// Parses `file` and runs every registered lint with the default configuration.
pub fn diagnostics(file: &str, source: &str) -> Report {
    check_source(file, source, &LintConfig::new())
}

/// Parses `source`, reports every syntax error, and otherwise runs every lint.
///
/// Syntax errors come back as `error` diagnostics carrying the parser's codes
/// (`E0001`…). When there is at least one, the lints are skipped: the AST is
/// partial, and a lint reading a half-built program reports noise rather than
/// findings.
///
/// # Example
///
/// ```
/// use xcl_analysis::{SourceMap, check_source, LintConfig};
///
/// let source = "if $x > { print 1 }";
/// let report = check_source("broken.xcl", source, &LintConfig::new());
/// assert!(report.has_errors());
///
/// let map = SourceMap::new("broken.xcl", source);
/// assert!(report.render_human(&map).contains("error[E"));
/// ```
pub fn check_source(file: &str, source: &str, config: &LintConfig) -> Report {
    let tokens = xcl_vm::lexer::lex(source, file);
    let out = xcl_vm::parser::parse_recover(&tokens, source);
    if out.errors.is_empty() {
        return run_with(file, source, &out.ast, config);
    }
    parse_error_report(&out.errors)
}

/// Builds a report from syntax errors the caller already collected.
///
/// This is for callers that parse anyway — the CLI does when it runs a program —
/// so a failed run can emit the same diagnostic contract `--lint` emits without
/// parsing the source twice.
pub fn parse_error_report(errors: &[xcl_vm::ParseError]) -> Report {
    let mut report = Report::default();
    for error in errors {
        report.push(parse_error_diagnostic(error));
    }
    report.sort();
    report
}

/// Turns a parser error into a diagnostic at the token it stopped on.
///
/// Only the start of the token is known, so the label underlines one character;
/// that is still enough to point a reader or a tool at the right place.
fn parse_error_diagnostic(error: &xcl_vm::ParseError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(error.code, error.message.clone())
        .label(Label::primary(SpanRange::new(&error.span, 1), ""));
    if let Some(help) = &error.help {
        diagnostic = diagnostic.help(help.clone());
    }
    diagnostic
}

/// Runs every registered lint over an already-parsed program.
pub fn run(file: &str, source: &str, ast: &[xcl_vm::AstNode]) -> Report {
    let map = SourceMap::new(file, source);
    let cx = LintContext { map: &map, ast };
    let mut report = Report::default();
    for lint in LINTS {
        let rule = lint.rule();
        for diagnostic in lint.check(&cx) {
            // Default config: no overrides, so use the rule's default level.
            // Allow/Forbid map to diagnostic level; only Allow suppresses.
            if rule.default_level == LintLevel::Allow {
                continue;
            }
            let mut d = diagnostic;
            d.level = rule.default_level.to_diagnostic();
            report.push(d);
        }
    }
    report.sort();
    report
}

/// Runs every registered lint with a configuration applied.
pub fn run_with(file: &str, source: &str, ast: &[xcl_vm::AstNode], config: &LintConfig) -> Report {
    let map = SourceMap::new(file, source);
    let cx = LintContext { map: &map, ast };
    let mut report = Report::default();
    for lint in LINTS {
        let rule = lint.rule();
        for mut diagnostic in lint.check(&cx) {
            // apply returns None when the rule is allowed (caller drops the diagnostic)
            if config.apply(&mut diagnostic, rule).is_some() {
                report.push(diagnostic);
            }
        }
    }
    report.sort();
    report
}

/// Renders the documentation for one rule code, or `None` if it is unknown.
///
/// Syntax error codes (`E0001`…) are documented too, from the parser's own
/// catalogue, so `--explain` works on a code the user saw in a diagnostic.
pub fn explain(code: &str) -> Option<String> {
    if let Some(rule) = rule::rule(code) {
        return Some(rule.render());
    }
    parse_error_explanation(code)
}

/// Documentation for a parser error code, from [`xcl_vm::PARSE_ERROR_CODES`].
fn parse_error_explanation(code: &str) -> Option<String> {
    let (code, summary) = xcl_vm::PARSE_ERROR_CODES
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(code))?;

    let mut out = format!("# {code}\n\n## What it does\n\n{summary}\n");
    out.push_str(
        "\n## Emitted by\n\nThe parser, when it cannot build a statement. Parsing \
         recovers from the error and keeps going, so one run reports every \
         syntax error in the file rather than only the first.\n",
    );
    out.push_str(
        "\n## See also\n\n- `xcl --list-lints` lists the lint rules, which are \
         separate and only run on a program that parses.\n",
    );
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_programs_are_clean() {
        let report = diagnostics("test.xcl", "print \"hello\"");
        assert!(report.is_empty(), "{:?}", report.diagnostics);
        assert!(!report.has_errors());
    }

    #[test]
    fn every_lint_name_is_unique() {
        let mut names: Vec<&str> = LINTS.iter().map(|l| l.rule().name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
    }

    #[test]
    fn catalogue_and_lints_agree() {
        for lint in LINTS {
            let code = lint.rule().code;
            assert!(rule::rule(code).is_some(), "{code} is missing from RULES");
        }
        for entry in RULES {
            assert!(
                LINTS.iter().any(|l| l.rule().code == entry.code),
                "{} is catalogued but has no lint",
                entry.code
            );
        }
    }

    #[test]
    fn explain_returns_guidance() {
        let text = explain("W0001").expect("W0001 is known");
        assert!(text.contains("unused_binding"));
        assert!(text.contains("## What it does"));
        assert!(explain("E9999").is_none());
    }

    #[test]
    fn explain_covers_parser_error_codes() {
        let text = explain("E0001").expect("E0001 is documented by the parser");
        assert!(text.contains("expected token not found"), "{text}");
        assert!(
            explain("e0002").is_some(),
            "error codes are matched case-insensitively"
        );
    }

    #[test]
    fn syntax_errors_are_reported_instead_of_panicking() {
        let source = "if $x > { print 1 }";
        let report = check_source("broken.xcl", source, &LintConfig::new());
        assert!(report.has_errors(), "a syntax error is an error diagnostic");
        assert!(
            report.diagnostics.iter().all(|d| d.code.starts_with('E')),
            "lints must not run on a program that did not parse: {:?}",
            report.diagnostics
        );
        let map = SourceMap::new("broken.xcl", source);
        let human = report.render_human(&map);
        assert!(human.contains("error[E"), "{human}");
        assert!(human.contains(" --> broken.xcl:1:"), "{human}");
    }

    #[test]
    fn every_syntax_error_is_reported() {
        let source = "$a = )\n$b = ]";
        let report = check_source("broken.xcl", source, &LintConfig::new());
        assert!(
            report.diagnostics.len() >= 2,
            "expected one diagnostic per bad line, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn configured_severity_reaches_the_report() {
        let source = "$unused = 1";
        let config = LintConfig::new().set("unused_binding", LintLevel::Deny);
        let report = check_source("script.xcl", source, &config);
        assert!(report.has_errors());
        assert_eq!(report.diagnostics[0].level, Level::Error);

        let silenced = LintConfig::new().set("style", LintLevel::Allow);
        assert!(
            check_source("script.xcl", source, &silenced).is_empty(),
            "a group override must silence the rule"
        );
    }

    #[test]
    fn secondary_group_reaches_the_rule() {
        let source = "$a = 1\n$b = 2\nif not ($a == $b) { print \"x\" }";
        let report = check_source("script.xcl", source, &LintConfig::new());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "W0006"),
            "precondition: negated_comparison fires"
        );

        // `pedantic` is a secondary group of W0006, not its primary group.
        let config = LintConfig::new().set("pedantic", LintLevel::Allow);
        let report = check_source("script.xcl", source, &config);
        assert!(report.diagnostics.iter().all(|d| d.code != "W0006"));
    }
}
