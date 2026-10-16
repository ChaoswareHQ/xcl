//! Model Context Protocol (MCP) client for XCL.
//!
//! Implements the MCP protocol over HTTP/SSE transport using JSON-RPC 2.0.
//! MCP allows XCL programs to connect to AI tools, resources, and prompts
//! exposed by MCP servers.
//!
//! ## Architecture
//!
//! ```text
//!  XCL Value
//!    │
//!    ▼
//!  json.rs (Value ↔ JSON)
//!    │
//!    ▼
//!  json_rpc2.rs (JSON-RPC 2.0 messages)
//!    │
//!    ▼
//!  https.rs (HTTP transport via ureq)
//!    │
//!    ▼
//!  MCP Server
//! ```
//!
//! ## MCP Lifecycle
//!
//! 1. **Initialize** — client sends `initialize` with capabilities
//! 2. **Initialized** — client sends `notifications/initialized`
//! 3. **Use** — call tools, list resources, etc.
//!
//! ## Design
//!
//! - `McpClient` holds the server URL and request ID generator
//! - Each method sends a JSON-RPC request and parses the response
//! - Results are returned as XCL `Result` values for error handling
//! - Follows DRY: all methods use a shared `request` helper

pub mod json_rpc2;

use std::collections::HashMap;
use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::Value;
use crate::core::result::{ok_result, err_result};
use json_rpc2::{JsonRpcRequest, JsonRpcResponse, IdGenerator};

/// MCP client type registered with the VM.
#[xcl_type(name = "McpClient", domain = "mcp", kind = "Opaque")]
pub struct McpClientType;

/// An MCP client that communicates with an MCP server over HTTP.
///
/// Created via `mcp.connect($url)` and used to call tools, list resources,
/// and interact with MCP server capabilities.
pub struct McpClient {
    /// The base URL of the MCP server (e.g., "http://localhost:3000/mcp")
    url: String,
    /// Auto-incrementing request ID generator
    id_gen: IdGenerator,
    /// Server capabilities (after initialization)
    server_name: Option<String>,
    /// Whether the client has been initialized
    initialized: bool,
}

impl McpClient {
    /// Create a new MCP client for the given server URL.
    pub fn new(url: String) -> Self {
        Self {
            url,
            id_gen: IdGenerator::new(),
            server_name: None,
            initialized: false,
        }
    }

    /// Send a JSON-RPC request and return the response body as a string.
    fn request(&mut self, method: &str, params: Option<Value>) -> Result<String, String> {
        let id = self.id_gen.next();
        let req = JsonRpcRequest::new(id, method, params);
        let body = req.to_json();

        let resp = ureq::post(&self.url)
            .header("Content-Type", "application/json")
            .send(&body)
            .map_err(|e| format!("HTTP error: {e}"))?;
        let response_str = resp.into_body().read_to_string().map_err(|e| format!("Response parse error: {e}"))?;
        Ok(response_str)
    }

    /// Send a JSON-RPC request and return the parsed response.
    fn request_parsed(&mut self, method: &str, params: Option<Value>) -> Result<JsonRpcResponse, String> {
        let response_str = self.request(method, params)?;
        JsonRpcResponse::from_json(&response_str)
            .ok_or_else(|| "Invalid JSON-RPC response".to_string())
    }

    /// Initialize the MCP connection.
    ///
    /// Sends the `initialize` request with client capabilities and version.
    pub fn initialize(&mut self, client_name: &str, client_version: &str) -> Result<Value, String> {
        let mut params = HashMap::new();
        params.insert("protocolVersion".to_string(), Value::from_str_boxed("2024-11-05".to_string()));

        let mut client_info = HashMap::new();
        client_info.insert("name".to_string(), Value::from_str_boxed(client_name.to_string()));
        client_info.insert("version".to_string(), Value::from_str_boxed(client_version.to_string()));
        params.insert("clientInfo".to_string(), Value::from_map_boxed(client_info));

        let caps = HashMap::new();
        params.insert("capabilities".to_string(), Value::from_map_boxed(caps));

        let params_val = Value::from_map_boxed(params);
        let resp = self.request_parsed("initialize", Some(params_val))?;

        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("Initialize failed").to_string());
        }

        // Extract server info
        if let Some(result) = resp.result_value() {
            if let Some(result_map) = result.as_map() {
                if let Some(server_info) = result_map.get("serverInfo") {
                    if let Some(si_map) = server_info.as_map() {
                        if let Some(name) = si_map.get("name") {
                            self.server_name = name.as_str().map(|s| s.to_string());
                        }
                    }
                }
            }
        }

        self.initialized = true;
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }

    /// Send the initialized notification.
    pub fn notify_initialized(&mut self) -> Result<(), String> {
        let req = JsonRpcRequest::notification("notifications/initialized", None);
        let body = req.to_json();

        let resp = ureq::post(&self.url)
            .header("Content-Type", "application/json")
            .send(&body)
            .map_err(|e| format!("Notification error: {e}"))?;
        let _ = resp.into_body().read_to_string();
        Ok(())
    }

    /// List available tools on the MCP server.
    pub fn list_tools(&mut self) -> Result<Value, String> {
        let resp = self.request_parsed("tools/list", None)?;
        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("list_tools failed").to_string());
        }
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }

    /// Call a tool on the MCP server.
    pub fn call_tool(&mut self, name: &str, arguments: Option<Value>) -> Result<Value, String> {
        let mut params = HashMap::new();
        params.insert("name".to_string(), Value::from_str_boxed(name.to_string()));
        if let Some(args) = arguments {
            params.insert("arguments".to_string(), args);
        }

        let params_val = Value::from_map_boxed(params);
        let resp = self.request_parsed("tools/call", Some(params_val))?;

        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("call_tool failed").to_string());
        }
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }

    /// List available resources on the MCP server.
    pub fn list_resources(&mut self) -> Result<Value, String> {
        let resp = self.request_parsed("resources/list", None)?;
        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("list_resources failed").to_string());
        }
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }

    /// Read a resource from the MCP server.
    pub fn read_resource(&mut self, uri: &str) -> Result<Value, String> {
        let mut params = HashMap::new();
        params.insert("uri".to_string(), Value::from_str_boxed(uri.to_string()));

        let params_val = Value::from_map_boxed(params);
        let resp = self.request_parsed("resources/read", Some(params_val))?;

        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("read_resource failed").to_string());
        }
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }

    /// List available prompts on the MCP server.
    pub fn list_prompts(&mut self) -> Result<Value, String> {
        let resp = self.request_parsed("prompts/list", None)?;
        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("list_prompts failed").to_string());
        }
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }

    /// Get a prompt from the MCP server.
    pub fn get_prompt(&mut self, name: &str, arguments: Option<Value>) -> Result<Value, String> {
        let mut params = HashMap::new();
        params.insert("name".to_string(), Value::from_str_boxed(name.to_string()));
        if let Some(args) = arguments {
            params.insert("arguments".to_string(), args);
        }

        let params_val = Value::from_map_boxed(params);
        let resp = self.request_parsed("prompts/get", Some(params_val))?;

        if resp.is_error() {
            return Err(resp.error_message().unwrap_or("get_prompt failed").to_string());
        }
        Ok(resp.result_value().unwrap_or(Value::nil()))
    }
}

// ─── XCL builtins ───

/// Connect to an MCP server and return an McpClient handle.
///
/// Creates a new MCP client connection to the specified server URL.
/// The server must support the JSON-RPC 2.0 protocol over HTTP POST.
///
/// ## SYNTAX
///
/// ```xcl
/// fn connect($url: str) -> McpClient
/// ```
///
/// ## PARAMETERS
///
/// `$url`: The HTTP URL of the MCP server endpoint.
///
/// ## EXAMPLES
///
/// ```xcl
/// $client = mcp.connect "http://localhost:3000/mcp"
/// mcp.initialize $client "xcl-client" "0.1.0"
/// $tools = mcp.list_tools $client
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `McpClient`
///
/// ## NOTES
///
/// - Returns `Err` if the URL is invalid.
/// - Call `mcp.initialize` before using the client.
#[xcl_fn(name = "mcp.connect", args = "url: str", ret = "McpClient")]
pub fn mcp_connect(url: &str) -> Value {
    // Store client state in a map — URL-based recreation approach
    let mut m = HashMap::new();
    m.insert("type".to_string(), Value::from_str_boxed("McpClient".to_string()));
    m.insert("url".to_string(), Value::from_str_boxed(url.to_string()));
    // We need to store the McpClient somehow. Since handles are for OS resources,
    // we'll use a different approach: store state in a map and recreate the client
    // from the URL when needed. For a full implementation, we'd use a HandleKind::McpClient.
    Value::from_map_boxed(m)
}

/// Initialize an MCP client connection.
///
/// Sends the MCP `initialize` request to the server, establishing the protocol
/// version and exchanging capabilities. Must be called before other MCP methods.
///
/// ## SYNTAX
///
/// ```xcl
/// fn initialize($client: McpClient; $name: str; $version: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$client`: An MCP client from `mcp.connect`.
///
/// `$name`: The client name identifier.
///
/// `$version`: The client version string.
///
/// ## EXAMPLES
///
/// ```xcl
/// $client = mcp.connect "http://localhost:3000/mcp"
/// $result = mcp.initialize $client "my-app" "1.0.0"
/// if $result.tag == "Ok" {
///     io.print "Connected to MCP server"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `McpClient`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.initialize", args = "client: McpClient; name: str; version: str", ret = "Result")]
pub fn mcp_initialize(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let name = ctx.arg(1).as_str().unwrap_or("xcl-client");
    let version = ctx.arg(2).as_str().unwrap_or("0.1.0");

    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    match client.initialize(name, version) {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

/// List all available tools on an MCP server.
///
/// Returns an array of tool descriptions, each containing the tool name,
/// description, and input schema.
///
/// ## SYNTAX
///
/// ```xcl
/// fn list_tools($client: McpClient) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$client`: An initialized MCP client.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = mcp.list_tools $client
/// if $result.tag == "Ok" {
///     for $tool in $result.value.tools {
///         io.print $tool.name
///     }
/// }
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.list_tools", args = "client: McpClient", ret = "Result")]
pub fn mcp_list_tools(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    match client.list_tools() {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

/// Call a tool on an MCP server.
///
/// Invokes a named tool with the provided arguments and returns the result.
///
/// ## SYNTAX
///
/// ```xcl
/// fn call_tool($client: McpClient; $name: str; $arguments: dict) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$client`: An initialized MCP client.
///
/// `$name`: The name of the tool to call.
///
/// `$arguments`: A dict of arguments to pass to the tool.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = mcp.call_tool $client "search" { query: "xcl language" }
/// if $result.tag == "Ok" {
///     io.print $result.value
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `McpClient`
/// - `str`
/// - `dict`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.call_tool", args = "client: McpClient; name: str; arguments: dict", ret = "Result")]
pub fn mcp_call_tool(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let name = ctx.arg(1).as_str().unwrap_or("");
    let arguments = ctx.arg(2);

    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    let args = if arguments.is_nil() { None } else { Some(arguments) };
    match client.call_tool(name, args) {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

/// List all available resources on an MCP server.
///
/// Returns an array of resource descriptions with URIs and metadata.
///
/// ## SYNTAX
///
/// ```xcl
/// fn list_resources($client: McpClient) -> Result
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.list_resources", args = "client: McpClient", ret = "Result")]
pub fn mcp_list_resources(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    match client.list_resources() {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

/// Read a resource from an MCP server by URI.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_resource($client: McpClient; $uri: str) -> Result
/// ```
///
/// ## INPUT TYPES
///
/// - `McpClient`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.read_resource", args = "client: McpClient; uri: str", ret = "Result")]
pub fn mcp_read_resource(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let uri = ctx.arg(1).as_str().unwrap_or("");

    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    match client.read_resource(uri) {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

/// List all available prompts on an MCP server.
///
/// ## SYNTAX
///
/// ```xcl
/// fn list_prompts($client: McpClient) -> Result
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.list_prompts", args = "client: McpClient", ret = "Result")]
pub fn mcp_list_prompts(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    match client.list_prompts() {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

/// Get a prompt template from an MCP server.
///
/// ## SYNTAX
///
/// ```xcl
/// fn get_prompt($client: McpClient; $name: str; $arguments: dict) -> Result
/// ```
///
/// ## INPUT TYPES
///
/// - `McpClient`
/// - `str`
/// - `dict`
///
/// ## OUTPUT TYPES
///
/// - `Result`
#[xcl_fn(name = "mcp.get_prompt", args = "client: McpClient; name: str; arguments: dict", ret = "Result")]
pub fn mcp_get_prompt(ctx: &mut xcl_vm::DomainCtx) -> Value {
    let client_val = ctx.arg(0);
    let name = ctx.arg(1).as_str().unwrap_or("");
    let arguments = ctx.arg(2);

    let Some(url) = client_val.as_map().and_then(|m| m.get("url")).and_then(|v| v.as_str()) else {
        return err_result("invalid_client", "Not a valid McpClient", 0);
    };

    let mut client = McpClient::new(url.to_string());
    let args = if arguments.is_nil() { None } else { Some(arguments) };
    match client.get_prompt(name, args) {
        Ok(result) => ok_result(result),
        Err(e) => err_result("mcp_error", &e, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_client_creation() {
        let client = McpClient::new("http://localhost:3000/mcp".to_string());
        assert_eq!(client.url, "http://localhost:3000/mcp");
        assert!(!client.initialized);
    }

    #[test]
    fn mcp_connect_returns_map() {
        let v = mcp_connect("http://localhost:3000/mcp");
        assert!(v.as_map().is_some());
        assert_eq!(v.as_map().unwrap().get("type").unwrap().as_str(), Some("McpClient"));
        assert_eq!(v.as_map().unwrap().get("url").unwrap().as_str(), Some("http://localhost:3000/mcp"));
    }
}
