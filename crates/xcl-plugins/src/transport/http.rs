//! HTTP/HTTPS client for XCL.
//!
//! Provides synchronous HTTP requests with TLS support via `ureq`.
//! Supports GET, POST, PUT, DELETE, PATCH methods with JSON body handling.

use xcl_macros::xcl_fn;
use xcl_vm::Value;
use crate::core::result::{ok_result, err_result};
use crate::encoding::json;

/// Make an HTTP GET request and return the response body as a string.
#[xcl_fn(name = "https.get", args = "url: str", ret = "Result")]
pub fn https_get(url: &str) -> Value {
    match ureq::get(url).call() {
        Ok(resp) => {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            ok_result(Value::from_str_boxed(body))
        }
        Err(e) => err_result("network_error", &format!("GET {url}: {e}"), 0),
    }
}

/// Make an HTTP POST request with a JSON body.
#[xcl_fn(name = "https.post", args = "url: str; body: any", ret = "Result")]
pub fn https_post(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let url = ctx.arg(0).as_str().unwrap_or("");
    let body = ctx.arg(1);

    let json_str = match json::to_json_string(body) {
        Some(s) => s,
        None => body.as_string(),
    };

    match ureq::post(url)
        .header("Content-Type", "application/json")
        .send(&json_str)
    {
        Ok(resp) => {
            let response_body = resp.into_body().read_to_string().unwrap_or_default();
            ok_result(Value::from_str_boxed(response_body))
        }
        Err(e) => err_result("network_error", &format!("POST {url}: {e}"), 0),
    }
}

/// Make an HTTP PUT request with a JSON body.
#[xcl_fn(name = "https.put", args = "url: str; body: any", ret = "Result")]
pub fn https_put(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let url = ctx.arg(0).as_str().unwrap_or("");
    let body = ctx.arg(1);

    let json_str = match json::to_json_string(body) {
        Some(s) => s,
        None => body.as_string(),
    };

    match ureq::put(url)
        .header("Content-Type", "application/json")
        .send(&json_str)
    {
        Ok(resp) => {
            let response_body = resp.into_body().read_to_string().unwrap_or_default();
            ok_result(Value::from_str_boxed(response_body))
        }
        Err(e) => err_result("network_error", &format!("PUT {url}: {e}"), 0),
    }
}

/// Make an HTTP DELETE request.
#[xcl_fn(name = "https.delete", args = "url: str", ret = "Result")]
pub fn https_delete(url: &str) -> Value {
    match ureq::delete(url).call() {
        Ok(resp) => {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            ok_result(Value::from_str_boxed(body))
        }
        Err(e) => err_result("network_error", &format!("DELETE {url}: {e}"), 0),
    }
}

/// Make an HTTP request with custom method, headers, and body.
#[xcl_fn(name = "https.request", args = "method: str; url: str; headers: dict; body: any", ret = "Result")]
pub fn https_request(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let method = ctx.arg(0).as_str().unwrap_or("GET");
    let url = ctx.arg(1).as_str().unwrap_or("");
    let headers = ctx.arg(2);
    let body = ctx.arg(3);

    let result: Result<String, ureq::Error> = if !body.is_nil() {
        let body_str = match json::to_json_string(body) {
            Some(s) => s,
            None => body.as_string(),
        };
        let mut builder = match method.to_uppercase().as_str() {
            "POST" => ureq::post(url),
            "PUT" => ureq::put(url),
            "PATCH" => ureq::patch(url),
            _ => return err_result("invalid_method", &format!("Unknown HTTP method for body: {method}"), 0),
        };
        if let Some(headers_map) = headers.as_map() {
            for (key, val) in headers_map {
                if let Some(val_str) = val.as_str() {
                    builder = builder.header(key, val_str);
                }
            }
        }
        builder.header("Content-Type", "application/json")
            .send(&body_str)
            .map(|resp| resp.into_body().read_to_string().unwrap_or_default())
    } else {
        let mut builder = match method.to_uppercase().as_str() {
            "GET" => ureq::get(url),
            "DELETE" => ureq::delete(url),
            "HEAD" => ureq::head(url),
            _ => return err_result("invalid_method", &format!("Unknown HTTP method: {method}"), 0),
        };
        if let Some(headers_map) = headers.as_map() {
            for (key, val) in headers_map {
                if let Some(val_str) = val.as_str() {
                    builder = builder.header(key, val_str);
                }
            }
        }
        builder.call().map(|resp| resp.into_body().read_to_string().unwrap_or_default())
    };

    match result {
        Ok(response_body) => ok_result(Value::from_str_boxed(response_body)),
        Err(e) => err_result("network_error", &format!("{method} {url}: {e}"), 0),
    }
}

/// Parse an HTTP response string as JSON.
#[xcl_fn(name = "https.json", args = "response: str", ret = "any")]
pub fn https_json(response: &str) -> Value {
    json::parse_json(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_json_parses_response() {
        let v = https_json(r#"{"status": "ok", "count": 5}"#);
        assert!(v.as_map().is_some());
        assert_eq!(v.as_map().unwrap().get("status").unwrap().as_str(), Some("ok"));
        assert_eq!(v.as_map().unwrap().get("count").unwrap().as_i48(), 5);
    }
}
