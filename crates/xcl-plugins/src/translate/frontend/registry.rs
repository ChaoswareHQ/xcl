//! Frontend registry for the translator.
//!
//! Frontends register themselves through this registry.

use std::collections::HashMap;
use crate::translate::version::LangSpec;
use super::Frontend;

/// Registry of frontends indexed by language specification.
pub struct Registry {
    frontends: HashMap<LangSpec, Box<dyn Frontend>>,
}

impl Registry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            frontends: HashMap::new(),
        }
    }

    /// Register a frontend.
    pub fn add_frontend<F: Frontend + 'static>(&mut self, frontend: F) {
        let spec = frontend.lang_spec();
        self.frontends.insert(spec, Box::new(frontend));
    }

    /// Look up a frontend by language specification.
    pub fn get(&self, spec: &LangSpec) -> Option<&dyn Frontend> {
        self.frontends.get(spec).map(|f| f.as_ref())
    }

    /// Check if a frontend exists for the given specification.
    pub fn has(&self, spec: &LangSpec) -> bool {
        self.frontends.contains_key(spec)
    }
}

/// Register all frontends.
pub fn register_all(reg: &mut Registry) {
    // XCL frontend is registered in translate/mod.rs
    reg.add_frontend(super::java::JavaFrontend::new("8"));
    reg.add_frontend(super::java::JavaFrontend::new("11"));
    reg.add_frontend(super::java::JavaFrontend::new("17"));
    reg.add_frontend(super::java::JavaFrontend::new("21"));
    reg.add_frontend(super::java::JavaFrontend::new("25"));
}
