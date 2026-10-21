//! The rule catalogue: what each code means, why it exists, and how to fix it.
//!
//! This is the machine-readable half of a lint. A diagnostic says *what* is
//! wrong at one place; a [`Rule`] says what the code means in general, which
//! group it belongs to, its default level, and a correct alternative. Tools and
//! agents look a code up here instead of guessing from the message.
//!
//! Adding a rule means adding an entry to [`RULES`] with the same code the
//! diagnostic emits. A test enforces that the two lists agree.
//!
//! # Resolution
//!
//! A published code or name is never reused for a different meaning. If a rule
//! is renamed, [`resolve_name`] returns the new name. If it is removed, the
//! replacement (if any) is reported.

use crate::level::{Group, LintLevel};

/// Everything known about one rule code.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// Stable code emitted in diagnostics, e.g. `W0001`.
    pub code: &'static str,
    /// Stable snake_case name, e.g. `unused_binding`.
    pub name: &'static str,
    /// Primary group used for bulk configuration.
    pub group: Group,
    /// Additional groups this rule belongs to.
    pub groups: &'static [&'static str],
    /// Lint level used when the configuration does not override it.
    pub default_level: LintLevel,
    /// One line, lowercase, no period.
    pub summary: &'static str,
    /// Long form for `--explain`, may be multi-line.
    pub explanation: &'static str,
    /// Why the rule exists: what goes wrong without it.
    pub rationale: &'static str,
    /// A short program demonstrating the problem.
    pub bad_example: &'static str,
    /// A short program demonstrating the fix.
    pub good_example: &'static str,
    /// Known problems with the rule; empty string when none.
    pub known_problems: &'static str,
    /// Related rule codes.
    pub see_also: &'static [&'static str],
    /// Version string when the rule was introduced, e.g. `"0.1"`.
    pub introduced: &'static str,
    /// Previous name, if the rule was renamed.
    pub renamed_from: Option<&'static str>,
    /// Whether the rule has been removed (kept for ABI stability).
    pub removed: bool,
    /// Code that replaced this rule, if removed.
    pub replaced_by: Option<&'static str>,
}

impl Rule {
    /// Renders the rule as rustc `--explain`-style documentation.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {} ({})\n", self.code, self.name));
        out.push_str(&format!("\n## What it does\n\n{}\n", self.summary));
        out.push_str(&format!("\n## Why is this bad\n\n{}\n", self.rationale));
        out.push_str(&format!("\n## Explanation\n\n{}\n", self.explanation));
        if !self.known_problems.is_empty() {
            out.push_str(&format!("\n## Known problems\n\n{}\n", self.known_problems));
        }
        if !self.see_also.is_empty() {
            out.push_str("\n## See also\n\n");
            for code in self.see_also {
                let r = crate::rule::rule(code);
                if let Some(rule) = r {
                    out.push_str(&format!("- {} ({})\n", code, rule.name));
                } else {
                    out.push_str(&format!("- {}\n", code));
                }
            }
        }
        out.push_str(&format!("\n## Version\n\n{}\n", self.introduced));
        out.push_str("\n## Groups\n\n");
        out.push_str(&format!("- {}\n", self.group.name()));
        for g in self.groups {
            out.push_str(&format!("- {}\n", g));
        }
        out
    }
}

/// Every rule this crate can emit.
pub static RULES: &[Rule] = &[
    Rule {
        code: "W0001",
        name: "unused_binding",
        group: Group::Style,
        groups: &["all", "unused"],
        default_level: LintLevel::Warn,
        summary: "a `$variable` is assigned but never read",
        explanation: "This rule fires when a variable is bound via assignment but never \
                      referenced anywhere else in the program. It typically indicates a \
                      forgotten use or a leftover from refactoring. To fix it, either remove \
                      the assignment or add a read of the variable. Note that a name appearing \
                      only inside a string literal is not counted as a use, so the binding may \
                      still be reported as unused.\n\n```xcl\n$unused = 1\nprint \"done\"\n```",
        rationale: "An assignment that is never read is dead code. Either the value was \
                    meant to be used and the use is missing, or the binding can be removed. \
                    Unlike a compiler error this does not make the program invalid, so it \
                    is reported as a warning.",
        bad_example: "$unused = 1\nprint \"done\"",
        good_example: "$used = 1\nprint $used",
        known_problems: "A name that only appears inside a string literal is not counted as a \
                         use, so it can be reported as unused.",
        see_also: &["W0004"],
        introduced: "0.1",
        renamed_from: None,
        removed: false,
        replaced_by: None,
    },
    Rule {
        code: "W0002",
        name: "constant_condition",
        group: Group::Suspicious,
        groups: &["all"],
        default_level: LintLevel::Warn,
        summary: "an `if` or `while` condition is a literal",
        explanation: "This rule fires when the condition of an `if` or `while` statement is a \
                      literal value such as `true`, `false`, a number, a string, or `nil`. \
                      Because the value is fixed at parse time, one branch can never execute, \
                      which usually means a comparison was intended but only one side was \
                      written, or a debug stub was left behind. To fix it, replace the literal \
                      with a variable or expression that depends on runtime values. Note that a \
                      constant reached through a variable is not detected by this rule.\n\n```xcl\nif true {\n    print \"yes\"\n}\n```",
        rationale: "A literal condition is decided before the program runs, so one branch \
                    can never execute. This is almost always a leftover from debugging, or \
                    a comparison was meant to be written but only one side survived.",
        bad_example: "if true {\n    print \"yes\"\n}",
        good_example: "$ready = true\nif $ready {\n    print \"yes\"\n}",
        known_problems: "A constant reached through a variable is not detected.",
        see_also: &[],
        introduced: "0.1",
        renamed_from: None,
        removed: false,
        replaced_by: None,
    },
    Rule {
        code: "W0003",
        name: "empty_body",
        group: Group::Suspicious,
        groups: &["all"],
        default_level: LintLevel::Warn,
        summary: "a block has no statements",
        explanation: "This rule fires when an `if` branch, a loop body, or a function body \
                      contains zero statements. An empty block does nothing and is almost \
                      always unintentional — either the implementation is missing or the block \
                      should be removed entirely. To fix it, add the expected statements or \
                      remove the block. If the empty body is intentional (e.g., a placeholder), \
                      add a `print` or a comment to make that clear.\n\n```xcl\nfor $i in 0..10 {\n}\n```",
        rationale: "An empty `if`, loop, or function body does nothing at all. If the block \
                    is a placeholder, say so with a comment or a `print`; if it is accidental, \
                    it hides a missing implementation.",
        bad_example: "for $i in 0..10 {\n}",
        good_example: "for $i in 0..10 {\n    print $i\n}",
        known_problems: "",
        see_also: &[],
        introduced: "0.1",
        renamed_from: None,
        removed: false,
        replaced_by: None,
    },
    Rule {
        code: "W0004",
        name: "unused_parameter",
        group: Group::Style,
        groups: &["all", "unused"],
        default_level: LintLevel::Warn,
        summary: "a parameter is never read in the function body",
        explanation: "This rule fires when a function parameter is declared but never referenced \
                      in the function body. It signals that the signature promises more than \
                      the function actually uses, which can mislead callers. To fix it, either \
                      remove the parameter or prefix it with `_` to indicate it is intentionally \
                      ignored (common in callback signatures). Parameters starting with `_` are \
                      exempt. Note that parameters of nested lambdas are not examined by this \
                      rule.\n\n```xcl\nhandler($event) {\n    print \"handled\"\n}\n```",
        rationale: "An unused parameter makes the signature lie about what the function \
                    needs. Remove it, or prefix it with `_` to state that it is intentionally \
                    ignored (callback shapes often need this).",
        bad_example: "handler($event) {\n    print \"handled\"\n}",
        good_example: "handler($event) {\n    print $event\n}",
        known_problems: "Parameters of nested lambdas are not examined.",
        see_also: &["W0001"],
        introduced: "0.1",
        renamed_from: None,
        removed: false,
        replaced_by: None,
    },
    Rule {
        code: "W0005",
        name: "shadowed_binding",
        group: Group::Complexity,
        groups: &["all"],
        default_level: LintLevel::Warn,
        summary: "a `$variable` is assigned more than once before being read",
        explanation: "This rule fires when the same variable is assigned twice in consecutive \
                      top-level statements without being read in between. The first value is \
                      silently discarded, which usually means either the first assignment is \
                      dead code or one of the two should use a different name. Self-referencing \
                      assignments like `$x = $x + 1` are exempt because they read the previous \
                      value. To fix it, remove the redundant assignment or use the variable \
                      between the two writes. Note that only consecutive top-level statements \
                      are compared; an assignment inside a branch is not tracked.\n\n```xcl\n$x = 1\n$x = 2\nprint $x\n```",
        rationale: "Assigning the same name twice in a row discards the first value. If that \
                    is intentional (a deliberate overwrite), the intermediate assignment is \
                    still dead code; if it is not, one of the two was meant to be a different \
                    name.",
        bad_example: "$x = 1\n$x = 2\nprint $x",
        good_example: "$x = 2\nprint $x",
        known_problems: "Only consecutive top-level statements are compared; an assignment \
                         inside a branch is not tracked.",
        see_also: &["W0001"],
        introduced: "0.1",
        renamed_from: None,
        removed: false,
        replaced_by: None,
    },
    Rule {
        code: "W0006",
        name: "negated_comparison",
        group: Group::Style,
        groups: &["all", "pedantic"],
        default_level: LintLevel::Warn,
        summary: "a comparison is wrapped in `not`",
        explanation: "This rule fires when a comparison operator (`==`, `!=`, `<`, `>`, `<=`, \
                      `>=`) is wrapped in `not`. Writing `not ($a == $b)` is harder to read \
                      than the equivalent `$a != $b`, and the direct form matches natural \
                      language. Each comparison operator has a mirrored counterpart, so the \
                      negation can always be eliminated. To fix it, replace the `not` with the \
                      mirrored operator. Note that this rule has no automatic fix because \
                      `Span` carries no operator position — the diagnostic can only point at \
                      the start of the expression.\n\n```xcl\nif not ($a == $b) {\n    print \"differs\"\n}\n```",
        rationale: "`not ($a == $b)` is harder to read than `$a != $b`, and the direct form \
                    matches how the condition reads out loud. The same applies to `<`, `>`, \
                    `<=`, and `>=`, which each have a mirrored operator.",
        bad_example: "if not ($a == $b) {\n    print \"differs\"\n}",
        good_example: "if $a != $b {\n    print \"differs\"\n}",
        known_problems: "The rule has no automatic fix because `Span` carries no operator \
                         position.",
        see_also: &[],
        introduced: "0.1",
        renamed_from: None,
        removed: false,
        replaced_by: None,
    },
];

/// Returns `true` when `target` names a rule code, a rule name, a group, or
/// the special `warnings` target.
///
/// This is the validator for `-A`/`-W`/`-D`/`-F`; a target this rejects would
/// silently match nothing.
pub fn is_known_target(target: &str) -> bool {
    normalize_target(target).is_some()
}

/// Canonicalizes a configuration target from user input.
///
/// Accepts a rule code in any case (`W0001`, `w0001`), a rule name in
/// snake_case or kebab-case (`unused_binding`, `unused-binding`), any group name
/// including secondary groups such as `pedantic`, and the special `warnings`
/// target. Returns `None` when nothing matches.
///
/// ```
/// use xcl_analysis::rule::normalize_target;
///
/// assert_eq!(normalize_target("w0001").as_deref(), Some("W0001"));
/// assert_eq!(normalize_target("unused-binding").as_deref(), Some("unused_binding"));
/// assert_eq!(normalize_target("Style").as_deref(), Some("style"));
/// assert!(normalize_target("nonsense").is_none());
/// ```
pub fn normalize_target(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    if target.eq_ignore_ascii_case("warnings") {
        return Some("warnings".to_string());
    }

    // Codes and names may be written with either separator and in any case.
    let underscored = target.replace('-', "_");
    if let Some(rule) = RULES.iter().find(|r| r.code.eq_ignore_ascii_case(target)) {
        return Some(rule.code.to_string());
    }
    if let Some(rule) = RULES
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(&underscored))
    {
        return Some(rule.name.to_string());
    }
    if let Some(group) = Group::from_name(&underscored.to_ascii_lowercase()) {
        return Some(group.name().to_string());
    }
    if let Some(rule) = RULES.iter().find(|r| {
        r.groups
            .iter()
            .any(|g| g.eq_ignore_ascii_case(&underscored))
    }) {
        // Secondary groups are only usable if a rule actually lists them.
        return rule
            .groups
            .iter()
            .find(|g| g.eq_ignore_ascii_case(&underscored))
            .map(|g| (*g).to_string());
    }
    None
}

/// Looks up a rule by code.
pub fn rule(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.code == code)
}

/// Looks up a rule by name.
pub fn rule_by_name(name: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.name == name)
}

/// Renders the whole catalogue, for `xcl --explain-all` style output.
pub fn render_catalogue() -> String {
    RULES
        .iter()
        .map(Rule::render)
        .collect::<Vec<_>>()
        .join("\n---\n\n")
}

/// Result of resolving a rule name.
///
/// A published code or name is never reused for a different meaning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    /// The name was renamed to this new name.
    Renamed { to: &'static str },
    /// The rule was removed, optionally with a replacement.
    Removed { replaced_by: Option<&'static str> },
    /// The name is not recognized.
    Unknown,
}

/// Resolves a rule name, following renames and removals.
///
/// Returns the current rule if the name is active, or a [`Resolution`] variant
/// explaining what happened to the name.
pub fn resolve_name(name: &str) -> Result<&'static Rule, Resolution> {
    // Check active rules first
    if let Some(r) = RULES.iter().find(|r| r.name == name && !r.removed) {
        return Ok(r);
    }
    // Check renamed rules
    if let Some(r) = RULES.iter().find(|r| r.renamed_from == Some(name)) {
        return Err(Resolution::Renamed { to: r.name });
    }
    // Check removed rules
    if let Some(r) = RULES.iter().find(|r| r.name == name && r.removed) {
        return Err(Resolution::Removed {
            replaced_by: r.replaced_by,
        });
    }
    Err(Resolution::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen: Vec<&str> = Vec::new();
        for rule in RULES {
            assert!(
                rule.code.starts_with('E') || rule.code.starts_with('W'),
                "{} must start with E or W",
                rule.code
            );
            assert!(
                rule.code.len() == 5 && rule.code[1..].chars().all(|c| c.is_ascii_digit()),
                "{} must look like E0001",
                rule.code
            );
            assert!(!seen.contains(&rule.code), "duplicate code {}", rule.code);
            seen.push(rule.code);
        }
    }

    #[test]
    fn names_are_unique_snake_case() {
        let mut seen: Vec<&str> = Vec::new();
        for rule in RULES {
            assert!(
                rule.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{} is not snake_case",
                rule.name
            );
            assert!(!seen.contains(&rule.name), "duplicate name {}", rule.name);
            seen.push(rule.name);
        }
    }

    #[test]
    fn names_pass_rustc_grammar_test() {
        // Every name must read correctly after `allow ` — i.e., be snake_case, no spaces.
        for rule in RULES {
            assert!(
                !rule.name.contains(' '),
                "{} contains a space, fails grammar test",
                rule.name
            );
            assert!(
                rule.name
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_lowercase())
                    .unwrap_or(false),
                "{} does not start with a lowercase letter",
                rule.name
            );
        }
    }

    #[test]
    fn lookup_works_by_code_and_name() {
        assert_eq!(rule("W0001").unwrap().name, "unused_binding");
        assert_eq!(rule_by_name("unused_binding").unwrap().code, "W0001");
        assert!(rule("W9999").is_none());
    }

    #[test]
    fn every_rule_has_guidance() {
        for rule in RULES {
            assert!(!rule.summary.is_empty(), "{} has no summary", rule.code);
            assert!(!rule.rationale.is_empty(), "{} has no rationale", rule.code);
            assert!(
                !rule.explanation.is_empty(),
                "{} has no explanation",
                rule.code
            );
            assert!(
                !rule.bad_example.is_empty(),
                "{} has no bad example",
                rule.code
            );
            assert!(
                !rule.good_example.is_empty(),
                "{} has no good example",
                rule.code
            );
        }
    }

    #[test]
    fn catalogue_renders_readable_text() {
        let text = render_catalogue();
        assert!(text.contains("# W0001 (unused_binding)"));
        assert!(text.contains("## What it does"));
        assert!(text.contains("## Why is this bad"));
        assert!(text.contains("## Explanation"));
        assert!(text.contains("## Version"));
        assert!(text.contains("## Groups"));
    }

    #[test]
    fn render_contains_explanation_and_is_not_duplicate_of_rationale() {
        for rule in RULES {
            let rendered = rule.render();
            assert!(
                rendered.contains("## Explanation"),
                "{} render() must contain ## Explanation",
                rule.code
            );
            assert!(
                !rule.explanation.contains(rule.rationale),
                "{} explanation must not be a substring of its rationale",
                rule.code
            );
        }
    }

    #[test]
    fn resolve_name_finds_active_rule() {
        let r = resolve_name("unused_binding").expect("unused_binding exists");
        assert_eq!(r.code, "W0001");
    }

    #[test]
    fn resolve_name_returns_unknown_for_garbage() {
        match resolve_name("nonexistent") {
            Err(Resolution::Unknown) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn message_style_enforcement() {
        for rule in RULES {
            let s = rule.summary;
            // Starts with a lowercase ASCII letter
            assert!(
                s.chars()
                    .next()
                    .map(|c| c.is_ascii_lowercase())
                    .unwrap_or(false),
                "{} summary does not start with lowercase: {:?}",
                rule.code,
                s
            );
            // Does not end with ., !, or ?
            assert!(
                !s.ends_with('.') && !s.ends_with('!') && !s.ends_with('?'),
                "{} summary ends with punctuation: {:?}",
                rule.code,
                s
            );
            // Contains no #[, #![, and no "illegal"
            assert!(!s.contains("#["), "{} summary contains #[", rule.code);
            assert!(!s.contains("#!["), "{} summary contains #![", rule.code);
            assert!(
                !s.contains("illegal"),
                "{} summary contains 'illegal'",
                rule.code
            );
        }
    }
}
