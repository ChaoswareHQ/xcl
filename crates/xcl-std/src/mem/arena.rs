//! Arena allocator for xcl-std.
//!
//! Provides bump allocation for fast, bulk memory allocation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use xcl_macros::xcl_fn;
use xcl_vm::Value;

use std::sync::LazyLock;

/// Global arena registry
static ARENAS: LazyLock<Mutex<HashMap<i64, Arc<Mutex<Vec<u8>>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static ARENA_COUNTER: LazyLock<Mutex<i64>> = LazyLock::new(|| Mutex::new(0));

/// Create a new arena allocator.
///
/// # Arguments
/// - `size` (int): Initial size in bytes
///
/// # Returns
/// Integer arena handle ID
///
/// # Example
/// ```xcl
/// arena = mem.arena_create(1024)
/// ```
#[xcl_fn("mem.arena_create", signature = "(size: int) -> int")]
pub fn arena_create(size: i64) -> Value {
    let mut arenas = ARENAS.lock().unwrap();
    let mut counter = ARENA_COUNTER.lock().unwrap();
    let id = *counter;
    *counter += 1;
    arenas.insert(id, Arc::new(Mutex::new(vec![0u8; size as usize])));
    Value::from_i48(id)
}

/// Allocate from an arena.
///
/// # Arguments
/// - `arena` (int): Arena handle ID
/// - `size` (int): Number of bytes to allocate
///
/// # Returns
/// Integer offset within the arena
///
/// # Example
/// ```xcl
/// offset = mem.arena_alloc(arena, 256)
/// ```
#[xcl_fn("mem.arena_alloc", signature = "(arena: int, size: int) -> int")]
pub fn arena_alloc(arena: i64, size: i64) -> Value {
    let arenas = ARENAS.lock().unwrap();
    if let Some(arena_data) = arenas.get(&arena) {
        let data = arena_data.lock().unwrap();
        Value::from_i48(data.len() as i64)
    } else {
        Value::from_i48(-1)
    }
}

/// Reset an arena, freeing all allocations.
///
/// # Arguments
/// - `arena` (int): Arena handle ID
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// mem.arena_reset(arena)
/// ```
#[xcl_fn("mem.arena_reset", signature = "(arena: int) -> result")]
pub fn arena_reset(arena: i64) -> Value {
    let arenas = ARENAS.lock().unwrap();
    if let Some(arena_data) = arenas.get(&arena) {
        let mut data = arena_data.lock().unwrap();
        *data = vec![0u8; data.len()];
        Value::from_i48(0)
    } else {
        Value::from_str_boxed("Arena not found".to_string())
    }
}

/// Destroy an arena.
///
/// # Arguments
/// - `arena` (int): Arena handle ID
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// mem.arena_destroy(arena)
/// ```
#[xcl_fn("mem.arena_destroy", signature = "(arena: int) -> result")]
pub fn arena_destroy(arena: i64) -> Value {
    let mut arenas = ARENAS.lock().unwrap();
    if arenas.remove(&arena).is_some() {
        Value::from_i48(0)
    } else {
        Value::from_str_boxed("Arena not found".to_string())
    }
}

/// Get arena usage statistics.
///
/// # Arguments
/// - `arena` (int): Arena handle ID
///
/// # Returns
/// Map containing arena statistics
///
/// # Example
/// ```xcl
/// mem.arena_stats(arena)
/// ```
#[xcl_fn("mem.arena_stats", signature = "(arena: int) -> map")]
pub fn arena_stats(arena: i64) -> Value {
    let mut info = HashMap::new();
    let arenas = ARENAS.lock().unwrap();
    if let Some(arena_data) = arenas.get(&arena) {
        let data = arena_data.lock().unwrap();
        info.insert("capacity".to_string(), Value::from_i48(data.len() as i64));
        info.insert("used".to_string(), Value::from_i48(0));
    }
    Value::from_map(info)
}
