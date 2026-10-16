//! Frontend module for the translator.
//!
//! A frontend turns target source code into IR. Each frontend implements
//! the `Frontend` trait and declares its language version.
//!
//! Per contract FE4.1: One frontend per (language, version).
//! Per contract FE4.5: A frontend is never called by a backend. They're peers.

pub mod java;
pub mod registry;

use crate::translate::ir::node::Ir;
use crate::translate::version::LangSpec;
use super::backend::TranslateError;

/// A frontend that parses source code into IR.
///
/// Per contract FE4.1: One frontend per (language, version).
pub trait Frontend {
    /// Return the language specification this frontend parses.
    fn lang_spec(&self) -> LangSpec;

    /// Parse source code into IR.
    ///
    /// Per contract FE4.2: A frontend must reject source it can't parse
    /// with a precise `ParseError`.
    /// Per contract FE4.3: A frontend must preserve semantics — no silent lowering.
    fn parse(&self, source: &str) -> Result<Vec<Ir>, TranslateError>;
}
