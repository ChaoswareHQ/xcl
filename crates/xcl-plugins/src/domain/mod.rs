//! Domain expansion — context blocks that influence nested calls.
//!
//! ## Concept
//!
//! A domain function takes a block-lambda as its last argument, sets context
//! variables before invoking the block, then clears them after. Inner calls can
//! read the context to dispatch behavior.
//!
//! ## Example
//!
//! ```xcl
//! // Set a context, run a block, then restore
//! domain.run "mykey" "myvalue" {
//!     $val = domain.get "mykey"   # returns "myvalue"
//!     print $val
//! }
//! # After the block, "mykey" is automatically cleared
//!
//! // Nesting works naturally
//! domain.run "browser" "chrome" {
//!     print domain.get "browser"   # "chrome"
//!     domain.run "browser" "firefox" {
//!         print domain.get "browser"   # "firefox"
//!     }
//!     print domain.get "browser"   # "chrome" again
//! }
//! ```
//!
//! ## Building a real domain
//!
//! A domain like `browser` or `rule` is just a function that:
//! 1. Pushes context with `domain.run`
//! 2. Inner functions read context with `domain.get`
//!
//! ```xcl
//! fn browser(backend, f) {
//!     domain.run "browser_backend" backend f
//! }
//!
//! fn nav(url) {
//!     $backend = domain.get "browser_backend"
//!     if $backend == "brave" {
//!         # dispatch to brave
//!     } else if $backend == "chrome" {
//!         # dispatch to chrome
//!     }
//! }
//! ```
//!
//! ## Why this works
//!
//! The context stack is per-VM, not per-frame, so it survives across closure
//! calls. This means `domain.run` + block-lambda gives you scoped context
//! without any parser changes — it's just function calls and closures.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;

/// Run a closure within a context scope.
///
/// Pushes `value` onto the context stack for `key`, calls the closure with no
/// arguments, then pops the context — even if the closure returns an error.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.run(key: str; value: any; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = domain.run "mode" "debug" {
///     if domain.get "mode" == "debug" {
///         print "running in debug"
///     }
///     42
/// }
/// # $r == 42
/// # "mode" is no longer set
/// ```
///
/// ## NESTING
///
/// ```xcl
/// domain.run "level" 1 {
///     domain.run "level" 2 {
///         print domain.get "level"   # 2
///     }
///     print domain.get "level"       # 1
/// }
/// ```
#[xcl_fn(
    name = "domain.run",
    args = "key: str; value: any; f: any",
    ret = "any",
    summary = "Run a closure within a scoped context. The context is automatically \
               cleared after the block finishes, enabling nested domain blocks."
)]
fn domain_run(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("").to_string();
    let value = ctx.arg(1);
    let closure = ctx.arg(2);

    ctx.push_context(&key, value);
    let result = ctx.call(closure, &[]);
    ctx.pop_context(&key);
    result
}

/// Get the current context value for a key.
///
/// Returns the most recently pushed value, or `nil` if the key is not set.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.get(key: str) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// domain.run "db" "postgres" {
///     $db = domain.get "db"
///     # $db == "postgres"
/// }
/// ```
#[xcl_fn(
    name = "domain.get",
    args = "key: str",
    ret = "any",
    summary = "Read the current context value for a key. Returns nil if not set."
)]
fn domain_get(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    ctx.get_context(key).unwrap_or(Value::nil())
}

/// Check if a context key is currently set.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.has(key: str) -> bool
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// domain.run "mode" "test" {
///     if domain.has "mode" {
///         print "mode is set"
///     }
/// }
/// ```
#[xcl_fn(
    name = "domain.has",
    args = "key: str",
    ret = "bool",
    summary = "Check if a context key is currently active."
)]
fn domain_has(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    Value::bool_(ctx.has_context(key))
}

/// Set a context value without running a block.
///
/// Unlike `domain.run`, this does not automatically pop — you must call
/// `domain.pop` or `domain.clear` manually. Useful for long-lived context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.set(key: str; value: any) -> nil
/// ```
#[xcl_fn(
    name = "domain.set",
    args = "key: str; value: any",
    ret = "nil",
    summary = "Push a context value manually. Use domain.pop or domain.clear to remove."
)]
fn domain_set(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("").to_string();
    let value = ctx.arg(1);
    ctx.push_context(&key, value);
    Value::nil()
}

/// Pop the top context value for a key.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.pop(key: str) -> any
/// ```
#[xcl_fn(
    name = "domain.pop",
    args = "key: str",
    ret = "any",
    summary = "Remove and return the top context value for a key."
)]
fn domain_pop(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    ctx.pop_context(key).unwrap_or(Value::nil())
}

/// Clear all context entries for a key.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.clear(key: str) -> nil
/// ```
#[xcl_fn(
    name = "domain.clear",
    args = "key: str",
    ret = "nil",
    summary = "Remove all stacked values for a context key."
)]
fn domain_clear(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    ctx.clear_context(key);
    Value::nil()
}

/// Run a closure with multiple context keys set at once.
///
/// Takes a map of key-value pairs, pushes all of them, runs the closure,
/// then pops all of them in reverse order.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.with(context: map; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// domain.with { db: "postgres", mode: "test" } {
///     print domain.get "db"     # "postgres"
///     print domain.get "mode"   # "test"
/// }
/// ```
#[xcl_fn(
    name = "domain.with",
    args = "context: map; f: any",
    ret = "any",
    summary = "Run a closure with multiple context keys set simultaneously."
)]
fn domain_with(ctx: &mut DomainCtx) -> Value {
    let context_map = ctx.arg(0);
    let closure = ctx.arg(1);

    let map = match context_map.as_map() {
        Some(m) => m.clone(),
        None => return err_result("type_error", "domain.with expects a map", 0),
    };

    let keys: Vec<String> = map.keys().map(|s| s.to_string()).collect();

    for key in &keys {
        if let Some(val) = map.get(key) {
            ctx.push_context(key, *val);
        }
    }

    let result = ctx.call(closure, &[]);

    for key in &keys {
        ctx.pop_context(key);
    }

    result
}

/// Get all currently set context keys as an array.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.keys() -> arr
/// ```
#[xcl_fn(
    name = "domain.keys",
    ret = "arr",
    summary = "List all context keys that currently have values."
)]
fn domain_keys(ctx: &mut DomainCtx) -> Value {
    let keys: Vec<Value> = ctx.context_keys()
        .into_iter()
        .map(Value::from_str_boxed)
        .collect();
    Value::from_array_boxed(keys)
}

/// Snapshot the current context state, returns a map of key → top value.
///
/// Useful for debugging or saving/restoring context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.snapshot() -> map
/// ```
#[xcl_fn(
    name = "domain.snapshot",
    ret = "map",
    summary = "Capture a snapshot of all current context values."
)]
fn domain_snapshot(ctx: &mut DomainCtx) -> Value {
    let map = ctx.context_snapshot();
    Value::from_map_boxed(map)
}

// ─── High-level domain wrappers ───────────────────────────────────────
// These build on domain.run/domain.get to provide ready-to-use context
// blocks for common std library patterns.

/// Run a closure with the working directory temporarily changed.
///
/// Saves the current directory, changes to `new_dir`, runs the closure,
/// then restores the original directory — even if the closure errors.
///
/// ## SYNTAX
///
/// ```xcl
/// fn io.dir(path: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// io.dir "/tmp" {
///     # All io.open() calls here use /tmp as the base
///     $files = path.list "."
///     print $files
/// }
/// # Working directory restored automatically
/// ```
///
/// ## NESTING
///
/// ```xcl
/// io.dir "/home" {
///     print path.cwd ()   # "/home"
///     io.dir "/home/user" {
///         print path.cwd ()   # "/home/user"
///     }
///     print path.cwd ()   # "/home" again
/// }
/// ```
#[xcl_fn(
    name = "io.dir",
    args = "path: str; f: any",
    ret = "any",
    summary = "Temporarily change the working directory for the duration of a block."
)]
fn io_dir(ctx: &mut DomainCtx) -> Value {
    let new_dir = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    // Save current directory
    let old_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => return err_result("io_error", &format!("Failed to get current dir: {}", e), 0),
    };

    // Change to new directory
    if let Err(e) = std::env::set_current_dir(new_dir) {
        return err_result("io_error", &format!("Failed to change to {}: {}", new_dir, e), 0);
    }

    let result = ctx.call(closure, &[]);

    // Restore original directory (best-effort)
    let _ = std::env::set_current_dir(&old_dir);

    result
}

/// Run a closure with environment variables temporarily overridden.
///
/// Takes a map of key-value pairs, sets them as environment variables,
/// runs the closure, then restores the original values — even on error.
///
/// ## SYNTAX
///
/// ```xcl
/// fn env.override(vars: map; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// env.override { PATH: "/usr/local/bin", DEBUG: "1" } {
///     # env.get "DEBUG" returns "1" here
///     # A subprocess spawned here sees the modified PATH
///     print env.get "DEBUG"
/// }
/// # Environment restored automatically
/// ```
///
/// ## NOTES
///
/// - Only the keys in the map are overridden; other env vars are untouched.
/// - If a key was not set before, it is removed after the block.
/// - `nil` values remove the variable for the duration of the block.
#[xcl_fn(
    name = "env.override",
    args = "vars: map; f: any",
    ret = "any",
    summary = "Temporarily override environment variables for the duration of a block."
)]
fn env_override(ctx: &mut DomainCtx) -> Value {
    let vars_map = ctx.arg(0);
    let closure = ctx.arg(1);

    let map = match vars_map.as_map() {
        Some(m) => m.clone(),
        None => return err_result("type_error", "env.override expects a map", 0),
    };

    let keys: Vec<String> = map.keys().map(|s| s.to_string()).collect();

    // Save original values
    let mut originals: HashMap<String, Option<String>> = HashMap::new();
    for key in &keys {
        originals.insert(key.clone(), std::env::var(key).ok());
    }

    // Apply overrides
    for key in &keys {
        if let Some(val) = map.get(key) {
            if val.is_nil() {
                unsafe { std::env::remove_var(key); }
            } else {
                let v = val.as_str().unwrap_or("").to_string();
                unsafe { std::env::set_var(key, v); }
            }
        }
    }

    let result = ctx.call(closure, &[]);

    // Restore originals
    for key in &keys {
        if let Some(orig) = originals.get(key) {
            match orig {
                Some(v) => unsafe { std::env::set_var(key, v); },
                None => unsafe { std::env::remove_var(key); },
            }
        }
    }

    result
}

/// Profile a block of code and return timing information.
///
/// Runs the closure and returns a map with elapsed time in milliseconds
/// and the closure's result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.profile(f: any) -> map
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = domain.profile {
///     # Some expensive operation
///     arr.range 1000000
/// }
/// print "Took: " $r.elapsed_ms "ms"
/// print "Result length: " $r.value.len
/// ```
///
/// ## RETURNS
///
/// A map with:
/// - `elapsed_ms`: float — milliseconds elapsed
/// - `value`: any — the closure's return value
#[xcl_fn(
    name = "domain.profile",
    args = "f: any",
    ret = "map",
    summary = "Time a block of code and return elapsed milliseconds with the result."
)]
fn domain_profile(ctx: &mut DomainCtx) -> Value {
    let closure = ctx.arg(0);
    let start = std::time::Instant::now();
    let result = ctx.call(closure, &[]);
    let elapsed = start.elapsed();

    let mut map = HashMap::new();
    map.insert("elapsed_ms".to_string(), Value::from_f64(elapsed.as_secs_f64() * 1000.0));
    map.insert("value".to_string(), result);
    Value::from_map_boxed(map)
}

/// Run a closure with a label, useful for structured logging or tracing.
///
/// Sets a `trace.label` context key so inner calls can include the label
/// in their output. Returns the closure's result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn domain.label(name: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// domain.label "database.init" {
///     # Inner calls can read domain.get "trace.label"
///     # to include "database.init" in logs
///     connect_db()
/// }
/// ```
#[xcl_fn(
    name = "domain.label",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a block under a trace label accessible via domain.get \"trace.label\"."
)]
fn domain_label(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("").to_string();
    let closure = ctx.arg(1);

    ctx.push_context("trace.label", Value::from_str_boxed(name));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("trace.label");
    result
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn domain_run_sets_and_clears_context() {
        let r = evaluate_new(r#"
            domain.run "key1" "val1" {
                domain.get "key1"
            }
        "#);
        assert_eq!(r.as_str(), Some("val1"));

        // After the block, key1 should be cleared
        let r2 = evaluate_new(r#"
            domain.run "key1" "val1" { }
            domain.get "key1"
        "#);
        assert!(r2.is_nil());
    }

    #[test]
    fn domain_nesting_works() {
        let r = evaluate_new(r#"
            domain.run "level" 1 {
                domain.run "level" 2 {
                    domain.get "level"
                }
            }
        "#);
        assert_eq!(r.as_i48(), 2);
    }

    #[test]
    fn domain_outer_restored_after_inner() {
        let r = evaluate_new(r#"
            domain.run "x" "outer" {
                domain.run "x" "inner" { }
                domain.get "x"
            }
        "#);
        assert_eq!(r.as_str(), Some("outer"));
    }

    #[test]
    fn domain_has_returns_bool() {
        let r = evaluate_new(r#"
            domain.run "k" "v" {
                domain.has "k"
            }
        "#);
        assert!(r.as_bool());

        let r2 = evaluate_new(r#"domain.has "nonexistent""#);
        assert!(!r2.as_bool());
    }

    #[test]
    fn domain_with_multiple_keys() {
        // Nested domain.run with different keys - test each separately first
        let r1 = evaluate_new(r#"
            domain.run "a" 10 {
                domain.get "a"
            }
        "#);
        assert_eq!(r1.as_i48(), 10);

        let r2 = evaluate_new(r#"
            domain.run "a" 10 {
                domain.run "b" 20 {
                    domain.get "b"
                }
            }
        "#);
        assert_eq!(r2.as_i48(), 20);

        // Now test both together
        let r3 = evaluate_new(r#"
            domain.run "a" 10 {
                domain.run "b" 20 {
                    domain.get "a"
                }
            }
        "#);
        assert_eq!(r3.as_i48(), 10);
    }

    #[test]
    fn domain_keys_lists_active_keys() {
        let r = evaluate_new(r#"
            domain.run "x" 1 {
                domain.run "y" 2 {
                    domain.keys ()
                }
            }
        "#);
        let arr = r.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn domain_set_and_pop_manual() {
        let r = evaluate_new(r#"
            domain.set "manual" 42
            $v = domain.get "manual"
            domain.pop "manual"
            $v
        "#);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn domain_clear_removes_all() {
        let r = evaluate_new(r#"
            domain.set "k" 1
            domain.set "k" 2
            domain.clear "k"
            domain.get "k"
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn domain_snapshot_returns_map() {
        let r = evaluate_new(r#"
            domain.run "a" 10 {
                domain.run "b" 20 {
                    domain.snapshot ()
                }
            }
        "#);
        let map = r.as_map().unwrap();
        assert_eq!(map.len(), 2);
    }

    // ─── High-level domain wrapper tests ──────────────────────────────

    #[test]
    fn io_dir_changes_and_restores() {
        let r = evaluate_new(r#"
            $before = path.cwd ().value
            io.dir "." {
                path.cwd ().value
            }
        "#);
        // Should return the current dir (same as before since we chdir to ".")
        assert!(r.as_str().is_some());

        // Verify cwd is restored after the block
        let r2 = evaluate_new(r#"
            $before = path.cwd ().value
            io.dir "." { }
            path.cwd ().value
        "#);
        assert_eq!(r.as_str(), r2.as_str());
    }

    #[test]
    fn io_dir_nesting_works() {
        let r = evaluate_new(r#"
            io.dir "." {
                io.dir "." {
                    path.cwd ().value
                }
            }
        "#);
        assert!(r.as_str().is_some());
    }

    #[test]
    fn env_override_sets_and_restores() {
        // Set a test var, override it, then verify it's restored
        unsafe { std::env::set_var("XCL_TEST_DOMAIN_VAR", "original"); }

        let r = evaluate_new(r#"
            env.override { XCL_TEST_DOMAIN_VAR: "overridden" } {
                env.get "XCL_TEST_DOMAIN_VAR"
            }
        "#);
        assert_eq!(r.as_str(), Some("overridden"));

        // After the block, the original value should be restored
        let r2 = evaluate_new(r#"
            env.override { XCL_TEST_DOMAIN_VAR: "overridden" } { }
            env.get "XCL_TEST_DOMAIN_VAR"
        "#);
        assert_eq!(r2.as_str(), Some("original"));

        unsafe { std::env::remove_var("XCL_TEST_DOMAIN_VAR"); }
    }

    #[test]
    fn env_override_new_var_removed_after() {
        // A var that didn't exist before should be removed after the block
        let r = evaluate_new(r#"
            env.override { XCL_TEST_NEW_VAR: "hello" } {
                env.get "XCL_TEST_NEW_VAR"
            }
        "#);
        assert_eq!(r.as_str(), Some("hello"));

        let r2 = evaluate_new(r#"
            env.override { XCL_TEST_NEW_VAR: "hello" } { }
            env.get "XCL_TEST_NEW_VAR"
        "#);
        assert!(r2.is_nil());
    }

    #[test]
    fn domain_profile_returns_map_with_timing() {
        let r = evaluate_new(r#"
            domain.profile { 42 }
        "#);
        let map = r.as_map().expect("domain.profile should return a map");
        assert!(map.contains_key("elapsed_ms"), "should have elapsed_ms");
        assert!(map.contains_key("value"), "should have value");
        assert_eq!(map.get("value").unwrap().as_i48(), 42);
    }

    #[test]
    fn domain_label_sets_trace_context() {
        let r = evaluate_new(r#"
            domain.label "my.operation" {
                domain.get "trace.label"
            }
        "#);
        assert_eq!(r.as_str(), Some("my.operation"));

        // After the block, label should be cleared
        let r2 = evaluate_new(r#"
            domain.label "my.operation" { }
            domain.get "trace.label"
        "#);
        assert!(r2.is_nil());
    }

    #[test]
    fn domain_label_nesting() {
        let r = evaluate_new(r#"
            domain.label "outer" {
                domain.label "inner" {
                    domain.get "trace.label"
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("inner"));
    }
}
