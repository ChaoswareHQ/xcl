//! JSON-RPC 2.0 protocol implementation for MCP.
//!
//! Implements the JSON-RPC 2.0 specification as the transport layer for MCP.
//! Provides request/response construction and parsing with automatic
//! XCL Value ↔ JSON-RPC message conversion.
//!
//! ## JSON-RPC 2.0 Message Types
//!
//! - **Request**: `{ jsonrpc: "2.0", id: N, method: "name", params: {...} }`
//! - **Response**: `{ jsonrpc: "2.0", id: N, result: {...} }`
//! - **Error**: `{ jsonrpc: "2.0", id: N, error: { code, message, data } }`
//! - **Notification**: `{ jsonrpc: "2.0", method: "name", params: {...} }` (no id)
//!
//! ## Design
//!
//! - `JsonRpcRequest` / `JsonRpcResponse` are serializable structs
//! - Automatic ID generation for request correlation
//! - XCL values are converted to/from JSON via the encoding layer

use serde::{Serialize, Deserialize};
use xcl_vm::Value;
use crate::encoding::json;

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request.
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        let json_params = params.and_then(|p| json::value_to_json(p));
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params: json_params,
        }
    }

    /// Create a notification (no id, no response expected).
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        let json_params = params.and_then(|p| json::value_to_json(p));
        Self {
            jsonrpc: "2.0".to_string(),
            id: 0,
            method: method.into(),
            params: json_params,
        }
    }

    /// Serialize to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// A JSON-RPC 2.0 error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Parse a JSON-RPC response from a string.
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }

    /// Extract the result as an XCL Value, if present.
    pub fn result_value(&self) -> Option<Value> {
        self.result.as_ref().map(|j| json::json_to_value(j))
    }

    /// Check if this response contains an error.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Extract the error message, if present.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_ref().map(|e| e.message.as_str())
    }
}

/// A JSON-RPC 2.0 notification (server-to-client, no response expected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    /// Parse a notification from a JSON string.
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }

    /// Extract params as an XCL Value.
    pub fn params_value(&self) -> Option<Value> {
        self.params.as_ref().map(|j| json::json_to_value(j))
    }
}

/// A generic JSON-RPC message that can be a request, response, or notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

impl JsonRpcMessage {
    /// Parse a JSON-RPC message from a string.
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }

    /// Serialize to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Standard JSON-RPC 2.0 error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

/// Simple auto-incrementing request ID generator.
pub struct IdGenerator {
    next: std::cell::Cell<u64>,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self { next: std::cell::Cell::new(1) }
    }

    pub fn next(&self) -> u64 {
        let id = self.next.get();
        self.next.set(id.wrapping_add(1));
        id
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_request() {
        let req = JsonRpcRequest::new(1, "test_method", None);
        let json = req.to_json();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"test_method\""));
    }

    #[test]
    fn create_request_with_params() {
        let params = Value::from_i48(42);
        let req = JsonRpcRequest::new(1, "test_method", Some(params));
        let json = req.to_json();
        assert!(json.contains("\"params\":42"));
    }

    #[test]
    fn parse_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"status":"ok"}}"#;
        let resp = JsonRpcResponse::from_json(json).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.result.is_some());
        let val = resp.result_value().unwrap();
        assert_eq!(val.as_map().unwrap().get("status").unwrap().as_str(), Some("ok"));
    }

    #[test]
    fn parse_error_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let resp = JsonRpcResponse::from_json(json).unwrap();
        assert!(resp.is_error());
        assert_eq!(resp.error_message(), Some("Method not found"));
    }

    #[test]
    fn id_generator() {
        let id_gen = IdGenerator::new();
        assert_eq!(id_gen.next(), 1);
        assert_eq!(id_gen.next(), 2);
        assert_eq!(id_gen.next(), 3);
    }

    #[test]
    fn notification_without_id() {
        let req = JsonRpcRequest::notification("initialized/notify", None);
        assert_eq!(req.id, 0);
    }
}
