//! Backend registry for the translator.
//!
//! Backends register themselves through this registry. The translator
//! looks up the appropriate backend by LangSpec.
//!
//! Per spec: The AI must never hardcode a language check. Everything goes through the registry.

use std::collections::HashMap;
use crate::translate::version::LangSpec;
use super::Backend;

/// Registry of backends indexed by language specification.
pub struct Registry {
    backends: HashMap<LangSpec, Box<dyn Backend>>,
}

impl Registry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
        }
    }

    /// Register a backend.
    pub fn add_backend<B: Backend + 'static>(&mut self, backend: B) {
        let spec = backend.lang_spec();
        self.backends.insert(spec, Box::new(backend));
    }

    /// Look up a backend by language specification.
    pub fn get(&self, spec: &LangSpec) -> Option<&dyn Backend> {
        self.backends.get(spec).map(|b| b.as_ref())
    }

    /// Check if a backend exists for the given specification.
    pub fn has(&self, spec: &LangSpec) -> bool {
        self.backends.contains_key(spec)
    }

    /// Get all registered language specifications.
    pub fn specs(&self) -> Vec<LangSpec> {
        self.backends.keys().cloned().collect()
    }
}

/// Register all backends.
pub fn register_all(reg: &mut Registry) {
    // Java backends (all LTS versions)
    reg.add_backend(super::java::v8::Java8::new());
    reg.add_backend(super::java::v11::Java11::new());
    reg.add_backend(super::java::v17::Java17::new());
    reg.add_backend(super::java::v21::Java21::new());
    reg.add_backend(super::java::v25::Java25::new());

    // Rust backends
    reg.add_backend(super::rust::v2024::Rust2024::new());

    // Python backends
    reg.add_backend(super::python::v313::Python313::new());

    // C backends (all versions)
    reg.add_backend(super::c::v99::C99::new());
    reg.add_backend(super::c::v11::C11::new());
    reg.add_backend(super::c::v17::C17::new());
    reg.add_backend(super::c::v23::C23::new());
}
