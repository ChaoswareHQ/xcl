//! Plugin system for XCL — vendor-specific standalone extensions.
//!
//! Plugins live outside the standard library in `plugins/<vendor>/<name>/` directories,
//! each with a `plugin.toml` manifest that describes the domain, entry point, and
//! optional MCP bridge configuration.
//!
//! ## Architecture
//!
//! ```text
//!  plugin.toml (manifest)
//!       │
//!       ▼
//!  PluginManifest (parsed TOML)
//!       │
//!       ├──► Domain registration (e.g., "microsoft.vscode")
//!       │
//!       ├──► MCP bridge (if mcp_url is set)
//!       │      │
//!       │      ▼
//!       │   MCP server → tools/resources → XCL functions + types
//!       │
//!       └──► Entry point XCL file (auto-loaded)
//! ```
//!
//! ## Manifest Format (plugin.toml)
//!
//! ```toml
//! [package]
//! name = "vscode"
//! version = "0.1.0"
//!
//! [plugin]
//! domain = "microsoft.vscode"
//! entry = "src/manifest.xcl"
//! mcp_url = "http://localhost:3000/mcp"
//! mcp_auto_discover = true
//! description = "VS Code integration"
//! ```
//!
//! ## Design
//!
//! - Plugins are XCL-only — no Rust code needed per plugin
//! - The manifest is parsed at runtime, domains are registered dynamically
//! - If MCP is configured, tools and resources are auto-translated to XCL
//! - AI reads only the generated XCL headers for context, saving tokens

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, LazyLock};
use hashbrown::HashMap as FastHashMap;
use xcl_vm::{VM, Value, DomainSlot, BuiltinImpl};

/// Global registry for plugin state.
///
/// Because `BuiltinImpl` stores `fn` pointers (no captures), we store
/// all plugin data here and look it up at call time by domain + method name.
#[derive(Debug, Clone)]
pub struct PluginState {
    /// MCP server URL for this plugin
    pub mcp_url: Option<String>,
    /// Plugin metadata
    pub info: Value,
    /// Version string
    pub version: String,
    /// Description string
    pub description: String,
    /// MCP tool names discovered from the server
    pub tools: Vec<String>,
    /// MCP resource URIs discovered from the server
    pub resources: Vec<String>,
    /// MCP prompt names discovered from the server
    pub prompts: Vec<String>,
}

/// Global plugin registry — maps domain names to their state.
static PLUGIN_REGISTRY: LazyLock<Mutex<FastHashMap<String, PluginState>>> = LazyLock::new(|| Mutex::new(FastHashMap::new()));

/// Parsed plugin manifest from plugin.toml.
#[derive(Debug, Clone)]
pub struct PluginManifest {
    /// Package name (e.g., "vscode")
    pub name: String,
    /// Package version
    pub version: String,
    /// Minimum XCL version required
    pub xcl_version: String,
    /// The XCL domain name (e.g., "microsoft.vscode")
    pub domain: String,
    /// Entry point XCL file relative to plugin root
    pub entry: String,
    /// Optional MCP server URL for protocol bridging
    pub mcp_url: Option<String>,
    /// Whether to auto-discover MCP tools and generate XCL wrappers
    pub mcp_auto_discover: bool,
    /// Human-readable description
    pub description: String,
}

impl PluginManifest {
    /// Parse a plugin.toml file from the given path.
    ///
    /// Returns `None` if the file cannot be read or parsed.
    pub fn from_path(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        let doc: toml::Value = content.parse().ok()?;

        let name = doc.get("package")?.get("name")?.as_str()?.to_string();
        let version = doc.get("package")?.get("version")?.as_str()?.to_string();
        let xcl_version = doc.get("package")?.get("xcl-version")?.as_str()?.to_string();

        let plugin = doc.get("plugin")?;
        let domain = plugin.get("domain")?.as_str()?.to_string();
        let entry = plugin.get("entry")?.as_str()?.to_string();
        let mcp_url = plugin.get("mcp_url").and_then(|v| v.as_str()).map(|s| s.to_string());
        let mcp_auto_discover = plugin.get("mcp_auto_discover").and_then(|v| v.as_bool()).unwrap_or(false);
        let description = plugin.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();

        Some(Self {
            name,
            version,
            xcl_version,
            domain,
            entry,
            mcp_url,
            mcp_auto_discover,
            description,
        })
    }

    /// Resolve the entry point file path relative to the plugin root directory.
    pub fn entry_path(&self, plugin_root: &Path) -> PathBuf {
        plugin_root.join(&self.entry)
    }
}

/// A loaded plugin with its manifest and root directory.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// The plugin's root directory (e.g., plugins/microsoft/vscode/)
    pub root: PathBuf,
    /// Parsed manifest
    pub manifest: PluginManifest,
}

/// Discover all plugins under a given plugins directory.
///
/// Scans `plugins/<vendor>/<plugin>/plugin.toml` for all valid manifests.
pub fn discover_plugins(plugins_dir: &Path) -> Vec<Plugin> {
    let mut plugins = Vec::new();

    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return plugins;
    };

    for vendor_entry in entries.filter_map(|e| e.ok()) {
        let vendor_dir = vendor_entry.path();
        if !vendor_dir.is_dir() {
            continue;
        }

        let Ok(plugin_entries) = std::fs::read_dir(&vendor_dir) else {
            continue;
        };

        for plugin_entry in plugin_entries.filter_map(|e| e.ok()) {
            let plugin_dir = plugin_entry.path();
            if !plugin_dir.is_dir() {
                continue;
            }

            let manifest_path = plugin_dir.join("plugin.toml");
            if let Some(manifest) = PluginManifest::from_path(&manifest_path) {
                plugins.push(Plugin {
                    root: plugin_dir,
                    manifest,
                });
            }
        }
    }

    plugins
}

/// Register a plugin's domain in the VM.
///
/// Creates a new domain slot and registers the plugin's metadata as domain methods.
pub fn register_plugin(vm: &mut VM, plugin: &Plugin) {
    let domain = &plugin.manifest.domain;

    // Store plugin state in the global registry
    let state = PluginState {
        mcp_url: plugin.manifest.mcp_url.clone(),
        info: plugin_info_value(plugin),
        version: plugin.manifest.version.clone(),
        description: plugin.manifest.description.clone(),
        tools: Vec::new(),
        resources: Vec::new(),
        prompts: Vec::new(),
    };
    {
        let mut reg = PLUGIN_REGISTRY.lock().unwrap();
        reg.insert(domain.clone(), state);
    }

    // Register metadata methods using fn pointers that look up state from the registry
    register_domain_method(vm, domain, "info", plugin_info_fn);
    register_domain_method(vm, domain, "version", plugin_version_fn);
    register_domain_method(vm, domain, "description", plugin_description_fn);

    // If MCP is configured, bridge the MCP server to this domain
    if let Some(ref mcp_url) = plugin.manifest.mcp_url {
        if plugin.manifest.mcp_auto_discover {
            bridge_mcp_to_domain(vm, domain, mcp_url);
        }
    }

    // Load and evaluate the entry point XCL file
    let entry_path = plugin.manifest.entry_path(&plugin.root);
    if let Ok(source) = std::fs::read_to_string(&entry_path) {
        let _ = xcl_vm::evaluate(&source, vm);
    }
}

// ─── Fn pointer handlers (no captures — look up state from registry) ───

fn plugin_info_fn(_args: &[Value]) -> Value {
    // The domain is not directly available from args, so we return a placeholder
    // In practice, the domain is resolved by the VM before calling this function
    Value::nil()
}

fn plugin_version_fn(_args: &[Value]) -> Value {
    Value::nil()
}

fn plugin_description_fn(_args: &[Value]) -> Value {
    Value::nil()
}

/// Register a simple fn-pointer method as a domain constant accessor.
fn register_domain_method(vm: &mut VM, domain: &str, method: &str, fn_ptr: fn(&[Value]) -> Value) {
    let didx = *vm.domain_index.entry(domain.into()).or_insert_with(|| {
        let idx = vm.domain_table.len() as u32;
        vm.domain_table.push(DomainSlot {
            name: domain.into(),
            methods: Vec::new(),
            index: HashMap::new(),
        });
        idx
    });

    let slot = &mut vm.domain_table[didx as usize];
    let midx = slot.methods.len() as u32;
    slot.methods.push(Some(BuiltinImpl::Pure(fn_ptr)));
    slot.index.insert(method.into(), midx);
}

/// Build a Value containing plugin metadata.
fn plugin_info_value(plugin: &Plugin) -> Value {
    let mut map = HashMap::new();
    map.insert("name".to_string(), Value::from_str_boxed(plugin.manifest.name.clone()));
    map.insert("domain".to_string(), Value::from_str_boxed(plugin.manifest.domain.clone()));
    map.insert("version".to_string(), Value::from_str_boxed(plugin.manifest.version.clone()));
    map.insert("description".to_string(), Value::from_str_boxed(plugin.manifest.description.clone()));
    if let Some(ref mcp_url) = plugin.manifest.mcp_url {
        map.insert("mcp_url".to_string(), Value::from_str_boxed(mcp_url.clone()));
    }
    Value::from_map_boxed(map)
}

/// Load and register all plugins found under the given directory.
///
/// This is called by `new_vm()` to auto-load all available plugins.
pub fn load_all_plugins(vm: &mut VM, plugins_dir: &Path) {
    let plugins = discover_plugins(plugins_dir);
    for plugin in plugins {
        register_plugin(vm, &plugin);
    }
}

/// Bridge an MCP server to an XCL domain.
///
/// Connects to the MCP server, discovers tools and resources,
/// and registers them as domain methods in the VM.
fn bridge_mcp_to_domain(vm: &mut VM, domain: &str, mcp_url: &str) {
    use crate::protocol::mcp::McpClient;

    let mut client = McpClient::new(mcp_url.to_string());

    // Initialize the MCP connection
    if let Ok(_init_result) = client.initialize("xcl-plugin", "0.1.0") {
        let _ = client.notify_initialized();

        // Discover tools
        if let Ok(tools_result) = client.list_tools() {
            if let Some(tools_map) = tools_result.as_map() {
                if let Some(tools_arr) = tools_map.get("tools").and_then(|v| v.as_array()) {
                    for tool_val in tools_arr {
                        if let Some(tool_map) = tool_val.as_map() {
                            let tool_name = match tool_map.get("name").and_then(|v| v.as_str()) {
                                Some(name) => name.to_string(),
                                None => continue,
                            };

                            // Store tool name in registry
                            {
                                if let Ok(mut reg) = PLUGIN_REGISTRY.lock() {
                                    if let Some(state) = reg.get_mut(domain) {
                                        state.tools.push(tool_name.clone());
                                    }
                                }
                            }

                            // Register the tool wrapper
                            register_domain_method(vm, domain, &tool_name, mcp_tool_call_fn);
                        }
                    }
                }
            }
        }

        // Discover resources
        if let Ok(resources_result) = client.list_resources() {
            if let Some(resources_map) = resources_result.as_map() {
                if let Some(resources_arr) = resources_map.get("resources").and_then(|v| v.as_array()) {
                    for resource_val in resources_arr {
                        if let Some(resource_map) = resource_val.as_map() {
                            let resource_uri = match resource_map.get("uri").and_then(|v| v.as_str()) {
                                Some(uri) => uri.to_string(),
                                None => continue,
                            };

                            let resource_name = resource_uri
                                .split('/')
                                .last()
                                .unwrap_or(&resource_uri)
                                .to_string();

                            // Store resource in registry
                            {
                                if let Ok(mut reg) = PLUGIN_REGISTRY.lock() {
                                    if let Some(state) = reg.get_mut(domain) {
                                        state.resources.push(resource_uri.clone());
                                    }
                                }
                            }

                            // Register resource reader
                            register_domain_method(vm, domain, &resource_name, mcp_resource_read_fn);
                        }
                    }
                }
            }
        }

        // Discover prompts
        if let Ok(prompts_result) = client.list_prompts() {
            if let Some(prompts_map) = prompts_result.as_map() {
                if let Some(prompts_arr) = prompts_map.get("prompts").and_then(|v| v.as_array()) {
                    for prompt_val in prompts_arr {
                        if let Some(prompt_map) = prompt_val.as_map() {
                            let prompt_name = match prompt_map.get("name").and_then(|v| v.as_str()) {
                                Some(name) => name.to_string(),
                                None => continue,
                            };

                            // Store prompt in registry
                            {
                                if let Ok(mut reg) = PLUGIN_REGISTRY.lock() {
                                    if let Some(state) = reg.get_mut(domain) {
                                        state.prompts.push(prompt_name.clone());
                                    }
                                }
                            }

                            // Register prompt getter
                            register_domain_method(vm, domain, &prompt_name, mcp_prompt_get_fn);
                        }
                    }
                }
            }
        }
    }

    // Register list_tools method
    register_domain_method(vm, domain, "list_tools", mcp_list_tools_fn);
}

// ─── MCP fn pointer handlers ───

fn mcp_tool_call_fn(args: &[Value]) -> Value {
    // Collect all arguments as a dict
    let mut params = HashMap::new();
    for (i, arg) in args.iter().enumerate() {
        params.insert(format!("arg_{}", i), *arg);
    }
    let args_val = if params.is_empty() {
        Value::nil()
    } else {
        Value::from_map_boxed(params)
    };

    // Look up the MCP URL from the registry
    let reg = PLUGIN_REGISTRY.lock().unwrap();
    for state in reg.values() {
        if let Some(url) = &state.mcp_url {
            return call_mcp_tool(url, "unknown", args_val);
        }
    }
    err_result("no_mcp_url", "No MCP URL configured")
}

fn call_mcp_tool(mcp_url: &str, tool_name: &str, args: Value) -> Value {
    use crate::protocol::mcp::McpClient;
    let mut client = McpClient::new(mcp_url.to_string());
    match client.call_tool(tool_name, Some(args)) {
        Ok(result) => result,
        Err(e) => err_result("mcp_error", &e),
    }
}

fn mcp_resource_read_fn(_args: &[Value]) -> Value {
    err_result("not_implemented", "Resource read requires domain context")
}

fn mcp_prompt_get_fn(_args: &[Value]) -> Value {
    err_result("not_implemented", "Prompt get requires domain context")
}

fn mcp_list_tools_fn(_args: &[Value]) -> Value {
    let mut tools = Vec::new();
    if let Ok(reg) = PLUGIN_REGISTRY.lock() {
        for state in reg.values() {
            for tool in &state.tools {
                tools.push(Value::from_str_boxed(tool.clone()));
            }
        }
    }
    Value::from_array_boxed(tools)
}

fn err_result(kind: &str, message: &str) -> Value {
    let mut err = HashMap::new();
    err.insert("tag".to_string(), Value::from_str_boxed("Err".to_string()));
    err.insert("kind".to_string(), Value::from_str_boxed(kind.to_string()));
    err.insert("message".to_string(), Value::from_str_boxed(message.to_string()));
    Value::from_map_boxed(err)
}
