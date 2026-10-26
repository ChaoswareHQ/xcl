//! Context passed to `NativeKind::Ctx` functions, and the global-override guard.

use super::*;

/// Context passed to context-aware native functions ([`NativeKind::Ctx`]).
///
/// Provides access to the VM instance, arguments, memory arena, context stack, and defer queue.
pub struct DomainCtx<'a> {
    pub(crate) vm: &'a mut crate::runtime::vm::VM,
    pub(crate) args: &'a [Value],
}

impl<'a> DomainCtx<'a> {
    /// Returns the slice of arguments passed to the function.
    pub fn args(&self) -> &[Value] {
        self.args
    }

    /// Returns the number of arguments passed.
    pub fn argc(&self) -> usize {
        self.args.len()
    }

    /// Retrieves the argument at index `i`, or unit/nil if out of bounds.
    pub fn arg(&self, i: usize) -> Value {
        self.args.get(i).copied().unwrap_or(Value::unit())
    }

    /// Returns the type descriptors registered in the VM.
    pub fn type_entries(&self) -> &[&'static TypeDescriptor] {
        &self.vm.cold.type_entries
    }

    /// Returns a reference to the VM's bump allocation arena.
    pub fn arena(&self) -> &bumpalo::Bump {
        &self.vm.arena
    }

    /// Constructs a unit/nil value.
    pub fn unit(&self) -> Value {
        Value::unit()
    }

    /// Constructs a boolean value.
    pub fn boolean(&self, b: bool) -> Value {
        Value::bool_(b)
    }

    /// Allocates a string with static lifetime.
    pub fn string(&self, s: String) -> Value {
        Value::from_str_boxed(s)
    }

    /// Allocates an array with static lifetime.
    pub fn array(&self, v: Vec<Value>) -> Value {
        Value::from_array_boxed(v)
    }

    /// Constructs an integer value from `i64`.
    pub fn i64(&self, n: i64) -> Value {
        Value::from_i48(n)
    }

    /// Constructs an integer value from `u64`.
    pub fn u64(&self, n: u64) -> Value {
        Value::from_i48(n as i64)
    }

    /// Reentrantly calls an XCL closure or function value with the given arguments.
    pub fn call(&mut self, func: Value, args: &[Value]) -> Value {
        self.vm.call_value(func, args)
    }

    /// Pushes a deferred closure onto the current execution frame's defer stack.
    pub fn defer(&mut self, closure: Value) {
        self.vm.push_defer(closure);
    }

    /// Pushes a value onto the domain context stack for the given key.
    ///
    /// Enables nested domain blocks: `browser "chrome" { browser "firefox" { ... } }`.
    pub fn push_context(&mut self, key: &str, value: Value) {
        self.vm
            .cold.context_stack
            .entry(key.to_string())
            .or_insert_with(Vec::new)
            .push(value);
    }

    /// Retrieves the active context value for the given key (top of the stack).
    pub fn get_context(&self, key: &str) -> Option<Value> {
        self.vm
            .cold.context_stack
            .get(key)
            .and_then(|stack| stack.last().copied())
    }

    /// Pops the active context value for the given key.
    pub fn pop_context(&mut self, key: &str) -> Option<Value> {
        self.vm
            .cold.context_stack
            .get_mut(key)
            .and_then(|stack| stack.pop())
    }

    /// Checks if a context key currently has values on its stack.
    pub fn has_context(&self, key: &str) -> bool {
        self.vm
            .cold.context_stack
            .get(key)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Clears all context entries for the given key.
    pub fn clear_context(&mut self, key: &str) {
        self.vm.cold.context_stack.remove(key);
    }

    /// Returns a list of all currently active context keys.
    pub fn context_keys(&self) -> Vec<String> {
        self.vm
            .cold.context_stack
            .iter()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Creates a snapshot map of the top active value for each context key.
    pub fn context_snapshot(&self) -> crate::HashMap<String, Value> {
        let mut map = crate::HashMap::new();
        for (key, stack) in &self.vm.cold.context_stack {
            if let Some(top) = stack.last() {
                map.insert(key.clone(), *top);
            }
        }
        map
    }

    /// Temporarily override global function names with new implementations.
    /// Returns a guard that restores the originals when dropped.
    /// Used by cloud provider wrappers to swap in provider-specific functions.
    pub fn override_globals(
        &mut self,
        overrides: &[(&'static str, BuiltinImpl)],
    ) -> GlobalOverrideGuard<'_> {
        let saved = self.vm.save_and_override_globals(overrides);
        GlobalOverrideGuard { vm: self.vm, saved }
    }

    /// Override global function names manually (without a guard).
    /// Returns the saved state for later restoration via `restore_globals_manual`.
    /// Use this when you need to call other `ctx` methods between override and restore.
    pub fn override_globals_manual(
        &mut self,
        overrides: &[(&'static str, BuiltinImpl)],
    ) -> Vec<(Box<str>, Option<u32>)> {
        self.vm.save_and_override_globals(overrides)
    }

    /// Restore global function mappings from a saved state returned by `override_globals_manual`.
    pub fn restore_globals_manual(&mut self, saved: &[(Box<str>, Option<u32>)]) {
        self.vm.restore_globals(saved);
    }

    /// Scan registered native entries for entries matching the given impl type name,
    /// and override globals with their implementations. Returns the saved state for later restoration.
    ///
    /// Each `#[xcl_fn]` declares `impl = "CloudflareProvider"` to link to its provider type.
    /// The method looks up the impl descriptor in the VM's impl_index, gets its method names,
    /// and matches them against native entries with the same `impl_type`.
    pub fn override_by_trait(&mut self, impl_type_name: &str) -> Vec<(Box<str>, Option<u32>)> {
        // Find all impl descriptors for this type — collect trait names
        let impl_descs = self
            .vm
            .cold.impl_index
            .iter()
            .filter(|desc| desc.type_name == impl_type_name)
            .collect::<Vec<_>>();

        if impl_descs.is_empty() {
            return Vec::new();
        }

        // Collect method names from all impl descriptors for this type
        let method_names: Vec<&str> = impl_descs
            .iter()
            .flat_map(|desc| desc.methods.iter().map(|m| m.name))
            .collect();

        // Collect (owned_name, BuiltinImpl) pairs for matching entries
        let mut pairs: Vec<(String, BuiltinImpl)> = Vec::new();
        for entry in self.vm.cold.native_entries.iter() {
            let entry = *entry;
            if entry.impl_type == Some(impl_type_name) {
                // Check if this entry's override_method or method matches any method in the impl
                let method_match = method_names.contains(&entry.method)
                    || entry
                        .override_method
                        .map_or(false, |om| method_names.contains(&om));
                if method_match {
                    let impl_ = match entry.kind {
                        NativeKind::Pure => {
                            let f: BuiltinFn = unsafe { core::mem::transmute(entry.ptr.0) };
                            BuiltinImpl::Pure(f)
                        }
                        NativeKind::Ctx => {
                            let f: BuiltinCtxFn = unsafe { core::mem::transmute(entry.ptr.0) };
                            BuiltinImpl::Ctx(f)
                        }
                    };
                    // Use the full dotted name (domain.method) to match how register_natives
                    // registers functions in global_index, e.g., "bucket.list" not just "list"
                    let full_name = if let Some(domain) = entry.domain {
                        format!("{}.{}", domain, entry.method)
                    } else {
                        entry.method.to_string()
                    };
                    pairs.push((full_name, impl_));
                }
            }
        }
        // Convert to static refs for save_and_override_globals
        let mut overrides: Vec<(&'static str, BuiltinImpl)> = Vec::with_capacity(pairs.len());
        for (name, impl_) in pairs {
            overrides.push((name.leak(), impl_));
        }
        self.vm.save_and_override_globals(&overrides)
    }

    /// Restore global function mappings from a saved state returned by `override_by_trait`.
    pub fn restore_by_trait(&mut self, saved: &[(Box<str>, Option<u32>)]) {
        self.vm.restore_globals(saved);
    }
}

/// Guard that restores global function overrides when dropped.
pub struct GlobalOverrideGuard<'a> {
    vm: &'a mut crate::runtime::vm::VM,
    saved: Vec<(Box<str>, Option<u32>)>,
}

impl Drop for GlobalOverrideGuard<'_> {
    fn drop(&mut self) {
        self.vm.restore_globals(&self.saved);
    }
}

// — XclType trait for Rust/XCL record conversion —
