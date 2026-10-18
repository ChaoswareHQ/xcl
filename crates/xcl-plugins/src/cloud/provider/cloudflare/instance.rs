//! Cloudflare Durable Objects — compute instance operations using unified `instance.*` naming.
//!
//! Uses the Cloudflare API (Bearer token auth) for Durable Objects operations:
//! - List namespaces: `GET /client/v4/accounts/{account}/workers/durable_objects/namespaces`
//! - Create namespace: `PUT /client/v4/accounts/{account}/workers/durable_objects/namespaces`

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};
use crate::net::tls::https_request;
use std::collections::HashMap;

#[xcl_fn(impl_type = "CloudflareProvider", override_method = "instance", name = "instance", args = "name: str; f: any", ret = "any", summary = "Run a closure within a Cloudflare Durable Object context.")]
pub fn instance_run(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    ctx.push_context("instance.name", Value::from_str_boxed(name.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("instance.name");
    result
}

#[xcl_fn(impl_type = "CloudflareProvider", override_method = "instance_config", name = "instance.config", ret = "map", summary = "Get the current instance configuration.")]
pub fn instance_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(inst_val) = ctx.get_context("instance.name") {
        config.insert("instance".to_string(), inst_val);
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

#[xcl_fn(impl_type = "CloudflareProvider", override_method = "instance_create", name = "instance.create", args = "config: map", ret = "Result", summary = "Create a Cloudflare Durable Object namespace.")]
pub fn instance_create(ctx: &mut DomainCtx) -> Value {
    let config = ctx.arg(0);
    let name = match config.as_map() {
        Some(m) => m.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        None => "",
    };

    if name.is_empty() {
        return err_result("missing_name", "Durable Object namespace name is required", 0);
    }

    let body = format!(r#"{{"name":"{}"}}"#, name);
    match cloudflare_request(ctx, "PUT", "workers/durable_objects/namespaces", &body) {
        Ok((status, _headers, resp_body)) => {
            if (200..300).contains(&status) {
                ok_result(Value::from_str_boxed(resp_body))
            } else {
                err_result("http_error", &format!("http status: {}", status), status)
            }
        }
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[xcl_fn(impl_type = "CloudflareProvider", override_method = "instance_list", name = "instance.list", ret = "Result", summary = "List Cloudflare Durable Object namespaces.")]
pub fn instance_list(ctx: &mut DomainCtx) -> Value {
    match cloudflare_request(ctx, "GET", "workers/durable_objects/namespaces", "") {
        Ok((status, _headers, body)) => {
            if (200..300).contains(&status) {
                ok_result(Value::from_str_boxed(body))
            } else {
                err_result("http_error", &format!("http status: {}", status), status)
            }
        }
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn instance_create_missing_name() {
        let result = evaluate_new(r#"instance.create {}"#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn instance_list_missing_account() {
        let result = evaluate_new(r#"instance.list"#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }
}
