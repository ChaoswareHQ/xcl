//! Cloud provider module — unified interface for cloud services.
//!
//! ## Concept
//!
//! The cloud module provides a **unified interface** across all cloud providers.
//! Regardless of whether you use AWS, GCP, Azure, Cloudflare, or GitHub, the
//! service names are always the same:
//!
//! - `bucket` — Object storage (AWS S3, GCP Storage, Azure Blob, etc.)
//! - `function` — Serverless functions (AWS Lambda, GCP Functions, Azure Functions, etc.)
//! - `instance` — Compute instances (EC2, GCE, Azure VM, etc.)
//! - `api` — API gateways and HTTP endpoints
//!
//! This follows the OpenTofu/Terraform pattern: the provider changes, but the
//! resource types stay consistent.
//!
//! ## Example
//!
//! ```xcl
//! # AWS — unified service names
//! aws "us-east-1" {
//!     bucket "my-bucket" {
//!         $list = bucket.list ()
//!         $data = bucket.get "key.txt"
//!     }
//!     function "my-function" {
//!         $result = function.invoke { input: "test" }
//!     }
//! }
//!
//! # GCP — same service names, different provider
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
//! ## Layering
//!
//! - `cloud/` — Provider abstraction and context blocks
//! - `cloud/provider/` — Provider implementations (aws, gcp, azure, cloudflare, github, etc.)
//! - Dependencies flow: `cloud/provider/` → `net/http` → `net/tls`

pub mod provider;

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Run a closure within a cloud provider context.
///
/// Sets the provider name and configuration, runs the closure, then clears
/// the context. Used by provider-specific wrappers like `aws(region)`.
///
/// ## SYNTAX
///
/// ```xcl
/// fn cloud.run(provider: str; config: map; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// cloud.run "aws" { region: "us-east-1" } {
///     $provider = cloud.get ()
///     print $provider
/// }
/// ```
#[xcl_fn(
    name = "cloud.run",
    args = "provider: str; config: map; f: any",
    ret = "any",
    summary = "Run a closure within a cloud provider context scope."
)]
fn cloud_run(ctx: &mut DomainCtx) -> Value {
    let provider = ctx.arg(0).as_str().unwrap_or("");
    let config = ctx.arg(1);
    let closure = ctx.arg(2);

    // Set provider context
    ctx.push_context("cloud.provider", Value::from_str_boxed(provider.to_string()));
    if let Some(map) = config.as_map() {
        let cloned: HashMap<String, Value> = map.iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        ctx.push_context("cloud.config", Value::from_map_boxed(cloned));
    }

    let result = ctx.call(closure, &[]);

    ctx.pop_context("cloud.config");
    ctx.pop_context("cloud.provider");
    result
}

/// Get the current cloud provider name.
///
/// Returns the provider name (e.g., "aws") or `nil` if not set.
///
/// ## SYNTAX
///
/// ```xcl
/// fn cloud.get() -> str
/// ```
#[xcl_fn(
    name = "cloud.get",
    ret = "str",
    summary = "Get the current cloud provider name."
)]
fn cloud_get(ctx: &mut DomainCtx) -> Value {
    ctx.get_context("cloud.provider").unwrap_or(Value::nil())
}

/// Get the current cloud provider configuration.
///
/// Returns a map with provider-specific configuration (region, credentials, etc.)
/// or `nil` if not set.
///
/// ## SYNTAX
///
/// ```xcl
/// fn cloud.config() -> map
/// ```
#[xcl_fn(
    name = "cloud.config",
    ret = "map",
    summary = "Get the current cloud provider configuration map."
)]
fn cloud_config(ctx: &mut DomainCtx) -> Value {
    ctx.get_context("cloud.config").unwrap_or(Value::nil())
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn cloud_run_sets_provider() {
        let r = evaluate_new(r#"
            cloud.run "aws" { region: "us-east-1" } {
                cloud.get ()
            }
        "#);
        assert_eq!(r.as_str(), Some("aws"));
    }

    #[test]
    fn cloud_run_clears_after_block() {
        let r = evaluate_new(r#"
            cloud.run "aws" { region: "us-east-1" } { }
            cloud.get ()
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn cloud_config_returns_map() {
        let r = evaluate_new(r#"
            cloud.run "aws" { region: "us-east-1" } {
                cloud.config ()
            }
        "#);
        let map = r.as_map().expect("cloud.config should return a map");
        assert_eq!(map.get("region").unwrap().as_str(), Some("us-east-1"));
    }

    #[test]
    fn cloud_nesting_works() {
        let r = evaluate_new(r#"
            cloud.run "aws" { region: "us-east-1" } {
                cloud.run "aws" { region: "eu-west-1" } {
                    cloud.config ().region
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("eu-west-1"));
    }

    #[test]
    fn unified_bucket_name() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                bucket "my-bucket" {
                    bucket.config ().bucket
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("my-bucket"));
    }

    #[test]
    fn unified_function_name() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                function "my-fn" {
                    function.config ().function
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("my-fn"));
    }
}
