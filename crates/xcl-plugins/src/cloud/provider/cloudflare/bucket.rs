//! Cloudflare R2 — object storage operations using unified `bucket.*` naming.
//!
//! Uses the Cloudflare API (Bearer token auth) for R2 operations:
//! - List objects: `GET /client/v4/accounts/{account}/r2/buckets/{bucket}/objects`
//! - Get object: `GET /client/v4/accounts/{account}/r2/buckets/{bucket}/objects/{key}`
//! - Put object: `PUT /client/v4/accounts/{account}/r2/buckets/{bucket}/objects/{key}`
//! - Delete object: `DELETE /client/v4/accounts/{account}/r2/buckets/{bucket}/objects/{key}`

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};
use crate::net::tls::https_request;
use std::collections::HashMap;

#[xcl_fn(name = "bucket", args = "name: str; f: any", ret = "any", summary = "Run a closure within a Cloudflare R2 bucket context.")]
fn bucket_run(ctx: &mut DomainCtx) -> Value {
    let bucket = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);
    ctx.push_context("bucket.name", Value::from_str_boxed(bucket.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("bucket.name");
    result
}

#[xcl_fn(name = "bucket.config", ret = "map", summary = "Get the current bucket configuration.")]
fn bucket_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(bucket_val) = ctx.get_context("bucket.name") {
        config.insert("bucket".to_string(), bucket_val);
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

#[xcl_fn(name = "bucket.list", args = "prefix: str", ret = "Result", summary = "List objects in Cloudflare R2.")]
fn bucket_list(ctx: &mut DomainCtx) -> Value {
    let prefix = ctx.arg(0).as_str().unwrap_or("");
    let bucket = match ctx.get_context("bucket.name") {
        Some(b) => b.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_bucket", "No bucket context set", 0),
    };

    let path = if prefix.is_empty() {
        format!("r2/buckets/{}/objects", bucket)
    } else {
        format!("r2/buckets/{}/objects?prefix={}", bucket, prefix)
    };

    match cloudflare_request(ctx, "GET", &path, "") {
        Ok((_status, _headers, body)) => ok_result(Value::from_str_boxed(body)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[xcl_fn(name = "bucket.get", args = "key: str", ret = "Result", summary = "Get an object from Cloudflare R2.")]
fn bucket_get(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    let bucket = match ctx.get_context("bucket.name") {
        Some(b) => b.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_bucket", "No bucket context set", 0),
    };

    let path = format!("r2/buckets/{}/objects/{}", bucket, key);
    match cloudflare_request(ctx, "GET", &path, "") {
        Ok((_status, _headers, body)) => ok_result(Value::from_str_boxed(body)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[xcl_fn(name = "bucket.put", args = "key: str; value: str", ret = "Result", summary = "Put an object into Cloudflare R2.")]
fn bucket_put(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    let value = ctx.arg(1).as_str().unwrap_or("");
    let bucket = match ctx.get_context("bucket.name") {
        Some(b) => b.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_bucket", "No bucket context set", 0),
    };

    let path = format!("r2/buckets/{}/objects/{}", bucket, key);
    match cloudflare_request(ctx, "PUT", &path, value) {
        Ok((_status, _headers, body)) => ok_result(Value::from_str_boxed(body)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[xcl_fn(name = "bucket.delete", args = "key: str", ret = "Result", summary = "Delete an object from Cloudflare R2.")]
fn bucket_delete(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    let bucket = match ctx.get_context("bucket.name") {
        Some(b) => b.as_str().unwrap_or("").to_string(),
        None => return err_result("missing_bucket", "No bucket context set", 0),
    };

    let path = format!("r2/buckets/{}/objects/{}", bucket, key);
    match cloudflare_request(ctx, "DELETE", &path, "") {
        Ok((_status, _headers, body)) => ok_result(Value::from_str_boxed(body)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[xcl_fn(name = "bucket.create", args = "name: str", ret = "Result", summary = "Create a new Cloudflare R2 bucket.")]
fn bucket_create(ctx: &mut DomainCtx) -> Value {
    let name = ctx.arg(0).as_str().unwrap_or("");
    if name.is_empty() {
        return err_result("missing_name", "Bucket name is required", 0);
    }
    let body = format!(r#"{{"name":"{}"}}"#, name);
    match cloudflare_request(ctx, "POST", "r2/buckets", &body) {
        Ok((_status, _headers, resp)) => ok_result(Value::from_str_boxed(resp)),
        Err(e) => err_result("api_error", &e, 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn bucket_list_missing_bucket() {
        let result = evaluate_new(r#"bucket.list """#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn bucket_get_missing_bucket() {
        let result = evaluate_new(r#"bucket.get "key""#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn bucket_put_missing_bucket() {
        let result = evaluate_new(r#"bucket.put "key" "value""#);
        assert_eq!(result.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }
}
