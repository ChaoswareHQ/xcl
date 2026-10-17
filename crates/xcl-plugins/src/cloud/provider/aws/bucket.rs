//! AWS S3 — Object storage operations using unified `bucket.*` naming.
//!
//! ## Domain — all functions are `bucket.*`
//!
//! ```xcl
//! aws "us-east-1" {
//!     bucket "my-bucket" {
//!         $list = bucket.list ()
//!         $data = bucket.get "key.txt"
//!         bucket.put "key.txt" "hello"
//!         bucket.delete "key.txt"
//!     }
//! }
//! ```

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};
use crate::net::tls::https_request;
use super::auth::{self, AwsCredentials};
use std::collections::HashMap;

/// Run a closure within a bucket context.
///
/// Sets the bucket name in the context so inner calls (bucket.list, bucket.get, etc.)
/// know which bucket to operate on.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket(name: str; f: any) -> any
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// aws "us-east-1" {
///     bucket "my-bucket" {
///         $list = bucket.list ()
///         $data = bucket.get "key.txt"
///     }
/// }
/// ```
#[xcl_fn(
    name = "bucket",
    args = "name: str; f: any",
    ret = "any",
    summary = "Run a closure within a bucket context."
)]
fn bucket_run(ctx: &mut DomainCtx) -> Value {
    let bucket = ctx.arg(0).as_str().unwrap_or("");
    let closure = ctx.arg(1);

    ctx.push_context("bucket.name", Value::from_str_boxed(bucket.to_string()));
    let result = ctx.call(closure, &[]);
    ctx.pop_context("bucket.name");
    result
}

/// List objects in the current bucket.
///
/// Returns an array of object keys. Optionally filter by prefix.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket.list(prefix: str) -> Result
/// ```
#[xcl_fn(
    name = "bucket.list",
    args = "prefix: str",
    ret = "Result",
    summary = "List objects in the current bucket."
)]
fn bucket_list(ctx: &mut DomainCtx) -> Value {
    let prefix = ctx.arg(0).as_str().unwrap_or("");

    let (credentials, region, bucket) = match get_aws_context(ctx) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let query = if prefix.is_empty() {
        String::new()
    } else {
        format!("?prefix={}", prefix)
    };

    let host = format!("{}.s3.{}.amazonaws.com", bucket, region);
    let (canonical_headers, signed_headers) = auth::build_canonical_headers(&host, "");

    let (auth_header, date, token) = auth::sign_request(
        &credentials,
        &region,
        "s3",
        "GET",
        &format!("/{}", bucket),
        &query,
        &canonical_headers,
        &signed_headers,
        "",
    );

    let mut request = format!("GET /{} HTTP/1.1\r\n", bucket);
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str(&format!("X-Amz-Date: {}\r\n", date));
    request.push_str(&format!("Authorization: {}\r\n", auth_header));
    if let Some(ref tok) = token {
        request.push_str(&format!("X-Amz-Security-Token: {}\r\n", tok));
    }
    request.push_str("Connection: close\r\n\r\n");

    make_aws_request(&host, &request)
}

/// Get an object from the current bucket.
///
/// Returns the object content as a string.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket.get(key: str) -> Result
/// ```
#[xcl_fn(
    name = "bucket.get",
    args = "key: str",
    ret = "Result",
    summary = "Get an object from the current bucket."
)]
fn bucket_get(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");

    let (credentials, region, bucket) = match get_aws_context(ctx) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let host = format!("{}.s3.{}.amazonaws.com", bucket, region);
    let (canonical_headers, signed_headers) = auth::build_canonical_headers(&host, "");

    let (auth_header, date, token) = auth::sign_request(
        &credentials,
        &region,
        "s3",
        "GET",
        &format!("/{}", key),
        "",
        &canonical_headers,
        &signed_headers,
        "",
    );

    let mut request = format!("GET /{} HTTP/1.1\r\n", key);
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str(&format!("X-Amz-Date: {}\r\n", date));
    request.push_str(&format!("Authorization: {}\r\n", auth_header));
    if let Some(ref tok) = token {
        request.push_str(&format!("X-Amz-Security-Token: {}\r\n", tok));
    }
    request.push_str("Connection: close\r\n\r\n");

    make_aws_request(&host, &request)
}

/// Put an object into the current bucket.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket.put(key: str; value: str) -> Result
/// ```
#[xcl_fn(
    name = "bucket.put",
    args = "key: str; value: str",
    ret = "Result",
    summary = "Put an object into the current bucket."
)]
fn bucket_put(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");
    let value = ctx.arg(1).as_str().unwrap_or("");

    let (credentials, region, bucket) = match get_aws_context(ctx) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let host = format!("{}.s3.{}.amazonaws.com", bucket, region);
    let (canonical_headers, signed_headers) = auth::build_canonical_headers(&host, "application/octet-stream");

    let payload_hash = auth::payload_hash(value);
    let (auth_header, date, token) = auth::sign_request(
        &credentials,
        &region,
        "s3",
        "PUT",
        &format!("/{}", key),
        "",
        &canonical_headers,
        &signed_headers,
        value,
    );

    let mut request = format!("PUT /{} HTTP/1.1\r\n", key);
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str(&format!("X-Amz-Date: {}\r\n", date));
    request.push_str(&format!("Authorization: {}\r\n", auth_header));
    request.push_str("content-type:application/octet-stream\r\n");
    request.push_str(&format!("x-amz-content-sha256: {}\r\n", payload_hash));
    request.push_str(&format!("Content-Length: {}\r\n", value.len()));
    if let Some(ref tok) = token {
        request.push_str(&format!("X-Amz-Security-Token: {}\r\n", tok));
    }
    request.push_str("Connection: close\r\n\r\n");
    request.push_str(value);

    make_aws_request(&host, &request)
}

/// Get the current bucket configuration.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket.config() -> map
/// ```
#[xcl_fn(
    name = "bucket.config",
    ret = "map",
    summary = "Get the current bucket configuration."
)]
fn bucket_config(ctx: &mut DomainCtx) -> Value {
    let mut config = HashMap::new();
    if let Some(bucket_val) = ctx.get_context("bucket.name") {
        config.insert("bucket".to_string(), bucket_val);
    }
    Value::from_map_boxed(config)
}

/// Delete an object from the current bucket.
///
/// ## SYNTAX
///
/// ```xcl
/// fn bucket.delete(key: str) -> Result
/// ```
#[xcl_fn(
    name = "bucket.delete",
    args = "key: str",
    ret = "Result",
    summary = "Delete an object from the current bucket."
)]
fn bucket_delete(ctx: &mut DomainCtx) -> Value {
    let key = ctx.arg(0).as_str().unwrap_or("");

    let (credentials, region, bucket) = match get_aws_context(ctx) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let host = format!("{}.s3.{}.amazonaws.com", bucket, region);
    let (canonical_headers, signed_headers) = auth::build_canonical_headers(&host, "");

    let (auth_header, date, token) = auth::sign_request(
        &credentials,
        &region,
        "s3",
        "DELETE",
        &format!("/{}", key),
        "",
        &canonical_headers,
        &signed_headers,
        "",
    );

    let mut request = format!("DELETE /{} HTTP/1.1\r\n", key);
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str(&format!("X-Amz-Date: {}\r\n", date));
    request.push_str(&format!("Authorization: {}\r\n", auth_header));
    if let Some(ref tok) = token {
        request.push_str(&format!("X-Amz-Security-Token: {}\r\n", tok));
    }
    request.push_str("Connection: close\r\n\r\n");

    make_aws_request(&host, &request)
}

// ─── Internal helpers ───

/// Extract AWS context from domain context.
fn get_aws_context(ctx: &mut DomainCtx) -> Result<(AwsCredentials, String, String), Value> {
    let region = match ctx.get_context("aws.region") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return Err(err_result("missing_context", "No AWS region set. Use aws() wrapper.", 0)),
    };

    let bucket = match ctx.get_context("bucket.name") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return Err(err_result("missing_context", "No bucket set. Use bucket() wrapper.", 0)),
    };

    let creds_val = match ctx.get_context("aws.credentials") {
        Some(v) => v,
        None => return Err(err_result("missing_credentials", "No AWS credentials found.", 0)),
    };

    let creds_map = match creds_val.as_map() {
        Some(m) => m.clone(),
        None => return Err(err_result("missing_credentials", "Invalid credentials format.", 0)),
    };

    let access_key = match creds_map.get("access_key_id") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return Err(err_result("missing_credentials", "AWS_ACCESS_KEY_ID not set.", 0)),
    };

    let secret_key = match creds_map.get("secret_access_key") {
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => return Err(err_result("missing_credentials", "AWS_SECRET_ACCESS_KEY not set.", 0)),
    };

    let session_token = creds_map.get("session_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let credentials = AwsCredentials {
        access_key_id: access_key,
        secret_access_key: secret_key,
        session_token,
    };

    Ok((credentials, region, bucket))
}

/// Make an HTTPS request to an AWS service and return the response.
pub fn make_aws_request(host: &str, request: &str) -> Value {
    // Parse the request to extract method, path, headers, and body
    let first_line_end = request.find("\r\n").unwrap_or(request.len());
    let first_line = &request[..first_line_end];
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    
    if parts.len() < 3 {
        return err_result("parse_error", "Invalid request format", 0);
    }
    
    let method = parts[0];
    let path = parts[1];
    
    // Extract headers and body
    let after_first_line = &request[first_line_end + 2..];
    let body_start = after_first_line.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    let headers_section = &after_first_line[..body_start];
    let body = &after_first_line[body_start..];
    
    // Parse headers
    let mut headers = HashMap::new();
    for line in headers_section.lines() {
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_string();
            let value = line[colon + 1..].trim().to_string();
            headers.insert(key, value);
        }
    }
    
    let url = format!("https://{}{}", host, path);
    
    match https_request(method, &url, &headers, body) {
        Ok((status, resp_headers, resp_body)) => {
            let mut result = HashMap::new();
            result.insert("status".to_string(), Value::from_i48(status));
            let headers_map: HashMap<String, Value> = resp_headers
                .into_iter()
                .map(|(k, v)| (k, Value::from_str_boxed(v)))
                .collect();
            result.insert("headers".to_string(), Value::from_map_boxed(headers_map));
            result.insert("body".to_string(), Value::from_str_boxed(resp_body));
            ok_result(Value::from_map_boxed(result))
        }
        Err(e) => err_result("request_error", &e, 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn bucket_run_sets_name() {
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
    fn bucket_run_clears_after_block() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                bucket "my-bucket" { }
                domain.get "bucket.name"
            }
        "#);
        assert!(r.is_nil());
    }

    #[test]
    fn bucket_list_requires_credentials() {
        let r = evaluate_new(r#"
            aws "us-east-1" {
                bucket "my-bucket" {
                    bucket.list ()
                }
            }
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }
}
