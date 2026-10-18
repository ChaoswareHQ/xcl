//! Cloudflare provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.cloudflare.*`
//!
//! ```xcl
//! # Cloudflare with domain expansion
//! cloudflare "my-account" {
//!     bucket "my-bucket" {
//!         $list = bucket.list
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
    impl_type = "CloudflareProvider",
    name = "cloudflare",
    args = "account: str; f: any",
    ret = "any",
    summary = "Run a closure within a Cloudflare account context."
)]
fn cloudflare_run(ctx: &mut DomainCtx) -> Value {
    let account_arg = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    let credentials = load_credentials(ctx);

    // Use the provided account if it's not empty; otherwise fall back to .env account
    let account = if !account_arg.is_empty() {
        account_arg.to_string()
    } else if let Some(creds_map) = credentials.as_map() {
        creds_map.get("account")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };

    ctx.push_context("cloudflare.account", Value::from_str_boxed(account));
    ctx.push_context("cloudflare.credentials", credentials);

    // Override shared global function names with Cloudflare implementations via impl type lookup
    let saved = ctx.override_by_trait("CloudflareProvider");

    let result = ctx.call(closure, &[]);

    // Restore original global function mappings
    ctx.restore_by_trait(&saved);

    ctx.pop_context("cloudflare.credentials");
    ctx.pop_context("cloudflare.account");
    result
}

/// Get the current Cloudflare configuration.
#[xcl_fn(
    impl_type = "CloudflareProvider",
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
    impl_type = "CloudflareProvider",
    name = "cloudflare.credential",
    ret = "map",
    summary = "Get the current Cloudflare credentials."
)]
fn cloudflare_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("cloudflare.credentials") {
        Some(creds) => creds,
        None => {
            // Load from .env if not in a cloudflare context
            load_credentials(ctx)
        }
    }
}

fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    use crate::io::io_read_env;

    let mut creds = HashMap::new();

    // Load from .env file using io.read_env
    let env_file = std::env::var("CLOUDFLARE_ENV_FILE").ok().unwrap_or_else(|| ".env".to_string());
    let env_result = io_read_env(&env_file);

    // Extract Cloudflare keys from io.read_env result: { variant: "Ok", value: { KEY: "val" } }
    if env_result.map_variant() == Some(xcl_vm::VARIANT_OK)
        && let Some(env_map) = env_result.as_map()
            .and_then(|m| m.get("value"))
            .and_then(|v| v.as_map())
    {
        for (key, out_key) in [
            ("CLOUDFLARE_API_TOKEN", "api_token"),
            ("CLOUDFLARE_API_KEY", "api_key"),
            ("CLOUDFLARE_EMAIL", "email"),
            ("CLOUDFLARE_ACCOUNT", "account"),
        ] {
            if let Some(v) = env_map.get(key)
                && let Some(s) = v.as_str()
            {
                let val = s.trim().to_string();
                creds.insert(out_key.to_string(), Value::from_str_boxed(val.clone()));
                unsafe { std::env::set_var(key, &val); }
            }
        }
    }

    // Override with environment variables if set
    if let Ok(token) = std::env::var("CLOUDFLARE_API_TOKEN") {
        creds.insert("api_token".to_string(), Value::from_str_boxed(token));
    }
    if let Ok(key) = std::env::var("CLOUDFLARE_API_KEY") {
        creds.insert("api_key".to_string(), Value::from_str_boxed(key));
    }
    if let Ok(email_val) = std::env::var("CLOUDFLARE_EMAIL") {
        creds.insert("email".to_string(), Value::from_str_boxed(email_val));
    }
    if let Ok(account) = std::env::var("CLOUDFLARE_ACCOUNT") {
        creds.insert("account".to_string(), Value::from_str_boxed(account));
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
                $c = cloudflare.config
                $c.account
            }
        "#);
        assert_eq!(r.as_str(), Some("my-account"));
    }

    #[test]
    fn cloudflare_nesting_works() {
        let r = evaluate_new(r#"
            cloudflare "acc-a" {
                cloudflare "acc-b" {
                    $c = cloudflare.config
                    $c.account
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("acc-b"));
    }
}
