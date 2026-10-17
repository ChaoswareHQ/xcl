//! AWS cloud provider — domain expansion wrapper and service modules.
//!
//! ## Domain — all functions are `cloud.aws.*`
//!
//! ```xcl
//! # AWS with domain expansion — unified service names
//! aws "us-east-1" {
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
//! - `aws(region)` — Domain wrapper that sets AWS context
//! - `aws.config()` — Get current AWS configuration
//! - `aws.credential()` — Get current credentials from env or .env file
//! - `bucket.*` — Object storage (AWS S3)
//! - `function.*` — Serverless functions (AWS Lambda)
//! - `api.*` — API Gateway operations

pub mod auth;
pub mod bucket;
pub mod function;
pub mod api;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within an AWS context.
///
/// Sets the AWS region and loads credentials from environment variables
/// or `.env` file. Inner calls (s3, lambda, api) read this context.
///
/// ## SYNTAX
///
/// ```xcl
/// fn aws(region: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// aws "us-east-1" {
///     s3 "my-bucket" {
///         $list = s3.list ()
///     }
/// }
/// ```
///
/// ## CREDENTIALS
///
/// Credentials are loaded automatically from:
/// 1. Environment variables: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`
/// 2. `.env` file via `io.read_env` (if `AWS_ENV_FILE` is set)
///
/// ## NESTING
///
/// ```xcl
/// aws "us-east-1" {
///     aws "eu-west-1" {
///         # Uses eu-west-1
///     }
///     # Uses us-east-1 again
/// }
/// ```
#[xcl_fn(
    name = "aws",
    args = "region: str; f: any",
    ret = "any",
    summary = "Run a closure within an AWS region context."
)]
fn aws_run(ctx: &mut DomainCtx) -> Value {
    let region = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    // Load credentials
    let credentials = load_credentials(ctx);

    // Set AWS context
    ctx.push_context("aws.region", Value::from_str_boxed(region.to_string()));
    ctx.push_context("aws.credentials", credentials);

    let result = ctx.call(closure, &[]);

    ctx.pop_context("aws.credentials");
    ctx.pop_context("aws.region");
    result
}

/// Get the current AWS configuration.
///
/// Returns a map with `region`, `access_key_id`, and `secret_access_key`.
///
/// ## SYNTAX
///
/// ```xcl
/// fn aws.config() -> map
/// ```
#[xcl_fn(
    name = "aws.config",
    ret = "map",
    summary = "Get the current AWS configuration."
)]
fn aws_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();

    if let Some(region_val) = ctx.get_context("aws.region") {
        config.insert("region".to_string(), region_val);
    }

    if let Some(creds_val) = ctx.get_context("aws.credentials")
        && let Some(creds_map) = creds_val.as_map() {
            if let Some(key) = creds_map.get("access_key_id") {
                config.insert("access_key_id".to_string(), *key);
            }
            if let Some(secret) = creds_map.get("secret_access_key") {
                config.insert("secret_access_key".to_string(), *secret);
            }
        }

    Value::from_map_boxed(config)
}

/// Get the current AWS credentials.
///
/// Returns a map with `access_key_id`, `secret_access_key`, and optionally `session_token`.
///
/// ## SYNTAX
///
/// ```xcl
/// fn aws.credential() -> map
/// ```
#[xcl_fn(
    name = "aws.credential",
    ret = "map",
    summary = "Get the current AWS credentials."
)]
fn aws_credential(ctx: &mut DomainCtx) -> Value {
    match ctx.get_context("aws.credentials") {
        Some(creds) => creds,
        None => {
            let empty: HashMap<String, Value> = HashMap::new();
            Value::from_map_boxed(empty)
        }
    }
}

/// Load AWS credentials from environment variables or `.env` file.
fn load_credentials(_ctx: &mut DomainCtx) -> Value {
    let mut creds = HashMap::new();

    // Try environment variables first
    let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
    let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

    match (access_key, secret_key) {
        (Some(ak), Some(sk)) => {
            creds.insert("access_key_id".to_string(), Value::from_str_boxed(ak));
            creds.insert("secret_access_key".to_string(), Value::from_str_boxed(sk));
            if let Some(st) = session_token {
                creds.insert("session_token".to_string(), Value::from_str_boxed(st));
            }
        }
        _ => {
            // Try .env file if AWS_ENV_FILE is set
            if let Ok(env_file) = std::env::var("AWS_ENV_FILE")
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
                                "AWS_ACCESS_KEY_ID" => {
                                    creds.insert("access_key_id".to_string(), Value::from_str_boxed(value.to_string()));
                                }
                                "AWS_SECRET_ACCESS_KEY" => {
                                    creds.insert("secret_access_key".to_string(), Value::from_str_boxed(value.to_string()));
                                }
                                "AWS_SESSION_TOKEN" => {
                                    creds.insert("session_token".to_string(), Value::from_str_boxed(value.to_string()));
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
    fn aws_run_sets_region() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                aws.config ().region
            }
        "#);
        assert_eq!(r.as_str(), Some("us-east-1"));
    }

    #[test]
    fn aws_run_clears_after_block() {
        let r = evaluate_new(r#"
            aws "us-east-1" { }
            aws.config ().region
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn aws_nesting_works() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                aws "eu-west-1" {
                    aws.config ().region
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("eu-west-1"));
    }

    #[test]
    fn aws_config_returns_map() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                aws.config ()
            }
        "#);
        let map = r.as_map().expect("aws.config should return a map");
        assert_eq!(map.get("region").unwrap().as_str(), Some("us-east-1"));
    }

    #[test]
    fn aws_credential_returns_map() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                aws.credential ()
            }
        "#);
        assert!(r.as_map().is_some());
    }

    #[test]
    fn aws_with_credentials_from_env() {
        // Set test credentials
        unsafe {
            std::env::set_var("AWS_ACCESS_KEY_ID", "test_key");
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "test_secret");
        }

        let r = evaluate_new(r#"
            aws "us-east-1" {
                aws.credential ().access_key_id
            }
        "#);
        assert_eq!(r.as_str(), Some("test_key"));

        // Cleanup
        unsafe {
            std::env::remove_var("AWS_ACCESS_KEY_ID");
            std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        }
    }
}
