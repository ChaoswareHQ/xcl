//! CPU-level primitives for xcl-plugins.
//!
//! Provides atomics, memory fences, SIMD, inline assembly, and cache info.

pub mod atomic;
pub mod fence;
pub mod simd;
pub mod asm;
pub mod cache;
