//! Supporting infrastructure: argument parsing and native function registry.

pub mod args;
pub mod native;

pub use args::split_named_args;
pub use native::{IMPL_REGISTRY, NATIVE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY};
