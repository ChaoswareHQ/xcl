//! Alibaba Cloud provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.alibaba.*`
//!
//! ```xcl
//! # Alibaba Cloud with domain expansion
//! alibaba "my-account" {
//!     oss "my-bucket" {
//!         $list = oss.list ()
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `alibaba(account)` — Domain wrapper that sets Alibaba context
//! - `alibaba.config()` — Get current Alibaba configuration
//! - `alibaba.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (Alibaba OSS)
//! - `function.*` — Serverless functions (Alibaba FC)
//! - `instance.*` — Compute instances (Alibaba ECS)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within an Alibaba Cloud context.
///
/// Sets the Alibaba account and loads credentials from environment
/// variables or `.env` file.
///
/// ## SYNTAX
///
/// ```xcl
/// fn alibaba(account: str; f: any) -> any
/// ```
///
/// ## CREDENTIALS
///
/// Credentials are loaded from:
/// 1. Environment variables: `ALIBABA_ACCESS_KEY_ID`, `ALIBABA_ACCESS_KEY_SECRET`
/// 2. `.env` file via `io.read_env` (if `ALIBABA_ENV_FILE` is set)
#[xcl_fn(
    name = "alibaba",
    args = "account: str; f: any",
    ret = "any",
    summary = "Run a closure within an Alibaba Cloud account context."
)]
fn alibaba_run(ctx: &mut DomainCtx) -> Value {
    let account = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    let credentials = load_credentials(ctx);

    ctx.push_context("alibaba.account", Value::from_str_boxed(account.to_string()));
    ctx.push_context("alibaba.credentials", credentials);
    let result = ctx.call(closure, &[]);
    ctx.pop_context("alibaba.credentials");
    ctx.pop_context("alibaba.account");
    result
}

/// Get the current Alibaba configuration.
#[xcl_fn(
    name = "alibaba.config",
    ret = "map",
    summary = "Get the current Alibaba Cloud configuration."
)]
fn alibaba_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(acc) = ctx.get_context("alibaba.account") {
        config.insert("account".to_string(), acc);
    }
    Value::from_map_boxed(config)
}

/// Get the current Alibaba credentials.
#[xcl_fn(
    name = "alibaba.credential",
    ret = "map",
    summary = "Get the current Alibaba Cloud credentials."
)]
fn alibaba_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("alibaba.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();
    let ak = std::env::var("ALIBABA_ACCESS_KEY_ID").ok();
    let sk = std::env::var("ALIBABA_ACCESS_KEY_SECRET").ok();
    match (ak, sk) {
        (Some(a), Some(s)) => {
            creds.insert("access_key_id".to_string(), Value::from_str_boxed(a));
            creds.insert("access_key_secret".to_string(), Value::from_str_boxed(s));
        }
        _ => {
            if let Ok(env_file) = std::env::var("ALIBABA_ENV_FILE")
                && let Ok(content) = std::fs::read_to_string(&env_file) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') { continue; }
                        if let Some((key, value)) = line.split_once('=') {
                            match key.trim() {
                                "ALIBABA_ACCESS_KEY_ID" => {
                                    creds.insert("access_key_id".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                "ALIBABA_ACCESS_KEY_SECRET" => {
                                    creds.insert("access_key_secret".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                _ => {}
                            }
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
    fn alibaba_run_sets_account() {
        let r = evaluate_new(r#"
            alibaba "my-account" {
                alibaba.config ().account
            }
        "#);
        assert_eq!(r.as_str(), Some("my-account"));
    }

    #[test]
    fn alibaba_nesting_works() {
        let r = evaluate_new(r#"
            alibaba "acc-a" {
                alibaba "acc-b" {
                    alibaba.config ().account
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("acc-b"));
    }
}
