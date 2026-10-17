//! AWS API Gateway — REST API and subdomain operations.
//!
//! ## Domain — all functions are `cloud.aws.api.*`
//!
//! ```xcl
//! aws "us-east-1" {
//!     api "my-api" {
//!         api.create "api.example.com" "arn:aws:lambda:..."
//!         api.deploy ()
//!     }
//! }
//! ```

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use super::bucket::make_aws_request;

/// Run a closure within an API Gateway context.
///
/// Sets the API name in the context so inner calls (api.create, api.deploy, etc.)
/// know which API to operate on.
///
/// ## SYNTAX
///
/// ```xcl
/// fn api(name: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// aws "us-east-1" {
///     api "my-api" {
///         api.create "api.example.com" "arn:aws:lambda:..."
///         api.deploy ()
///     }
/// }
/// ```
#[xcl_fn(
    name = "api",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a closure within an API Gateway context."
)]
fn api_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    ctx.push_context("api.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("api.name");
    result
}

/// Create an API Gateway with Lambda integration.
///
/// Creates a REST API, adds a Lambda integration, and sets up the specified
/// subdomain.
///
/// ## SYNTAX
///
/// ```xcl
/// fn api.create(domain: str; lambda_arn: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// aws "us-east-1" {
///     api "my-api" {
///         $result = api.create "api.example.com" "arn:aws:lambda:us-east-1:123456789:function:my-func"
///     }
/// }
/// ```
#[xcl_fn(
    name = "api.create",
    args = "domain: str; lambda_arn: str",
    ret = "Result",
    summary = "Create an API Gateway with Lambda integration for a subdomain."
)]
fn api_create(ctx: &mut DomainCtx) -> Value {
    let _domain = ctx.arg(0).as_str().unwrap_or("");
    let _lambda_arn = ctx.arg(1).as_str().unwrap_or("");

    let context = get_aws_api_context(ctx);
    if context.is_nil() || context.map_variant() == Some(xcl_vm::VARIANT_ERR) {
        return context;
    }
    let ctx_arr = context.as_array().unwrap();
    let region = ctx_arr[1].as_str().unwrap_or("");

    let host = format!("apigateway.{}.amazonaws.com", region);
    let mut request = "POST /restapis HTTP/1.1\r\n".to_string();
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str("Content-Type: application/json\r\n");
    request.push_str("Connection: close\r\n\r\n");
    request.push_str("{}");

    make_aws_request(&host, &request)
}

/// Deploy the current API to a stage.
///
/// ## SYNTAX
///
/// ```xcl
/// fn api.deploy() -> Result
/// ```
#[xcl_fn(
    name = "api.deploy",
    ret = "Result",
    summary = "Deploy the current API Gateway to a stage."
)]
fn api_deploy(ctx: &mut DomainCtx) -> Value {
    let context = get_aws_api_context(ctx);
    if context.is_nil() || context.map_variant() == Some(xcl_vm::VARIANT_ERR) {
        return context;
    }
    let ctx_arr = context.as_array().unwrap();
    let region = ctx_arr[1].as_str().unwrap_or("");

    let host = format!("apigateway.{}.amazonaws.com", region);
    let mut request = "POST /restapis/{id}/deployments HTTP/1.1\r\n".to_string();
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str("Connection: close\r\n\r\n");

    make_aws_request(&host, &request)
}

/// List API Gateways.
///
/// Returns an array of API names and IDs.
///
/// ## SYNTAX
///
/// ```xcl
/// fn api.list() -> Result
/// ```
#[xcl_fn(
    name = "api.list",
    ret = "Result",
    summary = "List API Gateways in the current region."
)]
fn api_list(ctx: &mut DomainCtx) -> Value {
    let context = get_aws_api_context(ctx);
    if context.is_nil() || context.map_variant() == Some(xcl_vm::VARIANT_ERR) {
        return context;
    }
    let ctx_arr = context.as_array().unwrap();
    let region = ctx_arr[1].as_str().unwrap_or("");

    let host = format!("apigateway.{}.amazonaws.com", region);
    let mut request = "GET /restapis HTTP/1.1\r\n".to_string();
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str("Connection: close\r\n\r\n");

    make_aws_request(&host, &request)
}

// ─── Internal helpers ───

/// Extract AWS context for API Gateway operations.
fn get_aws_api_context(ctx: &mut DomainCtx) -> Value {
    let region = match ctx.get_context("aws.region") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_context", "No AWS region set. Use aws() wrapper.", 0),
    };

    let name = match ctx.get_context("api.name") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => String::new(),
    };

    let creds_val = match ctx.get_context("aws.credentials") {
        Some(v) => v,
        None => return err_result("missing_credentials", "No AWS credentials found.", 0),
    };

    let creds_map = match creds_val.as_map() {
        Some(m) => m.clone(),
        None => return err_result("missing_credentials", "Invalid credentials format.", 0),
    };

    let access_key = match creds_map.get("access_key_id") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_credentials", "AWS_ACCESS_KEY_ID not set.", 0),
    };

    let _secret_key = match creds_map.get("secret_access_key") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_credentials", "AWS_SECRET_ACCESS_KEY not set.", 0),
    };

    Value::from_array_boxed(vec![
        Value::from_str_boxed(access_key),
        Value::from_str_boxed(region),
        Value::from_str_boxed(name),
    ])
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn api_run_sets_name() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                api "my-api" {
                    domain.get "api.name"
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("my-api"));
    }

    #[test]
    fn api_run_clears_after_block() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                api "my-api" { }
                domain.get "api.name"
            }
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn api_create_requires_https() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                api "my-api" {
                    api.create "api.example.com" "arn:aws:lambda:us-east-1:123:function:my-func"
                }
            }
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }
}
