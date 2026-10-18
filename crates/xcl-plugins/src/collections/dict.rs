//! Dict (HashMap) operations domain.
//!
//! Inspired by Raku Hash methods and Perl hash utilities.
//! All functions are registered under the `dict.*` domain prefix.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

// ─── Helper ───

fn get_map(v: Value) -> HashMap<String, Value> {
    v.as_map().cloned().unwrap_or_default()
}

// ═══════════════════════════════════════════════════════════
//  Access
// ═══════════════════════════════════════════════════════════

/// Get all keys as an array.
#[xcl_fn]
fn dict_keys(ctx: &mut DomainCtx) -> Value {
    let map = get_map(ctx.arg(0));
    let keys: Vec<Value> = map.keys()
        .map(|k| Value::from_str(k.clone(), ctx.arena()))
        .collect();
    Value::from_array(keys, ctx.arena())
}

/// Get all values as an array.
#[xcl_fn]
fn dict_values(ctx: &mut DomainCtx) -> Value {
    let map = get_map(ctx.arg(0));
    let values: Vec<Value> = map.values().copied().collect();
    Value::from_array(values, ctx.arena())
}

/// Get all entries as an array of [key, value] pairs.
#[xcl_fn]
fn dict_entries(ctx: &mut DomainCtx) -> Value {
    let map = get_map(ctx.arg(0));
    let mut entries = Vec::with_capacity(map.len());
    for (k, v) in map {
        let pair = vec![Value::from_str(k, ctx.arena()), v];
        entries.push(Value::from_array(pair, ctx.arena()));
    }
    Value::from_array(entries, ctx.arena())
}

// ═══════════════════════════════════════════════════════════
//  Lookup
// ═══════════════════════════════════════════════════════════

/// Get value by key, or nil if not found.
#[xcl_fn]
fn dict_get(args: &[Value]) -> Value {
    let map = get_map(args[0]);
    let key = args[1].as_string().to_string();
    map.get(&key).copied().unwrap_or(Value::nil())
}

/// Check if dict has a key.
#[xcl_fn]
fn dict_has(args: &[Value]) -> Value {
    let map = get_map(args[0]);
    let key = args[1].as_string().to_string();
    Value::bool_(map.contains_key(&key))
}

// ═══════════════════════════════════════════════════════════
//  Transformations
// ═══════════════════════════════════════════════════════════

/// Merge two dicts, second overrides first.
#[xcl_fn]
fn dict_merge(ctx: &mut DomainCtx) -> Value {
    let mut a = get_map(ctx.arg(0));
    let b = get_map(ctx.arg(1));
    for (k, v) in b {
        a.insert(k, v);
    }
    Value::from_map_boxed(a)
}

/// Filter dict by a predicate on values.
#[xcl_fn]
fn dict_filter(ctx: &mut DomainCtx) -> Value {
    let map = get_map(ctx.arg(0));
    let f = ctx.arg(1);
    let mut result = HashMap::new();
    for (k, v) in map {
        let cond = ctx.call(f, &[v]);
        if cond.as_bool() {
            result.insert(k, v);
        }
    }
    Value::from_map_boxed(result)
}

/// Map values through a function.
#[xcl_fn]
fn dict_map(ctx: &mut DomainCtx) -> Value {
    let map = get_map(ctx.arg(0));
    let f = ctx.arg(1);
    let mut result = HashMap::with_capacity(map.len());
    for (k, v) in map {
        let new_v = ctx.call(f, &[v]);
        result.insert(k, new_v);
    }
    Value::from_map_boxed(result)
}

// ═══════════════════════════════════════════════════════════
//  Properties
// ═══════════════════════════════════════════════════════════

/// Get the number of entries.
#[xcl_fn]
fn dict_len(args: &[Value]) -> Value {
    Value::from_i48(get_map(args[0]).len() as i64)
}

/// Check if dict is empty.
#[xcl_fn]
fn dict_is_empty(args: &[Value]) -> Value {
    Value::bool_(get_map(args[0]).is_empty())
}
