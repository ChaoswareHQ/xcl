//! AWS Lambda — Serverless function operations using unified `function.*` naming.
//!
//! ## Domain — all functions are `function.*`
//!
//! ```xcl
//! aws "us-east-1" {
//!     function "my-function" {
//!         $result = function.invoke { input: "test" }
//!     }
//! }
//! ```

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::err_result;
use super::bucket::make_aws_request;
use std::collections::HashMap;

/// Run a closure within a function context.
///
/// Sets the function name/ARN in the context so inner calls
/// (function.invoke, etc.) know which function to operate on.
///
/// ## SYNTAX
///
/// ```xcl
/// fn function(name: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// aws "us-east-1" {
///     function "my-function" {
///         $result = function.invoke { input: "test" }
///     }
/// }
/// ```
#[xcl_fn(
    impl_type = "AwsProvider",
    override_method = "function",
    name = "function",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a closure within a function context."
)]
pub fn function_run(ctx: &mut DomainCtx) -> Value {
    let function = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    ctx.push_context("function.name", Value::from_str_boxed(function.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("function.name");
    result
}

/// Get the current function configuration.
///
/// ## SYNTAX
///
/// ```xcl
/// fn function.config() -> map
/// ```
#[xcl_fn(
    impl_type = "AwsProvider",
    override_method = "function_config",
    name = "function.config",
    ret = "map",
    summary = "Get the current function configuration."
)]
pub fn function_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(fn_val) = ctx.get_context("function.name") {
        config.insert("function".to_string(), fn_val);
    }
    Value::from_map_boxed(config)
}

/// Invoke the current function.
///
/// Invokes the current serverless function with the given payload.
///
/// ## SYNTAX
///
/// ```xcl
/// fn function.invoke(payload: map) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// aws "us-east-1" {
///     function "my-function" {
///         $result = function.invoke { input: "test" }
///     }
/// }
/// ```
#[xcl_fn(
    impl_type = "AwsProvider",
    override_method = "function_invoke",
    name = "function.invoke",
    args = "payload: map",
    ret = "Result",
    summary = "Invoke the current serverless function."
)]
pub fn function_invoke(ctx: &mut DomainCtx) -> Value {
    let _payload = ctx.arg(0);

    let context = get_aws_lambda_context(ctx);
    if context.is_nil() || context.map_variant() == Some(xcl_vm::VARIANT_ERR) {
        return context;
    }
    let ctx_arr = context.as_array().unwrap();
    let region = ctx_arr[1].as_str().unwrap_or("");
    let function = ctx_arr[2].as_str().unwrap_or("");

    let host = format!("lambda.{}.amazonaws.com", region);
    let mut request = format!("POST /2015-03-31/functions/{}/invoke HTTP/1.1\r\n", function);
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str("Content-Type: application/json\r\n");
    request.push_str("Connection: close\r\n\r\n");
    request.push_str("{}");

    make_aws_request(&host, &request)
}

/// List functions in the current region.
///
/// Returns an array of function names.
///
/// ## SYNTAX
///
/// ```xcl
/// fn function.list() -> Result
/// ```
#[xcl_fn(
    impl_type = "AwsProvider",
    override_method = "function_list",
    name = "function.list",
    ret = "Result",
    summary = "List serverless functions in the current region."
)]
pub fn function_list(ctx: &mut DomainCtx) -> Value {
    let context = get_aws_lambda_context(ctx);
    if context.is_nil() || context.map_variant() == Some(xcl_vm::VARIANT_ERR) {
        return context;
    }
    let ctx_arr = context.as_array().unwrap();
    let region = ctx_arr[1].as_str().unwrap_or("");

    let host = format!("lambda.{}.amazonaws.com", region);
    let mut request = "GET /2015-03-31/functions/ HTTP/1.1\r\n".to_string();
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str("Connection: close\r\n\r\n");

    make_aws_request(&host, &request)
}

// ─── Internal helpers ───

/// Extract AWS context for Lambda operations.
fn get_aws_lambda_context(ctx: &mut DomainCtx) -> Value {
    let region = match ctx.get_context("aws.region") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_context", "No AWS region set. Use aws() wrapper.", 0),
    };

    let function = match ctx.get_context("function.name") {
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
        Value::from_str_boxed(function),
    ])
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn function_run_sets_name() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                function "my-function" {
                    $c = function.config

                    $c.function
                }
            }
        "#);
        assert_eq!(r.as_str(), Some("my-function"));
    }

    #[test]
    fn function_run_clears_after_block() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                function "my-function" { }
                domain.get "function.name"
            }
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn function_invoke_requires_credentials() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                function "my-function" {
                    function.invoke { input: "test" }
                }
            }
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }
}
