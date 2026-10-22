//! The lint trait, the AST walker, and the rules themselves.
//!
//! A lint is a value that reads a [`LintContext`] and returns diagnostics. Rules
//! are stateless, synchronous, and independent: one rule's output never depends
//! on another's, which is what makes whole-program reporting safe.

use crate::diagnostic::{Diagnostic, Label, Suggestion};
use crate::rule::{self, Rule};
use crate::source::{SourceMap, SpanRange};
use xcl_vm::{AstNode, Span};

/// What a lint is allowed to look at.
pub struct LintContext<'a> {
    /// Token/line index for the file being analysed.
    pub map: &'a SourceMap<'a>,
    /// The parsed program.
    pub ast: &'a [AstNode],
}

impl<'a> LintContext<'a> {
    /// Creates a range for an identifier-sized span.
    pub fn name_range(&self, span: &Span, name: &str) -> SpanRange {
        SpanRange::of_name(span, name)
    }

    /// Creates a range covering `len` characters from `span`.
    pub fn range(&self, span: &Span, len: u32) -> SpanRange {
        SpanRange::new(span, len)
    }
}

/// A single rule.
///
/// `rule()` returns the catalogue entry so a diagnostic and its documentation
/// cannot drift apart.
pub trait Lint: Sync {
    /// The catalogue entry for this rule.
    fn rule(&self) -> &'static Rule;

    /// Runs the rule.
    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic>;
}

/// The registered rules.
pub static LINTS: &[&dyn Lint] = &[
    &UnusedBinding,
    &ConstantCondition,
    &EmptyBody,
    &UnusedParameter,
    &ShadowedBinding,
    &NegatedComparison,
];

/// Calls `f` for `node` and every node below it.
///
/// Variants this walker does not descend into are listed explicitly with a
/// comment, so a new AST node shows up as a compile error instead of being
/// silently skipped.
pub fn walk<'a>(node: &'a AstNode, f: &mut impl FnMut(&'a AstNode)) {
    f(node);
    match node {
        AstNode::Assign { value, .. } => walk(value, f),
        AstNode::Return { value, .. } => {
            if let Some(v) = value {
                walk(v, f);
            }
        }
        AstNode::Break { .. } | AstNode::Continue { .. } => {}
        AstNode::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk(condition, f);
            for stmt in then_branch {
                walk(stmt, f);
            }
            if let Some(branch) = else_branch {
                for stmt in branch {
                    walk(stmt, f);
                }
            }
        }
        AstNode::For { iterable, body, .. } => {
            walk(iterable, f);
            for stmt in body {
                walk(stmt, f);
            }
        }
        AstNode::While {
            condition, body, ..
        } => {
            walk(condition, f);
            for stmt in body {
                walk(stmt, f);
            }
        }
        AstNode::FunctionDef { params, body, .. } => {
            for param in params {
                if let Some(default) = &param.default_value {
                    walk(default, f);
                }
            }
            for stmt in body {
                walk(stmt, f);
            }
        }
        AstNode::Lambda { body, .. } => walk(body, f),
        AstNode::Call { callee, args, .. } => {
            walk(callee, f);
            for arg in args {
                walk(&arg.value, f);
            }
        }
        AstNode::DomainCall {
            callee,
            args,
            items,
            ..
        } => {
            walk(callee, f);
            for arg in args {
                walk(&arg.value, f);
            }
            for item in items {
                walk(&item.value, f);
                for (_, value) in &item.named {
                    walk(value, f);
                }
            }
        }
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. }
        | AstNode::SpecialVar { .. } => {}
        AstNode::BinaryOp { left, right, .. } => {
            walk(left, f);
            walk(right, f);
        }
        AstNode::UnaryOp { operand, .. } => walk(operand, f),
        AstNode::Pipeline { input, stages, .. } => {
            walk(input, f);
            for stage in stages {
                if let Some(arg) = &stage.arg {
                    walk(arg, f);
                }
            }
        }
        AstNode::ArrayLit { elements, .. } => {
            for element in elements {
                walk(element, f);
            }
        }
        AstNode::DictLit { entries, .. } => {
            for (_, value) in entries {
                walk(value, f);
            }
        }
        AstNode::Index { target, index, .. } => {
            walk(target, f);
            walk(index, f);
        }
        AstNode::FieldAccess { target, .. } => walk(target, f),
        AstNode::SetField { target, value, .. } => {
            walk(target, f);
            walk(value, f);
        }
        AstNode::SetIndex {
            target,
            index,
            value,
            ..
        } => {
            walk(target, f);
            walk(index, f);
            walk(value, f);
        }
        AstNode::Range { start, end, .. } => {
            if let Some(s) = start {
                walk(s, f);
            }
            if let Some(e) = end {
                walk(e, f);
            }
        }
        AstNode::Block { statements, .. } => {
            for stmt in statements {
                walk(stmt, f);
            }
        }
        AstNode::TypeDef { body, .. } => {
            for item in body {
                walk(item, f);
            }
        }
        AstNode::ImplDef { body, .. } => {
            for item in body {
                walk(item, f);
            }
        }
        AstNode::TypeTest { value, .. } => walk(value, f),
        AstNode::TypeCast { value, .. } => walk(value, f),
    }
}

/// Collects every identifier name used in the program.
fn used_names(ast: &[AstNode]) -> Vec<&str> {
    let mut used = Vec::new();
    for node in ast {
        walk(node, &mut |n| {
            if let AstNode::Ident { name, .. } = n {
                used.push(name.as_str());
            }
        });
    }
    used
}

/// Returns `true` when `node` contains a read of the identifier `name`.
fn contains_ident(node: &AstNode, name: &str) -> bool {
    let mut found = false;
    walk(node, &mut |n| {
        if let AstNode::Ident { name: n, .. } = n
            && n == name
        {
            found = true;
        }
    });
    found
}

/// `W0001` — a `$variable` is assigned but never read.
pub struct UnusedBinding;

impl Lint for UnusedBinding {
    fn rule(&self) -> &'static Rule {
        rule::rule("W0001").expect("W0001 is catalogued")
    }

    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic> {
        let used = used_names(cx.ast);
        let mut out = Vec::new();
        for node in cx.ast {
            walk(node, &mut |n| {
                let AstNode::Assign { span, name, .. } = n else {
                    return;
                };
                // Synthetic field nodes from type bodies are not user bindings.
                if name.starts_with("__field__") {
                    return;
                }
                if used.contains(&name.as_str()) {
                    return;
                }
                let range = cx.name_range(span, &format!("${name}"));
                out.push(
                    Diagnostic::warning(
                        self.rule().code,
                        format!("`${name}` is assigned but never read"),
                    )
                    .label(Label::primary(range, "the value is never used"))
                    .help(format!("remove the assignment or use `${name}`")),
                );
            });
        }
        out
    }
}

/// `W0002` — an `if` or `while` condition is a literal.
pub struct ConstantCondition;

fn literal_span(node: &AstNode) -> Option<&Span> {
    match node {
        AstNode::Bool { span, .. }
        | AstNode::Int { span, .. }
        | AstNode::Float { span, .. }
        | AstNode::Str { span, .. }
        | AstNode::Nil { span } => Some(span),
        _ => None,
    }
}

impl Lint for ConstantCondition {
    fn rule(&self) -> &'static Rule {
        rule::rule("W0002").expect("W0002 is catalogued")
    }

    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for node in cx.ast {
            walk(node, &mut |n| {
                let (what, condition) = match n {
                    AstNode::If { condition, .. } => ("if", condition.as_ref()),
                    AstNode::While { condition, .. } => ("while", condition.as_ref()),
                    _ => return,
                };
                let Some(span) = literal_span(condition) else {
                    return;
                };
                // `Span` has no length, so the literal's extent is read from the
                // line: everything up to the next space or brace.
                let len = cx
                    .map
                    .line(span.line)
                    .map(|line| {
                        line.chars()
                            .skip(span.col.saturating_sub(1) as usize)
                            .take_while(|c| !c.is_whitespace() && *c != '{')
                            .count()
                            .max(1)
                    })
                    .unwrap_or(1)
                    .max(1) as u32;
                out.push(
                    Diagnostic::warning(
                        self.rule().code,
                        format!("this `{what}` condition is always the same value"),
                    )
                    .label(Label::primary(
                        cx.range(span, len),
                        "decided before the program runs",
                    ))
                    .help("use a condition that depends on the program's values"),
                );
            });
        }
        out
    }
}

/// `W0003` — a block has no statements.
pub struct EmptyBody;

impl Lint for EmptyBody {
    fn rule(&self) -> &'static Rule {
        rule::rule("W0003").expect("W0003 is catalogued")
    }

    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for node in cx.ast {
            walk(node, &mut |n| {
                let (keyword, span, empty) = match n {
                    AstNode::If {
                        span, then_branch, ..
                    } => ("if", span, then_branch.is_empty()),
                    AstNode::While { span, body, .. } => ("while", span, body.is_empty()),
                    AstNode::For { span, body, .. } => ("for", span, body.is_empty()),
                    AstNode::FunctionDef { span, body, .. } => ("function", span, body.is_empty()),
                    _ => return,
                };
                if !empty {
                    return;
                }
                out.push(
                    Diagnostic::warning(
                        self.rule().code,
                        format!("this `{keyword}` body is empty"),
                    )
                    .label(Label::primary(
                        cx.range(span, keyword.len() as u32),
                        "nothing happens here",
                    ))
                    .help("remove the block, or add the statements it is missing"),
                );
            });
        }
        out
    }
}

/// `W0004` — a parameter is never read in the function body.
pub struct UnusedParameter;

impl Lint for UnusedParameter {
    fn rule(&self) -> &'static Rule {
        rule::rule("W0004").expect("W0004 is catalogued")
    }

    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for node in cx.ast {
            let AstNode::FunctionDef { params, body, .. } = node else {
                continue;
            };
            let mut used = Vec::new();
            for stmt in body {
                walk(stmt, &mut |n| {
                    if let AstNode::Ident { name, .. } = n {
                        used.push(name.as_str());
                    }
                });
            }
            for param in params {
                if param.name.starts_with('_') || used.contains(&param.name.as_str()) {
                    continue;
                }
                out.push(
                    Diagnostic::warning(
                        self.rule().code,
                        format!("parameter `${}` is never read", param.name),
                    )
                    .label(Label::primary(
                        cx.name_range(&param.span, &format!("${}", param.name)),
                        "never used in the body",
                    ))
                    .help("remove the parameter, or name it `$_` if it must stay")
                    .suggest(Suggestion::replace(
                        cx.name_range(&param.span, &format!("${}", param.name)),
                        format!("$_{}", param.name),
                        format!("prefix `{}` with `_`", param.name),
                    )),
                );
            }
        }
        out
    }
}

/// `W0005` — a `$variable` is assigned twice before being read.
pub struct ShadowedBinding;

impl Lint for ShadowedBinding {
    fn rule(&self) -> &'static Rule {
        rule::rule("W0005").expect("W0005 is catalogued")
    }

    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let mut last: Option<(&str, &Span)> = None;
        for node in cx.ast {
            match node {
                AstNode::Assign {
                    span, name, value, ..
                } => {
                    // `$x = $x + 1` reads the previous value, so it is not a
                    // discarded assignment.
                    let reads_self = contains_ident(value, name);
                    if let Some((prev_name, prev_span)) = last
                        && prev_name == name
                        && !reads_self
                    {
                        let len = name.chars().count() as u32 + 1;
                        out.push(
                            Diagnostic::warning(
                                self.rule().code,
                                format!("`${name}` is assigned twice without being read"),
                            )
                            .label(Label::primary(
                                cx.range(prev_span, len),
                                "this value is discarded",
                            ))
                            .help("remove the first assignment, or use a different name"),
                        );
                    }
                    last = Some((name.as_str(), span));
                }
                other => {
                    if let Some((prev_name, _)) = last
                        && contains_ident(other, prev_name)
                    {
                        last = None;
                    }
                }
            }
        }
        out
    }
}

/// `W0006` — a comparison is wrapped in `not`.
///
/// There is no machine-applicable fix: replacing `==` with `!=` needs the
/// operator's own position, and [`Span`] records only where an expression
/// starts. The diagnostic points at the start of the condition instead.
pub struct NegatedComparison;

impl Lint for NegatedComparison {
    fn rule(&self) -> &'static Rule {
        rule::rule("W0006").expect("W0006 is catalogued")
    }

    fn check(&self, cx: &LintContext<'_>) -> Vec<Diagnostic> {
        use xcl_vm::compiler::ast::{BinaryOperator, UnaryOperator};

        let mut out = Vec::new();
        for node in cx.ast {
            walk(node, &mut |n| {
                let AstNode::UnaryOp {
                    span,
                    op: UnaryOperator::Not,
                    operand,
                } = n
                else {
                    return;
                };
                let AstNode::BinaryOp { op, .. } = operand.as_ref() else {
                    return;
                };
                let mirrored = match op {
                    BinaryOperator::Eq => "!=",
                    BinaryOperator::NotEq => "==",
                    BinaryOperator::Lt => ">=",
                    BinaryOperator::Gt => "<=",
                    BinaryOperator::Le => ">",
                    BinaryOperator::Ge => "<",
                    _ => return,
                };
                out.push(
                    Diagnostic::warning(self.rule().code, "a comparison is inverted with `not`")
                        .label(Label::primary(cx.range(span, 3), "negated comparison"))
                        .help(format!("write the comparison directly, using `{mirrored}`")),
                );
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Report;
    use crate::level::{Group, LintConfig};

    fn analyse(source: &str) -> Report {
        let map = SourceMap::new("test.xcl", source);
        let ast = xcl_vm::parser::parse(&xcl_vm::lexer::lex(source, "test.xcl"), source);
        let cx = LintContext {
            map: &map,
            ast: &ast,
        };
        let mut report = Report::default();
        for lint in LINTS {
            for diagnostic in lint.check(&cx) {
                report.push(diagnostic);
            }
        }
        report.sort();
        report
    }

    fn codes(report: &Report) -> Vec<&'static str> {
        report.diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn unused_binding_is_reported() {
        let report = analyse("$unused = 1\nprint \"done\"");
        assert_eq!(codes(&report), vec!["W0001"]);
        assert!(report.diagnostics[0].message.contains("`$unused`"));
    }

    #[test]
    fn a_read_binding_is_not_reported() {
        let report = analyse("$used = 1\nprint $used");
        assert!(!codes(&report).contains(&"W0001"));
    }

    #[test]
    fn constant_condition_is_reported() {
        let report = analyse("if true {\n    print \"yes\"\n}");
        assert!(codes(&report).contains(&"W0002"));
    }

    #[test]
    fn dynamic_condition_is_not_reported() {
        let report = analyse("$ready = true\nif $ready {\n    print \"yes\"\n}");
        assert!(!codes(&report).contains(&"W0002"));
    }

    #[test]
    fn empty_body_is_reported() {
        let report = analyse("for $i in 0..3 {\n}");
        assert!(codes(&report).contains(&"W0003"));
    }

    #[test]
    fn unused_parameter_is_reported_with_a_fix() {
        let report = analyse("f($unused) {\n    return 1\n}\nf 2");
        let diag = report
            .diagnostics
            .iter()
            .find(|d| d.code == "W0004")
            .expect("W0004 fires");
        assert_eq!(diag.suggestions.len(), 1);
        assert_eq!(
            diag.suggestions[0].applicability,
            crate::diagnostic::Applicability::MachineApplicable
        );
    }

    #[test]
    fn underscore_parameter_is_intentionally_ignored() {
        let report = analyse("f($_) {\n    return 1\n}\nf 2");
        assert!(!codes(&report).contains(&"W0004"));
    }

    #[test]
    fn double_assignment_is_reported() {
        let report = analyse("$x = 1\n$x = 2\nprint $x");
        assert!(codes(&report).contains(&"W0005"));
    }

    #[test]
    fn assignment_then_read_then_assignment_is_fine() {
        let report = analyse("$x = 1\nprint $x\n$x = 2\nprint $x");
        assert!(!codes(&report).contains(&"W0005"));
    }

    #[test]
    fn every_diagnostic_code_is_catalogued() {
        for source in [
            "$unused = 1",
            "if true {\n    print 1\n}",
            "for $i in 0..3 {\n}",
            "f($unused) {\n    return 1\n}\nf 2",
            "$x = 1\n$x = 2\nprint $x",
        ] {
            for diagnostic in analyse(source).diagnostics {
                assert!(
                    rule::rule(diagnostic.code).is_some(),
                    "{} is emitted but not catalogued",
                    diagnostic.code
                );
            }
        }
    }

    #[test]
    fn configuration_silences_a_rule() {
        let map = SourceMap::new("test.xcl", "$unused = 1");
        let ast = xcl_vm::parser::parse(
            &xcl_vm::lexer::lex("$unused = 1", "test.xcl"),
            "$unused = 1",
        );
        let cx = LintContext {
            map: &map,
            ast: &ast,
        };
        let cfg = LintConfig::new().set("W0001", crate::level::LintLevel::Allow);
        let mut report = Report::default();
        for lint in LINTS {
            let rule = lint.rule();
            for mut diagnostic in lint.check(&cx) {
                if cfg.apply(&mut diagnostic, rule).is_some() {
                    report.push(diagnostic);
                }
            }
        }
        // W0001 is allowed, so no diagnostics should be in the report
        assert!(report.is_empty());
        assert!(!report.has_errors());
    }

    #[test]
    fn negated_comparison_is_reported() {
        let report = analyse("$a = 1\n$b = 2\nif not ($a == $b) {\n    print 1\n}");
        let diag = report
            .diagnostics
            .iter()
            .find(|d| d.code == "W0006")
            .expect("W0006 fires");
        assert!(diag.children.iter().any(|c| c.message.contains("!=")));
    }

    #[test]
    fn direct_comparison_is_not_reported() {
        let report = analyse("$a = 1\n$b = 2\nif $a != $b {\n    print 1\n}");
        assert!(!codes(&report).contains(&"W0006"));
    }

    #[test]
    fn negation_of_a_non_comparison_is_not_reported() {
        let report = analyse("$flag = false\nif not $flag {\n    print 1\n}");
        assert!(!codes(&report).contains(&"W0006"));
    }

    #[test]
    fn reports_are_deterministic() {
        let source = "$a = 1\n$b = 2\nif true {\n    print 1\n}";
        let first = analyse(source).render_json(&SourceMap::new("test.xcl", source));
        let second = analyse(source).render_json(&SourceMap::new("test.xcl", source));
        assert_eq!(first, second);
    }

    #[test]
    fn group_membership_is_declared() {
        for lint in LINTS {
            let rule = lint.rule();
            assert!(matches!(
                rule.group,
                Group::Correctness
                    | Group::Suspicious
                    | Group::Style
                    | Group::Complexity
                    | Group::Perf
                    | Group::Restriction
            ));
        }
    }
}
