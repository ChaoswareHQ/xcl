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
//! Every emitted code has an entry in [`RULES`], reachable with [`explain`]. That
//! entry carries the reason the rule exists plus a bad and a good example, which
//! is what makes a diagnostic actionable without the reader knowing the rule.
//!
//! # Whole-program analysis
//!
//! Lints are independent and run over the whole program, so one pass reports
//! every finding rather than stopping at the first. A syntax error is different:
//! the parser aborts, and analysis cannot run on a program that does not exist.
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
pub use rule::{Resolution, RULES, Rule, render_catalogue, resolve_name, rule, rule_by_name};
pub use source::{SourceMap, SpanRange};

/// Parses `file` and runs every registered lint.
pub fn diagnostics(file: &str, source: &str) -> Report {
    let tokens = xcl_vm::lexer::lex(source, file);
    let ast = xcl_vm::parser::parse(&tokens, source);
    run(file, source, &ast)
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
pub fn explain(code: &str) -> Option<String> {
    rule::rule(code).map(Rule::render)
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
}
