//! Backend module for the translator.
//!
//! A backend turns IR into target source code. Each backend implements
//! the `Backend` trait and declares its language version and supported features.
//!
//! Per contract BE5.1: One backend per (language, version).
//! Per contract BE5.2: Output must be pure target — compile-ready, idiomatic.
//! Per contract BE5.6: Output must not depend on any XCL runtime. Standalone.

pub mod registry;
pub mod java;
pub mod rust;
pub mod python;
pub mod c;

use crate::translate::ir::node::Ir;
use crate::translate::ir::ops::Feature;
use crate::translate::version::LangSpec;

/// Translation error types.
#[derive(Debug)]
pub enum TranslateError {
    /// Failed to parse source code.
    ParseError {
        lang: crate::translate::version::Language,
        message: String,
        span: crate::translate::ir::node::SourceSpan,
    },
    /// Target does not support a required feature.
    UnsupportedFeature {
        feature: Feature,
        target: LangSpec,
    },
    /// No type mapping for the target language.
    NoTypeMapping {
        from: crate::translate::ir::types::IrType,
        to: LangSpec,
    },
    /// No stdlib mapping for the target language.
    NoStdlibMapping {
        op: crate::translate::ir::ops::IrOp,
        to: LangSpec,
    },
    /// Unknown language version.
    UnknownVersion {
        lang: crate::translate::version::Language,
        version: String,
    },
    /// Round-trip verification failed.
    RoundtripFailed {
        report: String,
    },
}

impl core::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TranslateError::ParseError { lang, message, span } => {
                write!(f, "Parse error in {}: {} at {:?}", lang, message, span)
            }
            TranslateError::UnsupportedFeature { feature, target } => {
                write!(f, "Feature {} not supported by {}", feature, target)
            }
            TranslateError::NoTypeMapping { from, to } => {
                write!(f, "No type mapping from {} to {}", from, to)
            }
            TranslateError::NoStdlibMapping { op, to } => {
                write!(f, "No stdlib mapping for {} to {}", op, to)
            }
            TranslateError::UnknownVersion { lang, version } => {
                write!(f, "Unknown version {} for {}", version, lang)
            }
            TranslateError::RoundtripFailed { report } => {
                write!(f, "Round-trip failed: {}", report)
            }
        }
    }
}

/// A backend that converts IR to target source code.
///
/// Per contract BE5.1: One backend per (language, version).
pub trait Backend {
    /// Return the language specification this backend targets.
    fn lang_spec(&self) -> LangSpec;

    /// Check if this backend supports a given feature.
    ///
    /// Per contract BE5.3: A backend must declare `supports(Feature) -> bool`.
    fn supports(&self, feature: Feature) -> bool;

    /// Emit target source code from IR.
    ///
    /// Per contract BE5.2: Output must be pure target — compile-ready, idiomatic.
    /// Per contract BE5.8: Output must be deterministic. Same IR → same text.
    fn emit(&self, ir: &[Ir]) -> Result<String, TranslateError>;
}
