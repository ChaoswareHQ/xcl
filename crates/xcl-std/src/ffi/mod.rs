//! Foreign Function Interface (FFI) for xcl-std.
//!
//! Provides C FFI types, callbacks, dynamic library loading, and generated bindings.

pub mod c;
pub mod callbacks;
pub mod libloading;
pub mod bindings;
