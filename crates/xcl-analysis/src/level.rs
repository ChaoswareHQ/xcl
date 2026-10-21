//! Severity levels, lint groups, and per-code configuration.
//!
//! Modelled on rustc's `Level` and on Clippy's groups: a rule declares a default
//! level and a group, and a project can override either per code, per group, or
//! globally with "deny warnings".
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
//! `Allow` and `Forbid` map to `Warn`/`Deny` respectively for the purpose of
//! determining the diagnostic level, but they differ in whether a later override
//! can change them.

/// How a diagnostic is reported.
///
/// `Help` and `Note` are not findings — they are context attached to a finding,
/// which is why they never make a report fail.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    /// The program is invalid.
    Error,
    /// The program runs, but this is probably a mistake.
    Warning,
    /// Extra context for another diagnostic.
    Note,
    /// A suggested action.
    Help,
}

impl Level {
    /// Lower-case label used in rendered output and in JSON.
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Note => "note",
            Level::Help => "help",
        }
    }

    /// Returns `true` for levels that make a report fail.
    pub fn is_failure(self) -> bool {
        matches!(self, Level::Error)
    }

    /// Parses a level from its label, for `-A/-W/-D` style flags.
    pub fn from_label(label: &str) -> Option<Level> {
        Some(match label {
            "allow" | "off" | "none" => Level::Note,
            "warn" | "warning" => Level::Warning,
            "deny" | "error" => Level::Error,
            "help" => Level::Help,
            _ => return None,
        })
    }
}

/// The lint-level policy for a rule.
///
/// This controls **whether** a diagnostic fires, not how it is rendered.
/// `Allow` suppresses the diagnostic entirely. `Forbid` is sticky: once a rule
/// is forbidden, later overrides for the same target are ignored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LintLevel {
    /// Suppress the diagnostic entirely.
    Allow,
    /// Emit as a warning.
    Warn,
    /// Emit as an error.
    Deny,
    /// Emit as an error and prevent later overrides from changing it.
    Forbid,
}

impl LintLevel {
    /// Parses a lint level from its string name.
    ///
    /// Accepts rustc's spellings: `allow`, `warn`, `deny`, `forbid`.
    /// Also accepts `error` as an alias for `deny` and `off` as an alias for `allow`.
    pub fn from_name(name: &str) -> Option<LintLevel> {
        Some(match name {
            "allow" | "off" => LintLevel::Allow,
            "warn" | "warning" => LintLevel::Warn,
            "deny" | "error" => LintLevel::Deny,
            "forbid" => LintLevel::Forbid,
            _ => return None,
        })
    }

    /// The spelling accepted by `-A`/`-W`/`-D`/`-F`.
    pub fn name(self) -> &'static str {
        match self {
            LintLevel::Allow => "allow",
            LintLevel::Warn => "warn",
            LintLevel::Deny => "deny",
            LintLevel::Forbid => "forbid",
        }
    }

    /// Maps a lint level to the diagnostic [`Level`] used when rendering.
    ///
    /// `Allow` and `Forbid` map to `Warn`/`Deny` respectively; they differ only
    /// in whether a later override can change them.
    pub fn to_diagnostic(self) -> Level {
        match self {
            LintLevel::Allow => Level::Warning,
            LintLevel::Warn => Level::Warning,
            LintLevel::Deny => Level::Error,
            LintLevel::Forbid => Level::Error,
        }
    }
}

/// The family a rule belongs to.
///
/// Groups exist so a project can turn a whole class on or off (`-W correctness`)
/// without listing individual codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// Rules about programs that are certainly wrong.
    Correctness,
    /// Rules about suspicious constructs that are usually a mistake.
    Suspicious,
    /// Rules about idioms and naming.
    Style,
    /// Rules about code that is more complex than it needs to be.
    Complexity,
    /// Rules about avoidable work.
    Perf,
    /// Rules that restrict a feature on purpose.
    Restriction,
}

impl Group {
    /// Stable group name, used in configuration.
    pub fn name(self) -> &'static str {
        match self {
            Group::Correctness => "correctness",
            Group::Suspicious => "suspicious",
            Group::Style => "style",
            Group::Complexity => "complexity",
            Group::Perf => "perf",
            Group::Restriction => "restriction",
        }
    }

    /// Parses a group from its name.
    pub fn from_name(name: &str) -> Option<Group> {
        Some(match name {
            "correctness" => Group::Correctness,
            "suspicious" => Group::Suspicious,
            "style" => Group::Style,
            "complexity" => Group::Complexity,
            "perf" => Group::Perf,
            "restriction" => Group::Restriction,
            _ => return None,
        })
    }

    /// Returns `true` when rules in this group are allowed by default.
    ///
    /// `Restriction` is the only group with this property, following the
    /// rustc/clippy convention that restriction lints are opt-in.
    pub fn is_allow_by_default(self) -> bool {
        matches!(self, Group::Restriction)
    }
}

/// One configuration override.
#[derive(Clone, Debug)]
pub struct Override {
    /// A rule code (`W0001`), a rule name (`unused_binding`), a group name
    /// (`style`), or `warnings`.
    pub target: String,
    /// The lint level to apply.
    pub level: LintLevel,
}

/// Per-run lint configuration.
///
/// Later overrides win, so `deny(warnings)` followed by `allow W0002` silences
/// that one rule. This mirrors rustc's `-D warnings -A unused`.
///
/// `Forbid` is sticky: once a rule is forbidden, later overrides for the same
/// target are ignored.
#[derive(Clone, Debug, Default)]
pub struct LintConfig {
    overrides: Vec<Override>,
}

impl LintConfig {
    /// An empty configuration: every rule uses its declared default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an override and returns `self`, for chaining.
    ///
    /// The target is stored as given; use [`crate::rule::normalize_target`] to
    /// turn user input into a target this configuration can match.
    pub fn set(mut self, target: impl Into<String>, level: LintLevel) -> Self {
        self.overrides.push(Override {
            target: target.into(),
            level,
        });
        self
    }

    /// A convenience for `-D warnings`.
    pub fn deny_warnings(self) -> Self {
        self.set("warnings", LintLevel::Deny)
    }

    /// Returns the effective lint level for `code` in `group`, starting from
    /// `default`.
    ///
    /// Implements `Forbid` sticky semantics: if the current effective level is
    /// `Forbid`, later overrides for the same target are ignored.
    pub fn level_for(&self, code: &str, group: Group, default: LintLevel) -> LintLevel {
        self.resolve(default, &|target| target == code || target == group.name())
    }

    /// Returns the effective lint level for a whole [`crate::rule::Rule`].
    ///
    /// Unlike [`LintConfig::level_for`] this also matches the rule's own name and
    /// every group listed in [`crate::rule::Rule::groups`], so `-W pedantic`
    /// reaches a rule that only lists `pedantic` as a secondary group.
    pub fn level_for_rule(&self, rule: &crate::rule::Rule) -> LintLevel {
        self.resolve(rule.default_level, &|target| {
            target == rule.code
                || target == rule.name
                || target == rule.group.name()
                || rule.groups.contains(&target)
        })
    }

    /// Folds the overrides over `default`, in order.
    fn resolve(&self, default: LintLevel, matches: &dyn Fn(&str) -> bool) -> LintLevel {
        let mut level = default;
        for o in &self.overrides {
            // Once forbidden, stay forbidden — ignore later overrides for the same target.
            if level == LintLevel::Forbid {
                break;
            }
            let targets_warnings = o.target == "warnings" && level == LintLevel::Warn;
            if targets_warnings || matches(&o.target) {
                level = o.level;
            }
        }
        level
    }

    /// Returns `true` when the rule is allowed (suppressed) given the current
    /// configuration.
    pub fn is_allowed(&self, code: &str, group: Group, default: LintLevel) -> bool {
        self.level_for(code, group, default) == LintLevel::Allow
    }

    /// Applies the configuration to a diagnostic.
    ///
    /// Returns `None` when the rule is allowed (caller should drop the
    /// diagnostic), otherwise sets `diagnostic.level` and returns `Some(())`.
    pub fn apply(
        &self,
        diagnostic: &mut crate::diagnostic::Diagnostic,
        rule: &crate::rule::Rule,
    ) -> Option<()> {
        let effective = self.level_for_rule(rule);
        if effective == LintLevel::Allow {
            return None;
        }
        diagnostic.level = effective.to_diagnostic();
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip_through_labels() {
        for level in [Level::Error, Level::Warning, Level::Note, Level::Help] {
            let label = level.label();
            if level != Level::Note && level != Level::Help {
                assert_eq!(Level::from_label(label), Some(level));
            }
        }
        assert_eq!(Level::from_label("deny"), Some(Level::Error));
        assert_eq!(Level::from_label("nonsense"), None);
    }

    #[test]
    fn lint_level_from_name() {
        assert_eq!(LintLevel::from_name("allow"), Some(LintLevel::Allow));
        assert_eq!(LintLevel::from_name("off"), Some(LintLevel::Allow));
        assert_eq!(LintLevel::from_name("warn"), Some(LintLevel::Warn));
        assert_eq!(LintLevel::from_name("warning"), Some(LintLevel::Warn));
        assert_eq!(LintLevel::from_name("deny"), Some(LintLevel::Deny));
        assert_eq!(LintLevel::from_name("error"), Some(LintLevel::Deny));
        assert_eq!(LintLevel::from_name("forbid"), Some(LintLevel::Forbid));
        assert_eq!(LintLevel::from_name("nonsense"), None);
    }

    #[test]
    fn lint_level_to_diagnostic() {
        assert_eq!(LintLevel::Allow.to_diagnostic(), Level::Warning);
        assert_eq!(LintLevel::Warn.to_diagnostic(), Level::Warning);
        assert_eq!(LintLevel::Deny.to_diagnostic(), Level::Error);
        assert_eq!(LintLevel::Forbid.to_diagnostic(), Level::Error);
    }

    #[test]
    fn deny_warnings_promotes_warnings_only() {
        let cfg = LintConfig::new().deny_warnings();
        assert_eq!(
            cfg.level_for("W0001", Group::Style, LintLevel::Warn),
            LintLevel::Deny
        );
        // An allow-listed rule (default = Allow) is not promoted by deny_warnings
        assert_eq!(
            cfg.level_for("W0001", Group::Style, LintLevel::Allow),
            LintLevel::Allow,
            "an allow-listed rule stays allowed"
        );
    }

    #[test]
    fn specific_codes_beat_the_warnings_switch() {
        let cfg = LintConfig::new()
            .deny_warnings()
            .set("W0002", LintLevel::Allow);
        assert_eq!(
            cfg.level_for("W0002", Group::Style, LintLevel::Warn),
            LintLevel::Allow
        );
        assert_eq!(
            cfg.level_for("W0001", Group::Style, LintLevel::Warn),
            LintLevel::Deny
        );
    }

    #[test]
    fn groups_can_be_switched_off_wholesale() {
        let cfg = LintConfig::new().set("style", LintLevel::Allow);
        assert_eq!(
            cfg.level_for("W0001", Group::Style, LintLevel::Warn),
            LintLevel::Allow
        );
        assert_eq!(
            cfg.level_for("W0009", Group::Suspicious, LintLevel::Warn),
            LintLevel::Warn
        );
    }

    // New tests for LintLevel semantics

    #[test]
    fn allow_suppresses() {
        let cfg = LintConfig::new().set("W0001", LintLevel::Allow);
        assert!(cfg.is_allowed("W0001", Group::Style, LintLevel::Warn));
        assert!(!cfg.is_allowed("W0002", Group::Style, LintLevel::Warn));
    }

    #[test]
    fn deny_promotes() {
        let cfg = LintConfig::new().set("W0001", LintLevel::Deny);
        let effective = cfg.level_for("W0001", Group::Style, LintLevel::Warn);
        assert_eq!(effective, LintLevel::Deny);
        assert_eq!(effective.to_diagnostic(), Level::Error);
    }

    #[test]
    fn forbid_is_sticky() {
        let cfg = LintConfig::new()
            .set("W0001", LintLevel::Forbid)
            .set("W0001", LintLevel::Allow);
        let effective = cfg.level_for("W0001", Group::Style, LintLevel::Warn);
        assert_eq!(effective, LintLevel::Forbid, "forbid should be sticky");
    }

    #[test]
    fn group_level_override() {
        let cfg = LintConfig::new().set("style", LintLevel::Deny);
        assert_eq!(
            cfg.level_for("W0001", Group::Style, LintLevel::Warn),
            LintLevel::Deny
        );
        assert_eq!(
            cfg.level_for("W0009", Group::Suspicious, LintLevel::Warn),
            LintLevel::Warn
        );
    }

    #[test]
    fn code_level_override_beats_group_override() {
        let cfg = LintConfig::new()
            .set("style", LintLevel::Deny)
            .set("W0001", LintLevel::Allow);
        assert_eq!(
            cfg.level_for("W0001", Group::Style, LintLevel::Warn),
            LintLevel::Allow
        );
        assert_eq!(
            cfg.level_for("W0004", Group::Style, LintLevel::Warn),
            LintLevel::Deny
        );
    }

    #[test]
    fn restriction_is_allow_by_default() {
        assert!(Group::Restriction.is_allow_by_default());
        assert!(!Group::Style.is_allow_by_default());
        assert!(!Group::Correctness.is_allow_by_default());
    }
}
