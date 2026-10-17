//! Cloudflare provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.cloudflare.*`
//!
//! ```xcl
//! # Cloudflare with domain expansion
//! cloudflare "my-account" {
//!     bucket "my-bucket" {
//!         $list = bucket.list ()
//!     }
//!     function "my-worker" {
//!         $result = function.invoke { input: "test" }
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `cloudflare(account)` — Domain wrapper that sets Cloudflare context
//! - `cloudflare.config()` — Get current Cloudflare configuration
//! - `cloudflare.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (Cloudflare R2)
//! - `function.*` — Serverless functions (Cloudflare Workers)
//! - `instance.*` — Compute instances (Cloudflare Durable Objects)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within a Cloudflare context.
#[xcl_fn(
    name = "cloudflare",
    args = "account: str; f: any",
    ret = "any",
    summary = "Run a closure within a Cloudflare account context."
)]
fn cloudflare_run(ctx: &mut DomainCtx) -> Value {
    let account = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    let credentials = load_credentials(ctx);

    ctx.push_context("cloudflare.account", Value::from_str_boxed(account.to_string()));
    ctx.push_context("cloudflare.credentials", credentials);
    let result = ctx.call(closure, &[]);
    ctx.pop_context("cloudflare.credentials");
    ctx.pop_context("cloudflare.account");
    result
}

/// Get the current Cloudflare configuration.
#[xcl_fn(
    name = "cloudflare.config",
    ret = "map",
    summary = "Get the current Cloudflare configuration."
)]
fn cloudflare_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(acc) = ctx.get_context("cloudflare.account") {
        config.insert("account".to_string(), acc);
    }
    Value::from_map_boxed(config)
}

/// Get the current Cloudflare credentials.
#[xcl_fn(
    name = "cloudflare.credential",
    ret = "map",
    summary = "Get the current Cloudflare credentials."
)]
fn cloudflare_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("cloudflare.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();
    let api_token = std::env::var("CLOUDFLARE_API_TOKEN").ok();
    let api_key = std::env::var("CLOUDFLARE_API_KEY").ok();
    let email = std::env::var("CLOUDFLARE_EMAIL").ok();

    match (api_token, api_key, email) {
        (Some(token), _, _) => {
            creds.insert("api_token".to_string(), Value::from_str_boxed(token));
        }
        (_, Some(key), Some(email_val)) => {
            creds.insert("api_key".to_string(), Value::from_str_boxed(key));
            creds.insert("email".to_string(), Value::from_str_boxed(email_val));
        }
        _ => {
            if let Ok(env_file) = std::env::var("CLOUDFLARE_ENV_FILE")
                && let Ok(content) = std::fs::read_to_string(&env_file) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') { continue; }
                        if let Some((key, value)) = line.split_once('=') {
                            match key.trim() {
                                "CLOUDFLARE_API_TOKEN" => {
                                    creds.insert("api_token".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                "CLOUDFLARE_API_KEY" => {
                                    creds.insert("api_key".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                "CLOUDFLARE_EMAIL" => {
                                    creds.insert("email".to_string(), Value::from_str_boxed(value.trim().to_string()));
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
    fn cloudflare_run_sets_account() {
        let r = evaluate_new(r#"
            cloudflare "my-account" {
                cloudflare.config ().account
            }
        "#);
        assert_eq!(r.as_str(), Some("my-account"));
    }

    #[test]
    fn cloudflare_nesting_works() {
        let r = evaluate_new(r#"
            cloudflare "acc-a" {
                cloudflare "acc-b" {
                    cloudflare.config ().account
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("acc-b"));
    }
}
