//! Diagnostics, suggestions, and the two renderings.
//!
//! One [`Diagnostic`] feeds two outputs:
//!
//! * [`Report::render_human`] — gutter and carets, for people.
//! * [`Report::render_json`] — one JSON object per line, for tools and agents.
//!
//! Nothing ever parses the human form. The JSON form is the contract: it carries
//! stable codes, byte offsets, and machine-applicable replacements.

use crate::level::Level;
use crate::source::{SourceMap, SpanRange};

/// How safe a suggested fix is.
///
/// Only [`Applicability::MachineApplicable`] may be applied without a human
/// reading the diff; tooling (and agents) should treat the rest as hints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applicability {
    /// Safe to apply automatically.
    MachineApplicable,
    /// Probably right, but the change may not compile or may alter behaviour.
    MaybeIncorrect,
    /// Contains placeholders the author must fill in.
    HasPlaceholders,
    /// No claim is made about correctness.
    Unspecified,
}

impl Applicability {
    /// Stable name used in JSON.
    pub fn name(self) -> &'static str {
        match self {
            Applicability::MachineApplicable => "machine-applicable",
            Applicability::MaybeIncorrect => "maybe-incorrect",
            Applicability::HasPlaceholders => "has-placeholders",
            Applicability::Unspecified => "unspecified",
        }
    }
}

/// One source edit inside a [`Suggestion`].
#[derive(Clone, Debug)]
pub struct Edit {
    /// The text this edit replaces.
    pub range: SpanRange,
    /// The replacement text.
    pub replacement: String,
}

impl Edit {
    /// Replaces `range` with `replacement`.
    pub fn new(range: SpanRange, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }
}

/// A proposed change, possibly spanning several edits.
#[derive(Clone, Debug)]
pub struct Suggestion {
    /// Human-readable description of the change.
    pub message: String,
    /// The edits, applied together.
    pub edits: Vec<Edit>,
    /// How safe the change is.
    pub applicability: Applicability,
}

impl Suggestion {
    /// A machine-applicable single-edit fix.
    pub fn replace(
        range: SpanRange,
        replacement: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            message: message.into(),
            edits: vec![Edit::new(range, replacement)],
            applicability: Applicability::MachineApplicable,
        }
    }

    /// Marks the suggestion as possibly incorrect.
    pub fn maybe_incorrect(mut self) -> Self {
        self.applicability = Applicability::MaybeIncorrect;
        self
    }
}

/// A span with an attached explanation.
///
/// Exactly one label of a diagnostic should be primary: it is the location a
/// reader looks at first, and the one a tool jumps to.
#[derive(Clone, Debug)]
pub struct Label {
    /// Where the label points.
    pub range: SpanRange,
    /// What is wrong here.
    pub text: String,
    /// Whether this is the primary label.
    pub primary: bool,
}

impl Label {
    /// Creates a primary label.
    pub fn primary(range: SpanRange, text: impl Into<String>) -> Self {
        Self {
            range,
            text: text.into(),
            primary: true,
        }
    }

    /// Creates a secondary label, drawn with `-` instead of `^`.
    pub fn secondary(range: SpanRange, text: impl Into<String>) -> Self {
        Self {
            range,
            text: text.into(),
            primary: false,
        }
    }
}

/// A note or help attached to a diagnostic.
#[derive(Clone, Debug)]
pub struct Child {
    /// `Help` or `Note`.
    pub level: Level,
    /// The message.
    pub message: String,
}

impl Child {
    /// A `= help:` line: what to do about it.
    pub fn help(message: impl Into<String>) -> Self {
        Self {
            level: Level::Help,
            message: message.into(),
        }
    }

    /// A `= note:` line: context that is not an instruction.
    pub fn note(message: impl Into<String>) -> Self {
        Self {
            level: Level::Note,
            message: message.into(),
        }
    }
}

/// One finding about a program.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// Severity, after configuration is applied.
    pub level: Level,
    /// Stable rule code such as `W0001`. Never reused for a different meaning.
    pub code: &'static str,
    /// One line, no trailing period, phrased about the user's code.
    pub message: String,
    /// Locations, primary first.
    pub labels: Vec<Label>,
    /// Attached help and notes.
    pub children: Vec<Child>,
    /// Proposed fixes.
    pub suggestions: Vec<Suggestion>,
}

impl Diagnostic {
    /// Creates a warning diagnostic with no labels.
    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Level::Warning, code, message)
    }

    /// Creates an error diagnostic with no labels.
    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Level::Error, code, message)
    }

    /// Creates a diagnostic at `level`.
    pub fn new(level: Level, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            level,
            code,
            message: message.into(),
            labels: Vec::new(),
            children: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Adds a label.
    pub fn label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    /// Adds a `= help:` line.
    pub fn help(self, message: impl Into<String>) -> Self {
        self.child(Child::help(message))
    }

    /// Adds a `= note:` line.
    pub fn note(self, message: impl Into<String>) -> Self {
        self.child(Child::note(message))
    }

    /// Adds an arbitrary child.
    pub fn child(mut self, child: Child) -> Self {
        self.children.push(child);
        self
    }

    /// Adds a suggested fix.
    pub fn suggest(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    /// The primary label, if any.
    pub fn primary_label(&self) -> Option<&Label> {
        self.labels
            .iter()
            .find(|l| l.primary)
            .or_else(|| self.labels.first())
    }
}

/// A set of diagnostics for one program.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Findings, in the order the lints produced them until [`Report::sort`] runs.
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    /// Appends a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns `true` when at least one diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.level.is_failure())
    }

    /// Returns `true` when there is nothing to report.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Counts diagnostics by level.
    pub fn count(&self, level: Level) -> usize {
        self.diagnostics.iter().filter(|d| d.level == level).count()
    }

    /// Sorts by file, line, column, then code.
    ///
    /// Deterministic output matters: two runs over the same source must produce
    /// byte-identical reports, or every diff is noise.
    pub fn sort(&mut self) {
        self.diagnostics.sort_by(|a, b| {
            let key = |d: &Diagnostic| {
                d.primary_label()
                    .map(|l| (l.range.file.clone(), l.range.line, l.range.col, d.code))
                    .unwrap_or_else(|| (String::new(), 0, 0, d.code))
            };
            key(a).cmp(&key(b))
        });
    }

    /// One-line summary such as `2 warnings emitted`.
    pub fn summary(&self) -> String {
        let errors = self.count(Level::Error);
        let warnings = self.count(Level::Warning);
        if self.is_empty() {
            return "no diagnostics emitted".to_string();
        }
        let mut parts = Vec::new();
        if errors > 0 {
            parts.push(format!("{errors} error{}", plural(errors)));
        }
        if warnings > 0 {
            parts.push(format!("{warnings} warning{}", plural(warnings)));
        }
        format!("{} emitted", parts.join(" and "))
    }

    /// Renders the compiler-style view.
    pub fn render_human(&self, map: &SourceMap<'_>) -> String {
        let mut out = String::new();
        for diagnostic in &self.diagnostics {
            render_one(&mut out, diagnostic, map);
        }
        if !self.is_empty() {
            out.push_str(&self.summary());
            out.push('\n');
        }
        out
    }

    /// Renders one JSON object per diagnostic, newline-delimited.
    ///
    /// The schema is stable: `level`, `code`, `message`, `spans`, `children`,
    /// `suggestions`.
    pub fn render_json(&self, map: &SourceMap<'_>) -> String {
        let mut out = String::new();
        for diagnostic in &self.diagnostics {
            render_json_object(&mut out, diagnostic, map);
            out.push('\n');
        }
        out
    }
}

impl core::fmt::Display for Report {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for diagnostic in &self.diagnostics {
            writeln!(
                f,
                "{}[{}]: {}",
                diagnostic.level.label(),
                diagnostic.code,
                diagnostic.message
            )?;
        }
        Ok(())
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn render_one(out: &mut String, d: &Diagnostic, map: &SourceMap<'_>) {
    out.push_str(&format!("{}[{}]: {}\n", d.level.label(), d.code, d.message));

    let Some(primary) = d.primary_label() else {
        render_children(out, d);
        out.push('\n');
        return;
    };

    out.push_str(&format!(
        " --> {}:{}:{}\n",
        primary.range.file, primary.range.line, primary.range.col
    ));

    let Some(line_text) = map.line(primary.range.line) else {
        render_children(out, d);
        out.push('\n');
        return;
    };

    let gutter_width = primary.range.line.to_string().len().max(1);
    let pad = " ".repeat(gutter_width);
    out.push_str(&format!("{pad} |\n"));
    out.push_str(&format!("{} | {}\n", primary.range.line, line_text));

    for label in &d.labels {
        if label.range.line != primary.range.line {
            continue;
        }
        let indent = " ".repeat(label.range.col.saturating_sub(1) as usize);
        let marker = if label.primary { '^' } else { '-' };
        let carets = marker.to_string().repeat(label.range.len as usize);
        let text = if label.text.is_empty() {
            String::new()
        } else {
            format!(" {}", label.text)
        };
        out.push_str(&format!("{pad} | {indent}{carets}{text}\n"));
    }
    out.push_str(&format!("{pad} |\n"));

    render_children(out, d);
    out.push('\n');
}

fn render_children(out: &mut String, d: &Diagnostic) {
    for child in &d.children {
        out.push_str(&format!("  = {}: {}\n", child.level.label(), child.message));
    }
    for suggestion in &d.suggestions {
        out.push_str(&format!(
            "  = help: {} ({})\n",
            suggestion.message,
            suggestion.applicability.name()
        ));
    }
}

fn render_json_object(out: &mut String, d: &Diagnostic, map: &SourceMap<'_>) {
    out.push('{');
    out.push_str(&format!("\"level\":{}", json_string(d.level.label())));

    // Code: emit as object with code and explanation
    out.push_str(",\"code\":{");
    out.push_str(&format!("\"code\":{}", json_string(d.code)));
    if let Some(rule) = crate::rule::rule(d.code) {
        out.push_str(&format!(
            ",\"explanation\":{}",
            json_string(rule.explanation)
        ));
    } else {
        out.push_str(",\"explanation\":null");
    }
    out.push('}');

    out.push_str(&format!(",\"message\":{}", json_string(&d.message)));

    // Rendered: human rendering of this single diagnostic without colour and without trailing summary
    let rendered = render_one_human(d, map);
    out.push_str(&format!(",\"rendered\":{}", json_string(&rendered)));

    out.push_str(",\"spans\":[");
    for (i, label) in d.labels.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let (byte_start, byte_end) = map.byte_range(&label.range).unwrap_or((0, 0));
        out.push('{');
        out.push_str(&format!("\"file_name\":{}", json_string(&label.range.file)));
        out.push_str(&format!(",\"byte_start\":{byte_start}"));
        out.push_str(&format!(",\"byte_end\":{byte_end}"));
        out.push_str(&format!(",\"line_start\":{}", label.range.line));
        out.push_str(&format!(",\"line_end\":{}", label.range.line));
        out.push_str(&format!(",\"column_start\":{}", label.range.col));
        out.push_str(&format!(",\"column_end\":{}", label.range.end_col()));
        out.push_str(&format!(",\"is_primary\":{}", label.primary));
        out.push_str(&format!(",\"label\":{}", json_string(&label.text)));
        out.push('}');
    }
    out.push(']');

    out.push_str(",\"children\":[");
    for (i, child) in d.children.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str(&format!("\"level\":{}", json_string(child.level.label())));
        out.push_str(&format!(",\"message\":{}", json_string(&child.message)));
        out.push('}');
    }
    out.push(']');

    out.push_str(",\"suggestions\":[");
    for (i, suggestion) in d.suggestions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str(&format!("\"message\":{}", json_string(&suggestion.message)));
        out.push_str(&format!(
            ",\"applicability\":{}",
            json_string(suggestion.applicability.name())
        ));
        out.push_str(",\"edits\":[");
        for (j, edit) in suggestion.edits.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            let (byte_start, byte_end) = map.byte_range(&edit.range).unwrap_or((0, 0));
            out.push('{');
            out.push_str(&format!("\"byte_start\":{byte_start}"));
            out.push_str(&format!(",\"byte_end\":{byte_end}"));
            out.push_str(&format!(
                ",\"replacement\":{}",
                json_string(&edit.replacement)
            ));
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("]}");
}

/// Renders a single diagnostic in human form without colour and without the
/// trailing summary line. Used for the `"rendered"` JSON field.
fn render_one_human(d: &Diagnostic, map: &SourceMap<'_>) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}[{}]: {}\n", d.level.label(), d.code, d.message));

    let Some(primary) = d.primary_label() else {
        render_children(&mut out, d);
        return out;
    };

    out.push_str(&format!(
        " --> {}:{}:{}\n",
        primary.range.file, primary.range.line, primary.range.col
    ));

    if let Some(line_text) = map.line(primary.range.line) {
        let gutter_width = primary.range.line.to_string().len().max(1);
        let pad = " ".repeat(gutter_width);
        out.push_str(&format!("{pad} |\n"));
        out.push_str(&format!("{} | {}\n", primary.range.line, line_text));

        for label in &d.labels {
            if label.range.line != primary.range.line {
                continue;
            }
            let indent = " ".repeat(label.range.col.saturating_sub(1) as usize);
            let marker = if label.primary { '^' } else { '-' };
            let carets = marker.to_string().repeat(label.range.len as usize);
            let text = if label.text.is_empty() {
                String::new()
            } else {
                format!(" {}", label.text)
            };
            out.push_str(&format!("{pad} | {indent}{carets}{text}\n"));
        }
        out.push_str(&format!("{pad} |\n"));
    }

    render_children(&mut out, d);
    out
}

/// Escapes `s` as a JSON string literal.
///
/// Hand-written so the crate has no dependencies; covers quotes, backslashes,
/// and control characters.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcl_vm::Span;

    fn range(line: u32, col: u32, len: u32) -> SpanRange {
        SpanRange::new(&Span::new("script.xcl".to_string(), line, col), len)
    }

    fn report_with_warning() -> Report {
        let mut report = Report::default();
        report.push(
            Diagnostic::warning("W0001", "`$x` is assigned but never read")
                .label(Label::primary(range(2, 1, 2), "the value is never used"))
                .help("remove the assignment or use `$x`")
                .suggest(Suggestion::replace(
                    range(2, 1, 2),
                    "",
                    "remove the binding",
                )),
        );
        report
    }

    #[test]
    fn human_rendering_has_a_gutter_and_carets() {
        let map = SourceMap::new("script.xcl", "print 1\n$x = 1\n");
        let text = report_with_warning().render_human(&map);
        assert!(text.contains("warning[W0001]: `$x` is assigned but never read"));
        assert!(text.contains(" --> script.xcl:2:1"));
        assert!(text.contains("2 | $x = 1"));
        assert!(text.contains("  | ^^ the value is never used"));
        assert!(text.contains("= help: remove the assignment or use `$x`"));
        assert!(text.contains("1 warning emitted"));
    }

    #[test]
    fn json_is_one_object_per_line_with_offsets() {
        let map = SourceMap::new("script.xcl", "print 1\n$x = 1\n");
        let json = report_with_warning().render_json(&map);
        assert_eq!(json.lines().count(), 1);
        assert!(json.contains("\"level\":\"warning\""));
        // Code is now an object
        assert!(json.contains("\"code\":{\"code\":\"W0001\""));
        assert!(json.contains("\"explanation\":"));
        assert!(json.contains("\"byte_start\":8"));
        assert!(json.contains("\"byte_end\":10"));
        assert!(json.contains("\"is_primary\":true"));
        assert!(json.contains("\"applicability\":\"machine-applicable\""));
    }

    #[test]
    fn json_rendered_field_present_and_contains_arrow() {
        let map = SourceMap::new("script.xcl", "print 1\n$x = 1\n");
        let json = report_with_warning().render_json(&map);
        // The "rendered" field must be present
        assert!(json.contains("\"rendered\":"));
        // The rendered content includes the --> location marker (JSON-escaped as \" -->)
        assert!(json.contains(" --> script.xcl:2:1"));
    }

    #[test]
    fn json_code_object_has_null_explanation_for_unknown_code() {
        let mut report = Report::default();
        report.push(
            Diagnostic::warning("W9999", "unknown rule")
                .label(Label::primary(range(1, 1, 1), "test")),
        );
        let map = SourceMap::new("script.xcl", "x");
        let json = report.render_json(&map);
        assert!(json.contains("\"code\":{\"code\":\"W9999\",\"explanation\":null}"));
    }

    #[test]
    fn json_escaping_handles_quotes_and_newlines() {
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn reports_sort_by_position_then_code() {
        let mut report = Report::default();
        report
            .push(Diagnostic::warning("W0002", "second").label(Label::primary(range(5, 1, 1), "")));
        report
            .push(Diagnostic::warning("W0001", "first").label(Label::primary(range(2, 1, 1), "")));
        report.sort();
        let order: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(order, vec!["W0001", "W0002"]);
    }

    #[test]
    fn only_errors_fail() {
        let mut report = Report::default();
        report.push(Diagnostic::warning("W0001", "minor"));
        assert!(!report.has_errors());
        report.push(Diagnostic::error("E0001", "broken"));
        assert!(report.has_errors());
    }
}
