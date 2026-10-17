//! Oracle Cloud Infrastructure (OCI) provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.oci.*`
//!
//! ```xcl
//! # OCI with domain expansion
//! oci "my-tenancy" {
//!     objectstorage "my-namespace" {
//!         $list = objectstorage.list ()
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `oci(tenancy)` — Domain wrapper that sets OCI context
//! - `oci.config()` — Get current OCI configuration
//! - `oci.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (OCI Object Storage)
//! - `function.*` — Serverless functions (OCI Functions)
//! - `instance.*` — Compute instances (OCI Compute)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within an OCI context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn oci(tenancy: str; f: any) -> any
/// ```
///
/// ## CREDENTIALS
///
/// Credentials are loaded from:
/// 1. Environment variables: `OCI_USER_ID`, `OCI_TENANCY_ID`, `OCI_FINGERPRINT`, `OCI_PRIVATE_KEY_PATH`
/// 2. `.env` file via `io.read_env` (if `OCI_ENV_FILE` is set)
#[xcl_fn(
    name = "oci",
    args = "tenancy: str; f: any",
    ret = "any",
    summary = "Run a closure within an OCI tenancy context."
)]
fn oci_run(ctx: &mut DomainCtx) -> Value {
    let tenancy = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    let credentials = load_credentials(ctx);

    ctx.push_context("oci.tenancy", Value::from_str_boxed(tenancy.to_string()));
    ctx.push_context("oci.credentials", credentials);
    let result = ctx.call(closure, &[]);
    ctx.pop_context("oci.credentials");
    ctx.pop_context("oci.tenancy");
    result
}

#[xcl_fn(
    name = "oci.config",
    ret = "map",
    summary = "Get the current OCI configuration."
)]
fn oci_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(ten) = ctx.get_context("oci.tenancy") {
        config.insert("tenancy".to_string(), ten);
    }
    Value::from_map_boxed(config)
}

#[xcl_fn(
    name = "oci.credential",
    ret = "map",
    summary = "Get the current OCI credentials."
)]
fn oci_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("oci.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();
    let user = std::env::var("OCI_USER_ID").ok();
    let tenancy = std::env::var("OCI_TENANCY_ID").ok();
    let fingerprint = std::env::var("OCI_FINGERPRINT").ok();
    let key_path = std::env::var("OCI_PRIVATE_KEY_PATH").ok();

    match (user, tenancy, fingerprint, key_path) {
        (Some(u), Some(t), Some(f), Some(k)) => {
            creds.insert("user_id".to_string(), Value::from_str_boxed(u));
            creds.insert("tenancy_id".to_string(), Value::from_str_boxed(t));
            creds.insert("fingerprint".to_string(), Value::from_str_boxed(f));
            creds.insert("private_key_path".to_string(), Value::from_str_boxed(k));
        }
        _ => {
            if let Ok(env_file) = std::env::var("OCI_ENV_FILE")
                && let Ok(content) = std::fs::read_to_string(&env_file) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') { continue; }
                        if let Some((key, value)) = line.split_once('=') {
                            match key.trim() {
                                "OCI_USER_ID" => {
                                    creds.insert("user_id".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                "OCI_TENANCY_ID" => {
                                    creds.insert("tenancy_id".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                "OCI_FINGERPRINT" => {
                                    creds.insert("fingerprint".to_string(), Value::from_str_boxed(value.trim().to_string()));
                                }
                                "OCI_PRIVATE_KEY_PATH" => {
                                    creds.insert("private_key_path".to_string(), Value::from_str_boxed(value.trim().to_string()));
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
    fn oci_run_sets_tenancy() {
        let r = evaluate_new(r#"
            oci "my-tenancy" {
                oci.config ().tenancy
            }
        "#);
        assert_eq!(r.as_str(), Some("my-tenancy"));
    }

    #[test]
    fn oci_nesting_works() {
        let r = evaluate_new(r#"
            oci "tenancy-a" {
                oci "tenancy-b" {
                    oci.config ().tenancy
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("tenancy-b"));
    }
}
