//! Google Cloud Platform provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.gcp.*`
//!
//! ```xcl
//! # GCP with domain expansion — unified service names
//! gcp "my-project" {
//!     bucket "my-bucket" {
//!         $list = bucket.list ()
//!     }
//!     function "my-function" {
//!         $result = function.invoke { input: "test" }
//!     }
//! }
//! ```
//!
//! ## Structure
//!
//! - `gcp(project)` — Domain wrapper that sets GCP context
//! - `gcp.config()` — Get current GCP configuration
//! - `gcp.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (GCP Cloud Storage)
//! - `function.*` — Serverless functions (GCP Cloud Functions)
//! - `instance.*` — Compute instances (GCP Compute Engine)

pub mod bucket;
pub mod function;
pub mod instance;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within a GCP context.
///
/// Sets the GCP project and region, and loads credentials from environment
/// variables or `.env` file. Inner calls (bucket, function, instance) read this context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn gcp(project: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// gcp "my-project" {
///     bucket "my-bucket" {
///         $list = bucket.list ()
///     }
/// }
/// ```
///
/// ## CREDENTIALS
///
/// Credentials are loaded automatically from:
/// 1. Environment variables: `GOOGLE_APPLICATION_CREDENTIALS` (path to JSON key file)
/// 2. `.env` file via `io.read_env` (if `GCP_ENV_FILE` is set)
///
/// ## NESTING
///
/// ```xcl
/// gcp "project-a" {
///     gcp "project-b" {
///         # Uses project-b
///     }
///     # Uses project-a again
/// }
/// ```
#[xcl_fn(
    name = "gcp",
    args = "project: str; f: any",
    ret = "any",
    summary = "Run a closure within a GCP project context."
)]
fn gcp_run(ctx: &mut DomainCtx) -> Value {
    let project = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    // Load credentials
    let credentials = load_credentials(ctx);

    // Set GCP context
    ctx.push_context("gcp.project", Value::from_str_boxed(project.to_string()));
    ctx.push_context("gcp.credentials", credentials);

    let result = ctx.call(closure, &[]);

    ctx.pop_context("gcp.credentials");
    ctx.pop_context("gcp.project");
    result
}

/// Get the current GCP configuration.
///
/// Returns a map with `project` and credential info.
///
/// ## SYNTAX
///
/// ```xcl
/// fn gcp.config() -> map
/// ```
#[xcl_fn(
    name = "gcp.config",
    ret = "map",
    summary = "Get the current GCP configuration."
)]
fn gcp_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();

    if let Some(project_val) = ctx.get_context("gcp.project") {
        config.insert("project".to_string(), project_val);
    }

    if let Some(creds_val) = ctx.get_context("gcp.credentials")
        && let Some(creds_map) = creds_val.as_map()
        && let Some(key) = creds_map.get("client_email") {
            config.insert("client_email".to_string(), *key);
        }

    Value::from_map_boxed(config)
}

/// Get the current GCP credentials.
///
/// Returns a map with `client_email`, `private_key_id`, and `token_uri`.
///
/// ## SYNTAX
///
/// ```xcl
/// fn gcp.credential() -> map
/// ```
#[xcl_fn(
    name = "gcp.credential",
    ret = "map",
    summary = "Get the current GCP credentials."
)]
fn gcp_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("gcp.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

/// Load GCP credentials from environment variables or `.env` file.
fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();

    // Try GOOGLE_APPLICATION_CREDENTIALS (path to JSON key file)
    if let Ok(key_path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        && let Ok(_content) = std::fs::read_to_string(&key_path) {
            // TODO: Parse JSON service account key file
            // For now, store the path as a marker
            creds.insert("key_file".to_string(), Value::from_str_boxed(key_path));
        } else {
        // Try .env file if GCP_ENV_FILE is set
        if let Ok(env_file) = std::env::var("GCP_ENV_FILE")
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
                            "GOOGLE_CLIENT_EMAIL" => {
                                creds.insert("client_email".to_string(), Value::from_str_boxed(value.to_string()));
                            }
                            "GOOGLE_PRIVATE_KEY_ID" => {
                                creds.insert("private_key_id".to_string(), Value::from_str_boxed(value.to_string()));
                            }
                            "GOOGLE_TOKEN_URI" => {
                                creds.insert("token_uri".to_string(), Value::from_str_boxed(value.to_string()));
                            }
                            _ => {}
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
    fn gcp_run_sets_project() {
        let r = evaluate_new(r#"
            gcp "my-project" {
                gcp.config ().project
            }
        "#);
        assert_eq!(r.as_str(), Some("my-project"));
    }

    #[test]
    fn gcp_run_clears_after_block() {
        let r = evaluate_new(r#"
            gcp "my-project" { }
            gcp.config ().project
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn gcp_nesting_works() {
        let r = evaluate_new(r#"
            gcp "project-a" {
                gcp "project-b" {
                    gcp.config ().project
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("project-b"));
    }
}
