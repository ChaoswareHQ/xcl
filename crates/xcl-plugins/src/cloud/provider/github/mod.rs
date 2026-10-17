//! GitHub provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.github.*`
//!
//! ```xcl
//! # GitHub with domain expansion
//! github "my-org" {
//!     bucket "my-repo" {
//!         $list = bucket.list ()
//!     }
//!     function "my-action" {
//!         $result = function.invoke { input: "test" }
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `github(org)` — Domain wrapper that sets GitHub context
//! - `github.config()` — Get current GitHub configuration
//! - `github.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (GitHub Packages/Actions artifacts)
//! - `function.*` — Serverless functions (GitHub Actions)
//! - `instance.*` — Compute instances (GitHub Runners)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within a GitHub context.
#[xcl_fn(
    name = "github",
    args = "org: str; f: any",
    ret = "any",
    summary = "Run a closure within a GitHub organization context."
)]
fn github_run(ctx: &mut DomainCtx) -> Value {
    let org = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    let credentials = load_credentials(ctx);

    ctx.push_context("github.org", Value::from_str_boxed(org.to_string()));
    ctx.push_context("github.credentials", credentials);
    let result = ctx.call(closure, &[]);
    ctx.pop_context("github.credentials");
    ctx.pop_context("github.org");
    result
}

/// Get the current GitHub configuration.
#[xcl_fn(
    name = "github.config",
    ret = "map",
    summary = "Get the current GitHub configuration."
)]
fn github_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(org) = ctx.get_context("github.org") {
        config.insert("org".to_string(), org);
    }
    Value::from_map_boxed(config)
}

/// Get the current GitHub credentials.
#[xcl_fn(
    name = "github.credential",
    ret = "map",
    summary = "Get the current GitHub credentials."
)]
fn github_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("github.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();
    let token = std::env::var("GITHUB_TOKEN").ok();

    match token {
        Some(t) => {
            creds.insert("token".to_string(), Value::from_str_boxed(t));
        }
        _ => {
            if let Ok(env_file) = std::env::var("GITHUB_ENV_FILE")
                && let Ok(content) = std::fs::read_to_string(&env_file) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') { continue; }
                        if let Some((key, value)) = line.split_once('=')
                            && key.trim() == "GITHUB_TOKEN" {
                                creds.insert("token".to_string(), Value::from_str_boxed(value.trim().to_string()));
                            }
                    }
                }
        }
    }
    Value::from_map_boxed(creds)
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn github_run_sets_org() {
        let r = evaluate_new(r#"
            github "my-org" {
                github.config ().org
            }
        "#);
        assert_eq!(r.as_str(), Some("my-org"));
    }

    #[test]
    fn github_nesting_works() {
        let r = evaluate_new(r#"
            github "org-a" {
                github "org-b" {
                    github.config ().org
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("org-b"));
    }
}
