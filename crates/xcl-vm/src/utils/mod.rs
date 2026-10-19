//! Supporting infrastructure: argument parsing, native function registry, and well-known types.

pub mod args;
pub mod native;
pub mod wellknown;

pub use args::split_named_args;
pub use native::{IMPL_REGISTRY, NATIVE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY};
pub use wellknown::WellKnown;
