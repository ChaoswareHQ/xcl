//! `rule.allow` / `rule.deny` — the print and input gate.
//!
//! The policy is a small per-thread set, checked once per I/O call:
//!
//! ```xcl
//! rule.deny "print"            # block stdout (print, println, write, flush)
//! rule.deny "input"            # block stdin (input, readline, read, prompt)
//! rule.deny "*"                # deny everything not explicitly allowed
//! rule.allow "print"           # explicit allows win over a wildcard deny
//! rule.message "no console in production"
//! rule.is_allowed "print"      # false
//! rule.reset                   # back to allow-all
//! ```
//!
//! Matching is exact, or a prefix when the pattern ends in `*` (`"print*"`).
//! A denied call does not run: it writes one `util: denied <op>: <message>`
//! line to stderr and returns without side effects.
//!
//! The state is thread-local, which matches how XCL VMs are used (one VM per
//! thread); two VMs on different threads do not share a policy.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;

use xcl_macros::{Xcl, xcl_impl};
use xcl_vm::Value;

use super::Util;

/// Policy state for the `rule` domain.
#[derive(Xcl)]
pub struct Rule;

struct Policy {
    allow: Vec<String>,
    deny: Vec<String>,
    deny_all: bool,
    message: String,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            deny: Vec::new(),
            deny_all: false,
            message: "denied by policy".to_string(),
        }
    }
}

thread_local! {
    static POLICY: RefCell<Policy> = RefCell::new(Policy::default());
    /// True while no rule has ever been set: the gate becomes a single load.
    static DEFAULT_POLICY: Cell<bool> = const { Cell::new(true) };
}

fn matches(pattern: &str, name: &str) -> bool {
    pattern == "*"
        || pattern == name
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| name.starts_with(prefix))
}

fn denied(op: &str) -> Option<String> {
    if DEFAULT_POLICY.get() {
        return None;
    }
    POLICY.with(|policy| {
        let policy = policy.borrow();
        if policy.allow.iter().any(|pattern| matches(pattern, op)) {
            return None;
        }
        if policy.deny.iter().any(|pattern| matches(pattern, op)) || policy.deny_all {
            return Some(policy.message.clone());
        }
        None
    })
}

/// Reports a denial on stderr. Returns `true` when `op` may run.
pub(crate) fn gate(op: &str) -> bool {
    if DEFAULT_POLICY.get() {
        return true;
    }
    let Some(message) = denied(op) else {
        return true;
    };
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    let _ = err.write_all(b"util: denied ");
    let _ = err.write_all(op.as_bytes());
    if !message.is_empty() {
        let _ = err.write_all(b": ");
        let _ = err.write_all(message.as_bytes());
    }
    let _ = err.write_all(b"\n");
    false
}

/// Checks the family name first (`"print"`, `"eprint"`, `"input"`), then the
/// exact operation. `rule.deny "input"` therefore blocks `readline` too.
pub(crate) fn gate_family(group: &str, op: &str) -> bool {
    if DEFAULT_POLICY.get() {
        return true;
    }
    if group == op {
        return gate(op);
    }
    gate(group) && gate(op)
}

fn allow_impl(name: String) -> bool {
    let name = name.trim().to_string();
    if name.is_empty() {
        return false;
    }
    POLICY.with(|policy| {
        let mut policy = policy.borrow_mut();
        policy.deny.retain(|pattern| pattern != &name);
        if !policy.allow.iter().any(|pattern| pattern == &name) {
            policy.allow.push(name);
        }
    });
    DEFAULT_POLICY.set(false);
    true
}

fn deny_impl(name: String) -> bool {
    let name = name.trim().to_string();
    if name.is_empty() {
        return false;
    }
    POLICY.with(|policy| {
        let mut policy = policy.borrow_mut();
        policy.allow.retain(|pattern| pattern != &name);
        if !policy.deny.iter().any(|pattern| pattern == &name) {
            policy.deny.push(name);
        }
    });
    DEFAULT_POLICY.set(false);
    true
}

fn is_allowed_impl(name: &str) -> bool {
    denied(name).is_none()
}

fn reset_impl() {
    POLICY.with(|policy| {
        let mut policy = policy.borrow_mut();
        policy.allow.clear();
        policy.deny.clear();
        policy.deny_all = false;
        policy.message = "denied by policy".to_string();
    });
    DEFAULT_POLICY.set(true);
}

#[xcl_impl]
impl Rule {
    /// Permit an operation (exact name, or a `prefix*` pattern).
    pub fn allow(name: String) -> bool {
        allow_impl(name)
    }

    /// Deny an operation (exact name, `prefix*`, or `*` for everything).
    ///
    /// Explicit allows win over `*`; `deny` removes an earlier `allow` of the
    /// same name.
    pub fn deny(name: String) -> bool {
        deny_impl(name)
    }

    /// Deny every operation that is not explicitly allowed.
    pub fn deny_all() {
        POLICY.with(|policy| policy.borrow_mut().deny_all = true);
        DEFAULT_POLICY.set(false);
    }

    /// Remove all rules and restore allow-all.
    pub fn reset() {
        reset_impl();
    }

    /// True when the operation would be allowed right now.
    pub fn is_allowed(name: String) -> bool {
        is_allowed_impl(name.as_str())
    }

    /// Set the message included in denial diagnostics.
    pub fn message(text: String) {
        POLICY.with(|policy| policy.borrow_mut().message = text);
        DEFAULT_POLICY.set(false);
    }

    /// The current policy as `{allow, deny, deny_all, message}`.
    pub fn list() -> Value {
        POLICY.with(|policy| {
            let policy = policy.borrow();
            let mut map = HashMap::with_capacity(4);
            map.insert(
                "allow".to_string(),
                Value::from_array_boxed(
                    policy
                        .allow
                        .iter()
                        .map(|pattern| Value::from_str_boxed(pattern.clone()))
                        .collect(),
                ),
            );
            map.insert(
                "deny".to_string(),
                Value::from_array_boxed(
                    policy
                        .deny
                        .iter()
                        .map(|pattern| Value::from_str_boxed(pattern.clone()))
                        .collect(),
                ),
            );
            map.insert("deny_all".to_string(), Value::bool(policy.deny_all));
            map.insert(
                "message".to_string(),
                Value::from_str_boxed(policy.message.clone()),
            );
            Value::from_map_boxed(map)
        })
    }
}

#[xcl_impl]
impl Util {
    /// `util.allow` is an alias of `rule.allow`.
    pub fn allow(name: String) -> bool {
        allow_impl(name)
    }

    /// `util.deny` is an alias of `rule.deny`.
    pub fn deny(name: String) -> bool {
        deny_impl(name)
    }

    /// `util.is_allowed` is an alias of `rule.is_allowed`.
    pub fn is_allowed(name: String) -> bool {
        is_allowed_impl(name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    fn allowed(name: &str) -> bool {
        evaluate_new(&format!("rule.is_allowed \"{name}\"")).as_bool()
    }

    #[test]
    fn allow_deny_and_wildcards() {
        evaluate_new("rule.reset");
        assert!(allowed("print"));

        evaluate_new("rule.deny \"print\"");
        assert!(!allowed("print"));
        assert!(allowed("input"));

        evaluate_new("rule.allow \"print\"");
        assert!(allowed("print"));

        evaluate_new("rule.deny \"*\"");
        assert!(!allowed("anything"));
        evaluate_new("rule.allow \"print\"");
        assert!(allowed("print"));
        assert!(!allowed("input"));

        evaluate_new("rule.reset");
        assert!(allowed("input"));
    }

    #[test]
    fn prefix_patterns() {
        evaluate_new("rule.reset");
        evaluate_new("rule.deny \"print*\"");
        assert!(!allowed("print"));
        assert!(!allowed("println"));
        assert!(allowed("input"));
        evaluate_new("rule.reset");
    }

    #[test]
    fn list_reports_state() {
        evaluate_new("rule.reset");
        evaluate_new("rule.deny \"print\"");
        let map = evaluate_new("rule.list").as_map().expect("map");
        assert_eq!(map.get("deny_all").map(|v| v.as_bool()), Some(false));
        let deny = map.get("deny").and_then(|v| v.as_array()).expect("deny[]");
        assert_eq!(deny.len(), 1);
        assert_eq!(deny[0].as_str(), Some("print"));
        evaluate_new("rule.reset");
    }
}
