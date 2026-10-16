//! XCL Translator — polyglot transpiler module.
//!
//! The translator converts XCL source code to other programming languages
//! (Java, Rust, Python, C, C++) and vice versa through a language-agnostic IR.
//!
//! # Architecture
//!
//! ```text
//! XCL source → Frontend → IR → Backend → Target source
//! Target source → Frontend → IR → Backend → XCL source
//! ```
//!
//! # Supported Languages
//!
//! - **Java**: 25
//! - **Rust**: 2021, 2024
//! - **Python**: 3.11, 3.12, 3.13
//! - **C**: 99, 11, 17, 23 (with compiler detection)
//! - **C++**: 11, 14, 17, 20, 23
//!
//! # Contracts
//!
//! - Every language/version is a `(Frontend, Backend)` pair registered in a registry.
//! - IR is the only thing they share.
//! - Pure output is the only acceptable output.
//! - Round-trip is the only proof of correctness.

pub mod version;
pub mod ir;
pub mod maps;
pub mod backend;
pub mod frontend;
pub mod emit;
pub mod emitter;
pub mod roundtrip;
pub mod verify;
pub mod compiler;
pub mod dependency;
pub mod xcl;
pub mod utils;
pub mod project;

pub use version::{LangSpec, Language, Version};
pub use backend::{TranslateError, Backend};
pub use frontend::Frontend;
pub use roundtrip::RoundtripReport;

/// Register all frontends.
pub fn register_all_frontends(reg: &mut FrontendRegistry) {
    reg.add_frontend(XclFrontend::new());
    frontend::registry::register_all(reg);
}

use backend::registry::Registry as BackendRegistry;
use frontend::registry::Registry as FrontendRegistry;
use ir::node::{Ir, Origin};

/// XCL frontend that lowers XCL source to IR.
struct XclFrontend;

impl XclFrontend {
    fn new() -> Self {
        Self
    }
}

impl Frontend for XclFrontend {
    fn lang_spec(&self) -> LangSpec {
        LangSpec::xcl("1")
    }

    fn parse(&self, source: &str) -> Result<Vec<Ir>, TranslateError> {
        let origin = Origin::new(LangSpec::xcl("1"));
        let ir_nodes = ir::lower::lower_xcl(source, origin);
        Ok(ir_nodes)
    }
}

/// The main translator struct.
///
/// Converts source code between languages using registered frontends and backends.
pub struct Translator {
    backends: BackendRegistry,
    frontends: FrontendRegistry,
}

impl Translator {
    /// Create a new translator with all backends and frontends registered.
    pub fn new() -> Self {
        let mut backends = BackendRegistry::new();
        let mut frontends = FrontendRegistry::new();
        backend::registry::register_all(&mut backends);
        register_all_frontends(&mut frontends);
        Self { backends, frontends }
    }

    /// Translate source code from one language to another.
    ///
    /// # Arguments
    /// - `source` — Source code string
    /// - `from` — Source language specification
    /// - `to` — Target language specification
    ///
    /// # Returns
    /// Translated source code or a `TranslateError`
    pub fn translate(
        &self,
        source: &str,
        from: LangSpec,
        to: LangSpec,
    ) -> Result<String, TranslateError> {
        // Step 1: Parse source to IR using frontend
        let ir_nodes = if from.language == Language::Xcl {
            // Use built-in XCL lowerer
            let origin = Origin::new(from.clone());
            ir::lower::lower_xcl(source, origin)
        } else {
            // Try registered frontend first
            if let Some(frontend) = self.frontends.get(&from) {
                frontend.parse(source)?
            } else {
                // No frontend registered for this language — fall back to XCL lowerer
                // so that simple code still produces something. In the future, each
                // language will have its own frontend.
                let origin = Origin::new(from.clone());
                ir::lower::lower_xcl(source, origin)
            }
        };

        // Step 2: Emit IR to target using backend
        if to.language == Language::Xcl {
            // Raise IR back to XCL
            Ok(ir::raise::raise_to_xcl(&ir_nodes))
        } else {
            let backend = self.backends.get(&to)
                .ok_or_else(|| TranslateError::UnknownVersion {
                    lang: to.language,
                    version: to.version.0.clone(),
                })?;
            backend.emit(&ir_nodes)
        }
    }

    /// Perform a round-trip translation: XCL → target → XCL.
    ///
    /// # Arguments
    /// - `source` — Original XCL source code
    /// - `from` — Source language specification
    /// - `to` — Target language specification
    ///
    /// # Returns
    /// Round-trip report with target code and round-tripped source
    pub fn roundtrip(
        &self,
        source: &str,
        _from: LangSpec,
        to: LangSpec,
    ) -> Result<RoundtripReport, TranslateError> {
        roundtrip::roundtrip_xcl_to_target(source, &to)
    }

    /// Get all supported target language specifications.
    pub fn supported_targets(&self) -> Vec<LangSpec> {
        self.backends.specs()
    }
}

impl Default for Translator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translator_new() {
        let translator = Translator::new();
        let targets = translator.supported_targets();
        assert!(!targets.is_empty(), "Should have registered backends");
    }

    #[test]
    fn test_translate_literal() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::java("25"),
        );
        assert!(result.is_ok(), "Should translate literal to Java");
    }

    #[test]
    fn test_translate_to_rust() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::rust("2024"),
        );
        assert!(result.is_ok(), "Should translate literal to Rust");
    }

    #[test]
    fn test_translate_to_python() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::python("313"),
        );
        assert!(result.is_ok(), "Should translate literal to Python");
    }

    #[test]
    fn test_roundtrip() {
        let translator = Translator::new();
        let result = translator.roundtrip(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::java("25"),
        );
        assert!(result.is_ok(), "Round-trip should succeed");
    }

    #[test]
    fn test_translate_to_c99() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::c("99"),
        );
        assert!(result.is_ok(), "Should translate to C99");
        let output = result.unwrap();
        assert!(output.contains("#include"), "C99 output should have includes");
    }

    #[test]
    fn test_translate_to_c23() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::c("23"),
        );
        assert!(result.is_ok(), "Should translate to C23");
    }

    #[test]
    fn test_translate_to_c11() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::c("11"),
        );
        assert!(result.is_ok(), "Should translate to C11");
    }

    #[test]
    fn test_translate_to_c17() {
        let translator = Translator::new();
        let result = translator.translate(
            "let x = 42",
            LangSpec::xcl("1"),
            LangSpec::c("17"),
        );
        assert!(result.is_ok(), "Should translate to C17");
    }

    #[test]
    fn test_c_backends_registered() {
        let translator = Translator::new();
        let targets = translator.supported_targets();
        
        let c_versions: Vec<&LangSpec> = targets.iter()
            .filter(|t| t.language == Language::C)
            .collect();
        
        assert_eq!(c_versions.len(), 4, "Should have 4 C backends (99, 11, 17, 23)");
    }
}
