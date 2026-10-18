//! Plugin system module.
//!
//! Provides functionality for loading and managing XCL plugins.
//!
//! Future: plugin.load, plugin.unload, plugin.list

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// List loaded plugins.
///
/// Currently returns an empty array; plugin loading not yet implemented.
#[xcl_fn]
pub fn plugin_list(ctx: &mut DomainCtx) -> Value {
    Value::from_array(Vec::new(), ctx.arena())
}
