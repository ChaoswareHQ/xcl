//! IBM Cloud provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.ibm.*`
//!
//! ```xcl
//! # IBM Cloud with domain expansion
//! ibm "my-account" {
//!     cos "my-bucket" {
//!         $list = cos.list ()
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `ibm(account)` — Domain wrapper that sets IBM context
//! - `ibm.config()` — Get current IBM configuration
//! - `ibm.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (IBM COS)
//! - `function.*` — Serverless functions (IBM Cloud Functions)
//! - `instance.*` — Compute instances (IBM VPC)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within an IBM Cloud context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn ibm(account: str; f: any) -> any
/// ```
///
/// ## CREDENTIALS
///
/// Credentials are loaded from:
/// 1. Environment variables: `IBM_API_KEY`
/// 2. `.env` file via `io.read_env` (if `IBM_ENV_FILE` is set)
#[xcl_fn(
    name = "ibm",
    args = "account: str; f: any",
    ret = "any",
    summary = "Run a closure within an IBM Cloud account context."
)]
fn ibm_run(ctx: &mut DomainCtx) -> Value {
    let account = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    let credentials = load_credentials(ctx);

    ctx.push_context("ibm.account", Value::from_str_boxed(account.to_string()));
    ctx.push_context("ibm.credentials", credentials);
    let result = ctx.call(closure, &[]);
    ctx.pop_context("ibm.credentials");
    ctx.pop_context("ibm.account");
    result
}

#[xcl_fn(
    name = "ibm.config",
    ret = "map",
    summary = "Get the current IBM Cloud configuration."
)]
fn ibm_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(acc) = ctx.get_context("ibm.account") {
        config.insert("account".to_string(), acc);
    }
    Value::from_map_boxed(config)
}

#[xcl_fn(
    name = "ibm.credential",
    ret = "map",
    summary = "Get the current IBM Cloud credentials."
)]
fn ibm_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("ibm.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();
    if let Ok(api_key) = std::env::var("IBM_API_KEY") {
        creds.insert("api_key".to_string(), Value::from_str_boxed(api_key));
    } else if let Ok(env_file) = std::env::var("IBM_ENV_FILE")
        && let Ok(content) = std::fs::read_to_string(&env_file) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                if let Some((key, value)) = line.split_once('=')
                    && key.trim() == "IBM_API_KEY" {
                        creds.insert("api_key".to_string(), Value::from_str_boxed(value.trim().to_string()));
                }
            }
        }
    Value::from_map_boxed(creds)
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn ibm_run_sets_account() {
        let r = evaluate_new(r#"
            ibm "my-account" {
                $c = ibm.config

                $c.account
            }
        "#);
        assert_eq!(r.as_str(), Some("my-account"));
    }
}
