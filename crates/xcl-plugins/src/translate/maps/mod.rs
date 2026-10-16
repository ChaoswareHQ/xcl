//! Mapping modules for the translator.
//!
//! Provides type and standard library mappings between IR and target languages.

pub mod types_map;
pub mod stdlib_map;
pub mod feature_map;

pub use types_map::*;
pub use stdlib_map::*;
pub use feature_map::*;
