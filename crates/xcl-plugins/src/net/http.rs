//! HTTPS client — secure HTTP/1.1 via `ureq` with `rustls`.
//!
//! ## Domain — all functions are `net.http.*`
//!
//! ```xcl
//! $r = net.http.get "https://example.com/"
//! $status = $r.value.status
//! $body = $r.value.body
//! ```
//!
//! ## Security
//!
//! **Plain HTTP is not supported.** All requests use HTTPS (TLS 1.2+) via `rustls`.
//! This ensures all network traffic is encrypted by default.
//!
//! ```xcl
//! # This will fail — plain HTTP is insecure
//! $r = net.http.get "http://example.com/"
//! # Error: Plain HTTP is not supported
//!
//! # This works — HTTPS is secure
//! $r = net.http.get "https://example.com/"
//! ```
//!
//! ## Why in `net/` and not `transport/`
//!
//! HTTP is a thin wrapper around `transport::tls::https_request`. The actual TLS
//! connection is handled by `ureq` with `rustls`. This module provides the XCL
//! domain interface for HTTP operations.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};
use crate::net::tls::https_request;

/// Parse headers from a map argument into a HashMap.
fn parse_headers(arg: &Value) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    if let Some(map) = arg.as_map() {
        for (key, val) in map {
            if let Some(v) = val.as_str() {
                headers.insert(key.clone(), v.to_string());
            }
        }
    }
    headers
}

/// Build a Result value from an HTTP response.
fn build_response_result(status: i64, headers: HashMap<String, String>, body: String) -> Value {
    let mut result = HashMap::new();
    result.insert("status".to_string(), Value::from_i48(status));
    let headers_map: HashMap<String, Value> = headers
        .into_iter()
        .map(|(k, v)| (k, Value::from_str_boxed(v)))
        .collect();
    result.insert("headers".to_string(), Value::from_map_boxed(headers_map));
    result.insert("body".to_string(), Value::from_str_boxed(body));
    Value::from_map_boxed(result)
}

/// Perform an HTTP GET request over HTTPS.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.http.get(url: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.http.get "https://example.com/"
/// if $r.is_ok {
///     print $r.value.status   # 200
///     print $r.value.body     # HTML content
/// }
/// ```
///
/// ## RETURNS
///
/// A Result with a map `{status: int, headers: map, body: str}` in the `value` field.
///
/// ## NOTES
///
/// Only HTTPS URLs are supported. Plain HTTP returns an error.
#[xcl_fn(
    name = "net.http.get",
    args = "url: str",
    ret = "Result",
    summary = "Perform an HTTPS GET request and return status, headers, and body."
)]
fn http_get(ctx: &mut DomainCtx) -> Value {
    let url = ctx.arg(0).as_str().unwrap_or("");

    let headers = HashMap::new();
    match https_request("GET", url, &headers, "") {
        Ok((status, headers, body)) => ok_result(build_response_result(status, headers, body)),
        Err(e) => err_result("request_error", &e, 0),
    }
}

/// Perform an HTTP POST request over HTTPS.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.http.post(url: str; body: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.http.post "https://example.com/api" "key=value"
/// print $r.value.status
/// ```
///
/// ## RETURNS
///
/// A Result with a map `{status: int, headers: map, body: str}` in the `value` field.
#[xcl_fn(
    name = "net.http.post",
    args = "url: str; body: str",
    ret = "Result",
    summary = "Perform an HTTPS POST request and return status, headers, and body."
)]
fn http_post(ctx: &mut DomainCtx) -> Value {
    let url = ctx.arg(0).as_str().unwrap_or("");
    let body = ctx.arg(1).as_str().unwrap_or("");

    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string());

    match https_request("POST", url, &headers, body) {
        Ok((status, headers, resp_body)) => {
            ok_result(build_response_result(status, headers, resp_body))
        }
        Err(e) => err_result("request_error", &e, 0),
    }
}

/// Perform an HTTP request with custom method, headers, and body over HTTPS.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.http.request(method: str; url: str; headers: map; body: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.http.request "PUT" "https://example.com/api/1" { Authorization: "Bearer token" } "data"
/// ```
#[xcl_fn(
    name = "net.http.request",
    args = "method: str; url: str; headers: map; body: str",
    ret = "Result",
    summary = "Perform a custom HTTPS request with method, headers, and body."
)]
fn http_request(ctx: &mut DomainCtx) -> Value {
    let method = ctx.arg(0).as_str().unwrap_or("GET");
    let url = ctx.arg(1).as_str().unwrap_or("");
    let headers_arg = ctx.arg(2);
    let body = ctx.arg(3).as_str().unwrap_or("");

    let headers = parse_headers(&headers_arg);

    match https_request(method, url, &headers, body) {
        Ok((status, headers, resp_body)) => {
            ok_result(build_response_result(status, headers, resp_body))
        }
        Err(e) => err_result("request_error", &e, 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn http_get_https_url() {
        // Test that HTTPS URLs are accepted (may fail on network, but not on scheme)
        let result = evaluate_new(r#"net.http.get "https://httpbin.org/get""#);
        let map = result.as_map().expect("http.get should return a map");
        // Either is_ok (network success) or is_err (network failure)
        let is_ok = map.get("is_ok").map(|v| v.is_truthy()).unwrap_or(false);
        let is_err = map.get("is_err").map(|v| v.is_truthy()).unwrap_or(false);
        assert!(is_ok || is_err, "Should return either Ok or Err");
        // If it's an error, it should NOT be a scheme rejection
        if is_err {
            let message = map.get("message").and_then(|v| v.as_str()).unwrap_or("");
            assert!(
                !message.contains("Plain HTTP is not supported"),
                "HTTPS should not be rejected as plain HTTP"
            );
        }
    }

    #[test]
    fn http_get_plain_http_rejected() {
        let result = evaluate_new(r#"net.http.get "http://example.com/""#);
        let map = result.as_map().expect("http.get should return a map");
        assert!(
            map.get("is_err").unwrap().is_truthy(),
            "Plain HTTP should be rejected"
        );
    }

    #[test]
    fn http_post_https_url() {
        let result = evaluate_new(r#"net.http.post "https://httpbin.org/post" "test=1""#);
        let map = result.as_map().expect("http.post should return a map");
        let is_ok = map.get("is_ok").map(|v| v.is_truthy()).unwrap_or(false);
        let is_err = map.get("is_err").map(|v| v.is_truthy()).unwrap_or(false);
        assert!(is_ok || is_err, "Should return either Ok or Err");
        if is_err {
            let message = map.get("message").and_then(|v| v.as_str()).unwrap_or("");
            assert!(
                !message.contains("Plain HTTP is not supported"),
                "HTTPS should not be rejected"
            );
        }
    }

    #[test]
    fn http_post_plain_http_rejected() {
        let result = evaluate_new(r#"net.http.post "http://example.com/" "test""#);
        let map = result.as_map().expect("http.post should return a map");
        assert!(
            map.get("is_err").unwrap().is_truthy(),
            "Plain HTTP should be rejected"
        );
    }

    #[test]
    fn http_request_custom_method() {
        let result = evaluate_new(
            r#"net.http.request "DELETE" "https://httpbin.org/delete" {} ""#,
        );
        let map = result.as_map().expect("http.request should return a map");
        let is_ok = map.get("is_ok").map(|v| v.is_truthy()).unwrap_or(false);
        let is_err = map.get("is_err").map(|v| v.is_truthy()).unwrap_or(false);
        assert!(is_ok || is_err, "Should return either Ok or Err");
    }

    #[test]
    fn http_request_plain_http_rejected() {
        let result = evaluate_new(
            r#"net.http.request "GET" "http://example.com/" {} ""#,
        );
        let map = result.as_map().expect("http.request should return a map");
        assert!(
            map.get("is_err").unwrap().is_truthy(),
            "Plain HTTP should be rejected"
        );
    }
}
