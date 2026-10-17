//! Cloudflare Workers — serverless function operations using unified `function.*` naming.
//!
//! Uses the Cloudflare API (Bearer token auth) for Workers operations:
//! - List workers: `GET /client/v4/accounts/{account}/workers/scripts`
//! - Invoke worker: `POST /client/v4/accounts/{account}/workers/dispatch/namespaces/{namespace}/scripts/{script}/invoke`

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};
use crate::net::tls::https_request;
use std::collections::HashMap;

#[xcl_fn(name = "function", args = "name: str; f: any", ret = "any", summary = "Run a closure within a Cloudflare Worker context.")]
fn function_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    ctx.push_context("function.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("function.name");
    result
}

#[xcl_fn(name = "function.config", ret = "map", summary = "Get the current function configuration.")]
fn function_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(fn_val) = ctx.get_context("function.name") {
        config.insert("function".to_string(), fn_val);
    }
    Value::from_map_boxed(config)
}

/// Get Cloudflare credentials from context.
fn get_cloudflare_credentials(ctx: &DomainCtx) -> Option<HashMap<String, Value>> {
    ctx.get_context("cloudflare.credentials").and_then(|v| v.as_map().cloned())
}

/// Get Cloudflare account from context.
fn get_cloudflare_account(ctx: &DomainCtx) -> Option<String> {
    ctx.get_context("cloudflare.account").and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Get API token from credentials.
fn get_api_token(creds: &HashMap<String, Value>) -> Option<String> {
    creds.get("api_token").and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Make an authenticated request to the Cloudflare API.
fn cloudflare_request(
    ctx: &DomainCtx,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(i64, HashMap<String, String>, String), String> {
    let account = match get_cloudflare_account(ctx) {
        Some(a) => a,
        None => return Err("Missing cloudflare account context".to_string()),
    };
    let creds = match get_cloudflare_credentials(ctx) {
        Some(c) => c,
        None => return Err("Missing cloudflare credentials".to_string()),
    };
    let token = match get_api_token(&creds) {
        Some(t) => t,
        None => return Err("Missing CLOUDFLARE_API_TOKEN".to_string()),
    };

    let url = format!("https://api.cloudflare.com/client/v4/accounts/{}/{}", account, path);
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {}", token));
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    https_request(method, &url, &headers, body)
}

#[xcl_fn(name = "function.invoke", args = "payload: map", ret = "Result", summary = "Invoke a Cloudflare Worker.")]
fn function_invoke(ctx: &mut DomainCtx) -> Value {
    let function = match ctx.get_context("function.name") {
        Some(f) => f.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_function", "No function context set", 0),
    };

    // Build JSON payload from the map argument
    let payload = ctx.arg(0);
    let payload_str = match payload.as_map() {
        Some(m) => {
            // Simple JSON serialization for the payload
            let mut parts = Vec::new();
            for (k, v) in m {
                if let Some(vs) = v.as_str() {
                    parts.push(format!(r#""{}":"{}""#, k, vs));
                } else {
                    let vi = v.as_i48();
                    parts.push(format!(r#""{}":{}"#, k, vi));
                }
            }
            format!("{{{}}}", parts.join(","))
        }
        None => "{}".to_string(),
    };

    // Use default dispatch namespace for invocation
    let path = format!("workers/dispatch/namespaces/default/scripts/{}/invoke", function);
    match cloudflare_request(ctx, "POST", &path, &payload_str) {
        Ok((_status, _headers, body)) => ok_result(Value::from_str_boxed(body)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[xcl_fn(name = "function.list", ret = "Result", summary = "List Cloudflare Workers.")]
fn function_list(ctx: &mut DomainCtx) -> Value {
    match cloudflare_request(ctx, "GET", "workers/scripts", "") {
        Ok((_status, _headers, body)) => ok_result(Value::from_str_boxed(body)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn function_invoke_missing_function() {
        let result = evaluate_new(r#"function.invoke {}"#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn function_list_missing_account() {
        let result = evaluate_new(r#"function.list"#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }
}
