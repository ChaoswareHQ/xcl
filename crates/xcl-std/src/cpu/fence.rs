//! Memory fence operations for xcl-std.
//!
//! Provides memory barriers for controlling instruction ordering.

use std::sync::atomic::Ordering;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Insert a full memory fence.
///
/// Prevents reordering of memory operations across this point.
///
/// # Example
/// ```xcl
/// cpu.fence()
/// ```
#[xcl_fn("cpu.fence", signature = "() -> nil")]
pub fn fence() -> Value {
    std::sync::atomic::fence(Ordering::SeqCst);
    Value::nil()
}

/// Insert an acquire fence.
///
/// Prevents subsequent memory operations from being reordered before this fence.
///
/// # Example
/// ```xcl
/// cpu.fence_acquire()
/// ```
#[xcl_fn("cpu.fence_acquire", signature = "() -> nil")]
pub fn fence_acquire() -> Value {
    std::sync::atomic::fence(Ordering::Acquire);
    Value::nil()
}

/// Insert a release fence.
///
/// Prevents preceding memory operations from being reordered after this fence.
///
/// # Example
/// ```xcl
/// cpu.fence_release()
/// ```
#[xcl_fn("cpu.fence_release", signature = "() -> nil")]
pub fn fence_release() -> Value {
    std::sync::atomic::fence(Ordering::Release);
    Value::nil()
}

/// Insert a relaxed fence.
///
/// No ordering guarantees, but may be useful for some architectures.
///
/// # Example
/// ```xcl
/// cpu.fence_relaxed()
/// ```
#[xcl_fn("cpu.fence_relaxed", signature = "() -> nil")]
pub fn fence_relaxed() -> Value {
    std::sync::atomic::fence(Ordering::Relaxed);
    Value::nil()
}

/// Insert an acquire-release fence.
///
/// Combines acquire and release semantics.
///
/// # Example
/// ```xcl
/// cpu.fence_acq_rel()
/// ```
#[xcl_fn("cpu.fence_acq_rel", signature = "() -> nil")]
pub fn fence_acq_rel() -> Value {
    std::sync::atomic::fence(Ordering::AcqRel);
    Value::nil()
}

/// Spin until a condition is met (compiler barrier).
///
/// Hints to the compiler and CPU that this is a spin loop.
///
/// # Example
/// ```xcl
/// cpu.spin_loop_hint()
/// ```
#[xcl_fn("cpu.spin_loop_hint", signature = "() -> nil")]
pub fn spin_loop_hint() -> Value {
    std::sync::atomic::spin_loop_hint();
    Value::nil()
}

/// Single thread hint.
///
/// Hints to the CPU that only one thread is executing.
///
/// # Example
/// ```xcl
/// cpu.single_thread_hint()
/// ```
#[xcl_fn("cpu.single_thread_hint", signature = "() -> nil")]
pub fn single_thread_hint() -> Value {
    std::sync::atomic::singlethread();
    Value::nil()
}
