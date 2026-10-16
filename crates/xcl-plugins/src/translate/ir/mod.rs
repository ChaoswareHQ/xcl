//! Intermediate Representation (IR) for the translator.
//!
//! The IR is the pivot of the translator — all frontends produce IR,
//! and all backends consume IR. This ensures language-agnostic transformation.
//!
//! # Contracts
//!
//! - **IR3.1**: IR is language-agnostic. No Java/Rust/Python names.
//! - **IR3.2**: IR is lossless enough to reconstruct XCL. Every node carries a SourceSpan.
//! - **IR3.3**: IR is version-agnostic. Version checks happen in backends, not in IR.
//! - **IR3.4**: IR is typed. No untyped expressions.
//! - **IR3.5**: IR carries origin metadata.
//! - **IR3.6**: IR carries hints: Record, Sealed, Defer, Optional.

pub mod node;
pub mod types;
pub mod ops;
pub mod lower;
pub mod raise;

pub use node::*;
pub use types::*;
pub use ops::*;
pub use lower::*;
pub use raise::*;
