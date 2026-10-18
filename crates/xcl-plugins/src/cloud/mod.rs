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
//!         $list = bucket.list
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
//!         $list = bucket.list
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
use xcl_macros::{xcl_fn, xcl_trait, xcl_impl};
use xcl_vm::{Value, DomainCtx};

/// Cloud provider service interface for bucket, function, and instance operations.
///
/// Defines the contract that all cloud providers must implement. Each provider
/// (Cloudflare, AWS, GCP, etc.) implements this trait with its own API-specific
/// logic while maintaining the same method signatures.
#[xcl_trait]
pub trait CloudService {
    fn bucket();
    fn bucket_config();
    fn bucket_list();
    fn bucket_get();
    fn bucket_put();
    fn bucket_delete();
    fn bucket_create();
    fn function();
    fn function_config();
    fn function_invoke();
    fn function_list();
    fn instance();
    fn instance_config();
    fn instance_create();
    fn instance_list();
    fn api();
    fn api_create();
    fn api_deploy();
    fn api_list();
}

/// Cloudflare implements the CloudService trait with R2, Workers, and Durable Objects.
#[derive(xcl_macros::Xcl)]
pub struct CloudflareProvider;

#[xcl_impl]
impl CloudService for CloudflareProvider {
    fn bucket() {}
    fn bucket_config() {}
    fn bucket_list() {}
    fn bucket_get() {}
    fn bucket_put() {}
    fn bucket_delete() {}
    fn bucket_create() {}
    fn function() {}
    fn function_config() {}
    fn function_invoke() {}
    fn function_list() {}
    fn instance() {}
    fn instance_config() {}
    fn instance_create() {}
    fn instance_list() {}
    fn api() {}
    fn api_create() {}
    fn api_deploy() {}
    fn api_list() {}
}

/// AWS implements the CloudService trait with S3, Lambda, and API Gateway.
#[derive(xcl_macros::Xcl)]
pub struct AwsProvider;

#[xcl_impl]
impl CloudService for AwsProvider {
    fn bucket() {}
    fn bucket_config() {}
    fn bucket_list() {}
    fn bucket_get() {}
    fn bucket_put() {}
    fn bucket_delete() {}
    fn bucket_create() {}
    fn function() {}
    fn function_config() {}
    fn function_invoke() {}
    fn function_list() {}
    fn instance() {}
    fn instance_config() {}
    fn instance_create() {}
    fn instance_list() {}
    fn api() {}
    fn api_create() {}
    fn api_deploy() {}
    fn api_list() {}
}

/// Run a closure within a cloud provider context.
///
/// Sets the provider name and region, runs the closure, then clears
/// the context. Used by provider-specific wrappers like `aws(region)`.
///
/// ## SYNTAX
///
/// ```xcl
/// fn cloud.run(provider: str; region: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// cloud.run "aws" "us-east-1" {
///     $provider = cloud.get
///     print $provider
/// }
/// ```
#[xcl_fn(
    name = "cloud.run",
    args = "provider: str; region: str; f: any",
    ret = "any",
    summary = "Run a closure within a cloud provider context scope."
)]
fn cloud_run(ctx: &mut DomainCtx) -> Value {
    let provider = ctx.arg(0).as_str().unwrap_or("");
    let region = ctx.arg(1).as_str().unwrap_or("");
    let closure = ctx.arg(2);

    // Set provider context
    ctx.push_context("cloud.provider", Value::from_str_boxed(provider.to_string()));
    let mut config: HashMap<String, Value> = HashMap::new();
    config.insert("region".to_string(), Value::from_str_boxed(region.to_string()));
    ctx.push_context("cloud.config", Value::from_map_boxed(config));

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
            cloud.run "aws" "us-east-1" {
                $p = cloud.get
                $p
            }
        "#);
        assert_eq!(r.as_str(), Some("aws"));
    }

    #[test]
    fn cloud_run_clears_after_block() {
        let r = evaluate_new(r#"
            cloud.run "aws" "us-east-1" { }
            cloud.get
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn cloud_config_returns_map() {
        let r = evaluate_new(r#"
            cloud.run "aws" "us-east-1" {
                $c = cloud.config
                $c
            }
        "#);
        let map = r.as_map().expect("cloud.config should return a map");
        assert_eq!(map.get("region").unwrap().as_str(), Some("us-east-1"));
    }

    #[test]
    fn cloud_nesting_works() {
        let r = evaluate_new(r#"
            cloud.run "aws" "us-east-1" {
                cloud.run "aws" "eu-west-1" {
                    $c = cloud.config

                    $c.region
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
                    $c = bucket.config

                    $c.bucket
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
                    $c = function.config

                    $c.function
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("my-fn"));
    }

    #[test]
    fn cloudflare_bucket_uses_cloudflare_impl() {
        // bucket.config inside cloudflare {} should use Cloudflare's impl, not AWS's
        let r = evaluate_new(r#"
            cloudflare "test-account" {
                bucket "my-bucket" {
                    $c = bucket.config
                    $c
                }
            }
        "#);
        let map = r.as_map().expect("bucket.config should return a map");
        assert_eq!(map.get("bucket").unwrap().as_str(), Some("my-bucket"));
    }
}
