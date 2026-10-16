//! Atomic operations for xcl-plugins.
//!
//! Provides atomic integer operations for lock-free programming.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::LazyLock;
use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

static ATOMIC_STORE: LazyLock<HashMap<i64, AtomicI64>> = LazyLock::new(HashMap::new);

/// Create a new atomic variable.
///
/// # Arguments
/// - `initial` (int): Initial value
///
/// # Returns
/// Integer atomic handle ID
///
/// # Example
/// ```xcl
/// atomic = cpu.atomic_create(0)
/// ```
#[xcl_fn("cpu.atomic_create", signature = "(initial: int) -> int")]
pub fn atomic_create(initial: i64) -> Value {
    let id = std::process::id() as i64 ^ initial;
    Value::from_i48(id)
}

/// Load an atomic value.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `order` (int): Memory ordering (0=Relaxed, 1=Acquire, 2=Release, 3=AcqRel, 4=SeqCst)
///
/// # Returns
/// Integer atomic value
///
/// # Example
/// ```xcl
/// cpu.atomic_load(ptr, 4)
/// ```
#[xcl_fn("cpu.atomic_load", signature = "(ptr: int, order: int) -> int")]
pub fn atomic_load(ptr: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let ordering = ordering_from_int(order);
    let val = unsafe { &(ptr as *const AtomicI64) }.load(ordering);
    Value::from_i48(val)
}

/// Store a value to an atomic variable.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `value` (int): Value to store
/// - `order` (int): Memory ordering
///
/// # Example
/// ```xcl
/// cpu.atomic_store(ptr, 42, 4)
/// ```
#[xcl_fn("cpu.atomic_store", signature = "(ptr: int, value: int, order: int) -> nil")]
pub fn atomic_store(ptr: i64, value: i64, order: i64) -> Value {
    if ptr != 0 {
        let ordering = ordering_from_int(order);
        unsafe { &(ptr as *const AtomicI64) }.store(value, ordering);
    }
    Value::nil()
}

/// Atomic compare-and-swap.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `expected` (int): Expected current value
/// - `new` (int): New value to set
/// - `order` (int): Memory ordering
///
/// # Returns
/// Integer previous value (equals expected on success)
///
/// # Example
/// ```xcl
/// cpu.atomic_cas(ptr, 0, 1, 4)
/// ```
#[xcl_fn("cpu.atomic_cas", signature = "(ptr: int, expected: int, new: int, order: int) -> int")]
pub fn atomic_cas(ptr: i64, expected: i64, new: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(expected);
    }
    let ordering = ordering_from_int(order);
    let prev = unsafe { &(ptr as *const AtomicI64) }.compare_exchange(expected, new, ordering, ordering).unwrap_or(expected);
    Value::from_i48(prev)
}

/// Atomic fetch-and-add.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `delta` (int): Value to add
/// - `order` (int): Memory ordering
///
/// # Returns
/// Integer previous value
///
/// # Example
/// ```xcl
/// cpu.atomic_add(ptr, 1, 4)
/// ```
#[xcl_fn("cpu.atomic_add", signature = "(ptr: int, delta: int, order: int) -> int")]
pub fn atomic_add(ptr: i64, delta: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let ordering = ordering_from_int(order);
    let prev = unsafe { &(ptr as *const AtomicI64) }.fetch_add(delta, ordering);
    Value::from_i48(prev)
}

/// Atomic fetch-and-subtract.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `delta` (int): Value to subtract
/// - `order` (int): Memory ordering
///
/// # Returns
/// Integer previous value
///
/// # Example
/// ```xcl
/// cpu.atomic_sub(ptr, 1, 4)
/// ```
#[xcl_fn("cpu.atomic_sub", signature = "(ptr: int, delta: int, order: int) -> int")]
pub fn atomic_sub(ptr: i64, delta: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let ordering = ordering_from_int(order);
    let prev = unsafe { &(ptr as *const AtomicI64) }.fetch_sub(delta, ordering);
    Value::from_i48(prev)
}

/// Atomic fetch-and-AND.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `mask` (int): Bitmask to AND
/// - `order` (int): Memory ordering
///
/// # Returns
/// Integer previous value
///
/// # Example
/// ```xcl
/// cpu.atomic_and(ptr, 0xFF, 4)
/// ```
#[xcl_fn("cpu.atomic_and", signature = "(ptr: int, mask: int, order: int) -> int")]
pub fn atomic_and(ptr: i64, mask: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let ordering = ordering_from_int(order);
    let prev = unsafe { &(ptr as *const AtomicI64) }.fetch_and(mask, ordering);
    Value::from_i48(prev)
}

/// Atomic fetch-and-OR.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `mask` (int): Bitmask to OR
/// - `order` (int): Memory ordering
///
/// # Returns
/// Integer previous value
///
/// # Example
/// ```xcl
/// cpu.atomic_or(ptr, 0xFF, 4)
/// ```
#[xcl_fn("cpu.atomic_or", signature = "(ptr: int, mask: int, order: int) -> int")]
pub fn atomic_or(ptr: i64, mask: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let ordering = ordering_from_int(order);
    let prev = unsafe { &(ptr as *const AtomicI64) }.fetch_or(mask, ordering);
    Value::from_i48(prev)
}

/// Atomic fetch-and-XOR.
///
/// # Arguments
/// - `ptr` (int): Memory address of atomic
/// - `mask` (int): Bitmask to XOR
/// - `order` (int): Memory ordering
///
/// # Returns
/// Integer previous value
///
/// # Example
/// ```xcl
/// cpu.atomic_xor(ptr, 0xFF, 4)
/// ```
#[xcl_fn("cpu.atomic_xor", signature = "(ptr: int, mask: int, order: int) -> int")]
pub fn atomic_xor(ptr: i64, mask: i64, order: i64) -> Value {
    if ptr == 0 {
        return Value::from_i48(0);
    }
    let ordering = ordering_from_int(order);
    let prev = unsafe { &(ptr as *const AtomicI64) }.fetch_xor(mask, ordering);
    Value::from_i48(prev)
}

/// Get memory ordering name.
///
/// # Arguments
/// - `order` (int): Ordering number
///
/// # Returns
/// String ordering name
///
/// # Example
/// ```xcl
/// cpu.ordering_name(4)  // Returns "SeqCst"
/// ```
#[xcl_fn("cpu.ordering_name", signature = "(order: int) -> str")]
pub fn ordering_name(order: i64) -> Value {
    let name = match order {
        0 => "Relaxed",
        1 => "Acquire",
        2 => "Release",
        3 => "AcqRel",
        4 => "SeqCst",
        _ => "Unknown",
    };
    Value::from_str_boxed(name.to_string())
}

fn ordering_from_int(order: i64) -> Ordering {
    match order {
        0 => Ordering::Relaxed,
        1 => Ordering::Acquire,
        2 => Ordering::Release,
        3 => Ordering::AcqRel,
        4 => Ordering::SeqCst,
        _ => Ordering::SeqCst,
    }
}
