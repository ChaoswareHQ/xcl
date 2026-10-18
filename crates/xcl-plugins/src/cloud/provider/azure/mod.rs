//! Microsoft Azure provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.azure.*`
//!
//! ```xcl
//! # Azure with domain expansion
//! azure "my-subscription" {
//!     storage "my-storage" {
//!         $list = storage.list ()
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `azure(subscription)` — Domain wrapper that sets Azure context
//! - `azure.config()` — Get current Azure configuration
//! - `azure.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (Azure Blob Storage)
//! - `function.*` — Serverless functions (Azure Functions)
//! - `instance.*` — Compute instances (Azure VM)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within an Azure context.
///
/// Sets the Azure subscription and loads credentials from environment
/// variables or `.env` file. Inner calls (bucket, function, instance) read this context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn azure(subscription: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// azure "my-subscription" {
///     bucket "my-storage" {
///         $list = bucket.list
///     }
/// }
/// ```
///
/// ## CREDENTIALS
///
/// Credentials are loaded automatically from:
/// 1. Environment variables: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`
/// 2. `.env` file via `io.read_env` (if `AZURE_ENV_FILE` is set)
///
/// ## NESTING
///
/// ```xcl
/// azure "sub-a" {
///     azure "sub-b" {
///         # Uses sub-b
///     }
///     # Uses sub-a again
/// }
/// ```
#[xcl_fn(name = "azure")]
fn azure_run(ctx: &mut DomainCtx) -> Value {
    let subscription = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    // Load credentials
    let credentials = load_credentials(ctx);

    // Set Azure context
    ctx.push_context("azure.subscription", Value::from_str_boxed(subscription.to_string()));
    ctx.push_context("azure.credentials", credentials);

    let result = ctx.call(closure, &[]);

    ctx.pop_context("azure.credentials");
    ctx.pop_context("azure.subscription");
    result
}

/// Get the current Azure configuration.
///
/// Returns a map with `subscription` and credential info.
///
/// ## SYNTAX
///
/// ```xcl
/// fn azure.config() -> map
/// ```
#[xcl_fn]
fn azure_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();

    if let Some(sub_val) = ctx.get_context("azure.subscription") {
        config.insert("subscription".to_string(), sub_val);
    }

    if let Some(creds_val) = ctx.get_context("azure.credentials")
        && let Some(creds_map) = creds_val.as_map()
        && let Some(tenant) = creds_map.get("tenant_id") {
            config.insert("tenant_id".to_string(), *tenant);
        }

    Value::from_map_boxed(config)
}

/// Get the current Azure credentials.
///
/// Returns a map with `client_id`, `client_secret`, and `tenant_id`.
///
/// ## SYNTAX
///
/// ```xcl
/// fn azure.credential() -> map
/// ```
#[xcl_fn]
fn azure_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("azure.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

/// Load Azure credentials from environment variables or `.env` file.
fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();

    // Try environment variables first
    let client_id = std::env::var("AZURE_CLIENT_ID").ok();
    let client_secret = std::env::var("AZURE_CLIENT_SECRET").ok();
    let tenant_id = std::env::var("AZURE_TENANT_ID").ok();

    match (client_id, client_secret, tenant_id) {
        (Some(cid), Some(csec), Some(tid)) => {
            creds.insert("client_id".to_string(), Value::from_str_boxed(cid));
            creds.insert("client_secret".to_string(), Value::from_str_boxed(csec));
            creds.insert("tenant_id".to_string(), Value::from_str_boxed(tid));
        }
        _ => {
            // Try .env file if AZURE_ENV_FILE is set
            if let Ok(env_file) = std::env::var("AZURE_ENV_FILE")
                && let Ok(content) = std::fs::read_to_string(&env_file) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        if let Some((key, value)) = line.split_once('=') {
                            let key = key.trim();
                            let value = value.trim();
                            match key {
                                "AZURE_CLIENT_ID" => {
                                    creds.insert("client_id".to_string(), Value::from_str_boxed(value.to_string()));
                                }
                                "AZURE_CLIENT_SECRET" => {
                                    creds.insert("client_secret".to_string(), Value::from_str_boxed(value.to_string()));
                                }
                                "AZURE_TENANT_ID" => {
                                    creds.insert("tenant_id".to_string(), Value::from_str_boxed(value.to_string()));
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
    fn azure_run_sets_subscription() {
        let r = evaluate_new(r#"
            azure "my-sub" {
                $c = azure.config

                $c.subscription
            }
        "#);
        assert_eq!(r.as_str(), Some("my-sub"));
    }

    #[test]
    fn azure_run_clears_after_block() {
        let r = evaluate_new(r#"
            azure "my-sub" { }
            azure.config.subscription
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn azure_nesting_works() {
        let r = evaluate_new(r#"
            azure "sub-a" {
                azure "sub-b" {
                    $c = azure.config

                    $c.subscription
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("sub-b"));
    }
}
