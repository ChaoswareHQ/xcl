//! Array operations domain.
//!
//! Inspired by Haskell Data.List, Perl List::MoreUtils, and Raku array methods.
//! All functions are registered under the `arr.*` domain prefix.

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

// ─── Helper: get array from args ───

fn get_array(v: Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

fn make_array(ctx: &DomainCtx, elements: Vec<Value>) -> Value {
    Value::from_array(elements, ctx.arena())
}

// ═══════════════════════════════════════════════════════════
//  Access
// ═══════════════════════════════════════════════════════════

/// Get the first element of an array.
#[xcl_fn(name = "arr.first", args = "a: array", ret = "any")]
fn arr_first(args: &[Value]) -> Value {
    get_array(args[0]).first().copied().unwrap_or(Value::nil())
}

/// Get the last element of an array.
#[xcl_fn(name = "arr.last", args = "a: array", ret = "any")]
fn arr_last(args: &[Value]) -> Value {
    get_array(args[0]).last().copied().unwrap_or(Value::nil())
}

/// Get element at index, or nil if out of bounds.
#[xcl_fn(name = "arr.get", args = "a: array; i: int", ret = "any")]
fn arr_get(args: &[Value]) -> Value {
    let arr = get_array(args[0]);
    let idx = args[1].as_i48() as usize;
    arr.get(idx).copied().unwrap_or(Value::nil())
}

/// Get the length of an array.
#[xcl_fn(name = "arr.len", args = "a: array", ret = "int")]
fn arr_len(args: &[Value]) -> Value {
    Value::from_i48(get_array(args[0]).len() as i64)
}

/// Check if array is empty.
#[xcl_fn(name = "arr.is_empty", args = "a: array", ret = "bool")]
fn arr_is_empty(args: &[Value]) -> Value {
    Value::bool_(get_array(args[0]).is_empty())
}

// ═══════════════════════════════════════════════════════════
//  Functional transformations
// ═══════════════════════════════════════════════════════════

/// Transform each element: `arr.map $arr (fn { $it * 2 })`.
#[xcl_fn(name = "arr.map", args = "a: array; f: any", ret = "array")]
fn arr_map(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    let mut result = Vec::with_capacity(arr.len());
    for item in arr {
        result.push(ctx.call(f, &[item]));
    }
    make_array(ctx, result)
}

/// Filter elements by predicate: `arr.filter $arr (fn { $it > 0 })`.
#[xcl_fn(name = "arr.filter", args = "a: array; f: any", ret = "array")]
fn arr_filter(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    let mut result = Vec::new();
    for item in arr {
        let cond = ctx.call(f, &[item]);
        if cond.as_bool() {
            result.push(item);
        }
    }
    make_array(ctx, result)
}

/// Reduce to a single value: `arr.reduce $arr 0 (fn { $acc + $it })`.
#[xcl_fn(name = "arr.reduce", args = "a: array; init: any; f: any", ret = "any")]
fn arr_reduce(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let mut acc = ctx.arg(1);
    let f = ctx.arg(2);
    for item in arr {
        acc = ctx.call(f, &[acc, item]);
    }
    acc
}

/// Partition into two arrays: `arr.partition $arr (fn { $it > 0 })`.
#[xcl_fn(name = "arr.partition", args = "a: array; f: any", ret = "array")]
fn arr_partition(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    let mut left = Vec::new();
    let mut right = Vec::new();
    for item in arr {
        let cond = ctx.call(f, &[item]);
        if cond.as_bool() {
            left.push(item);
        } else {
            right.push(item);
        }
    }
    let result = vec![make_array(ctx, left), make_array(ctx, right)];
    make_array(ctx, result)
}

// ═══════════════════════════════════════════════════════════
//  Sorting
// ═══════════════════════════════════════════════════════════

/// Sort array in ascending order.
#[xcl_fn(name = "arr.sort", args = "a: array", ret = "array")]
fn arr_sort(ctx: &mut DomainCtx) -> Value {
    let mut arr = get_array(ctx.arg(0));
    arr.sort_by(|a, b| {
        // Compare as numbers if possible, otherwise as strings
        if a.is_int() && b.is_int() {
            a.as_i48().cmp(&b.as_i48())
        } else if a.is_float() && b.is_float() {
            a.as_f64().partial_cmp(&b.as_f64()).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.as_string().cmp(&b.as_string())
        }
    });
    make_array(ctx, arr)
}

/// Sort array using a custom key function.
#[xcl_fn(name = "arr.sort_by", args = "a: array; f: any", ret = "array")]
fn arr_sort_by(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    // Pre-compute keys for each element
    let mut keys: Vec<(Value, Value)> = arr.iter().map(|v| (*v, ctx.call(f, &[*v]))).collect();
    keys.sort_by(|(_, ka), (_, kb)| {
        if ka.is_int() && kb.is_int() {
            ka.as_i48().cmp(&kb.as_i48())
        } else if ka.is_float() && kb.is_float() {
            ka.as_f64().partial_cmp(&kb.as_f64()).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            ka.as_string().cmp(&kb.as_string())
        }
    });
    let result = keys.into_iter().map(|(v, _)| v).collect();
    make_array(ctx, result)
}

/// Reverse the array.
#[xcl_fn(name = "arr.reverse", args = "a: array", ret = "array")]
fn arr_reverse(ctx: &mut DomainCtx) -> Value {
    let mut arr = get_array(ctx.arg(0));
    arr.reverse();
    make_array(ctx, arr)
}

// ═══════════════════════════════════════════════════════════
//  Search
// ═══════════════════════════════════════════════════════════

/// Check if array contains a value.
#[xcl_fn(name = "arr.contains", args = "a: array; v: any", ret = "bool")]
fn arr_contains(args: &[Value]) -> Value {
    let arr = get_array(args[0]);
    let v = args[1];
    Value::bool_(arr.iter().any(|item| item.eq_(v)))
}

/// Count elements matching a predicate.
#[xcl_fn(name = "arr.count", args = "a: array; f: any", ret = "int")]
fn arr_count(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    let mut count = 0i64;
    for item in arr {
        let cond = ctx.call(f, &[item]);
        if cond.as_bool() {
            count += 1;
        }
    }
    Value::from_i48(count)
}

/// Find first element matching a predicate.
#[xcl_fn(name = "arr.find", args = "a: array; f: any", ret = "any")]
fn arr_find(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    for item in arr {
        let cond = ctx.call(f, &[item]);
        if cond.as_bool() {
            return item;
        }
    }
    Value::nil()
}

/// Find index of first element matching a predicate.
#[xcl_fn(name = "arr.find_index", args = "a: array; f: any", ret = "int")]
fn arr_find_index(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    for (i, item) in arr.iter().enumerate() {
        let cond = ctx.call(f, &[*item]);
        if cond.as_bool() {
            return Value::from_i48(i as i64);
        }
    }
    Value::from_i48(-1)
}

// ═══════════════════════════════════════════════════════════
//  Aggregation
// ═══════════════════════════════════════════════════════════

/// Sum all numeric elements.
#[xcl_fn(name = "arr.sum", args = "a: array", ret = "float")]
fn arr_sum(args: &[Value]) -> Value {
    let arr = get_array(args[0]);
    let sum: f64 = arr.iter().map(|v| {
        if v.is_float() { v.as_f64() } else if v.is_int() { v.as_i48() as f64 } else { 0.0 }
    }).sum();
    Value::from_f64(sum)
}

/// Product of all numeric elements.
#[xcl_fn(name = "arr.product", args = "a: array", ret = "float")]
fn arr_product(args: &[Value]) -> Value {
    let arr = get_array(args[0]);
    let product: f64 = arr.iter().map(|v| {
        if v.is_float() { v.as_f64() } else if v.is_int() { v.as_i48() as f64 } else { 1.0 }
    }).product();
    Value::from_f64(product)
}

/// Minimum value.
#[xcl_fn(name = "arr.min", args = "a: array", ret = "any")]
fn arr_min(args: &[Value]) -> Value {
    let arr = get_array(args[0]);
    let mut min = arr.first().copied().unwrap_or(Value::nil());
    for item in &arr[1..] {
        if item.lt(min) {
            min = *item;
        }
    }
    min
}

/// Maximum value.
#[xcl_fn(name = "arr.max", args = "a: array", ret = "any")]
fn arr_max(args: &[Value]) -> Value {
    let arr = get_array(args[0]);
    let mut max = arr.first().copied().unwrap_or(Value::nil());
    for item in &arr[1..] {
        if max.lt(*item) {
            max = *item;
        }
    }
    max
}

// ═══════════════════════════════════════════════════════════
//  Predicates
// ═══════════════════════════════════════════════════════════

/// Check if any element matches a predicate.
#[xcl_fn(name = "arr.any", args = "a: array; f: any", ret = "bool")]
fn arr_any(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    for item in arr {
        let cond = ctx.call(f, &[item]);
        if cond.as_bool() {
            return Value::bool_(true);
        }
    }
    Value::bool_(false)
}

/// Check if all elements match a predicate.
#[xcl_fn(name = "arr.all", args = "a: array; f: any", ret = "bool")]
fn arr_all(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    for item in arr {
        let cond = ctx.call(f, &[item]);
        if !cond.as_bool() {
            return Value::bool_(false);
        }
    }
    Value::bool_(true)
}

// ═══════════════════════════════════════════════════════════
//  Sublists
// ═══════════════════════════════════════════════════════════

/// Take first n elements.
#[xcl_fn(name = "arr.take", args = "a: array; n: int", ret = "array")]
fn arr_take(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let n = ctx.arg(1).as_i48() as usize;
    make_array(ctx, arr.into_iter().take(n).collect())
}

/// Drop first n elements.
#[xcl_fn(name = "arr.drop", args = "a: array; n: int", ret = "array")]
fn arr_drop(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let n = ctx.arg(1).as_i48() as usize;
    make_array(ctx, arr.into_iter().skip(n).collect())
}

/// Slice from start to end (exclusive).
#[xcl_fn(name = "arr.slice", args = "a: array; start: int; end: int", ret = "array")]
fn arr_slice(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let start = ctx.arg(1).as_i48() as usize;
    let end = ctx.arg(2).as_i48() as usize;
    let len = arr.len();
    let start = start.min(len);
    let end = end.min(len);
    make_array(ctx, arr[start..end].to_vec())
}

// ═══════════════════════════════════════════════════════════
//  Combination
// ═══════════════════════════════════════════════════════════

/// Flatten nested arrays one level.
#[xcl_fn(name = "arr.flatten", args = "a: array", ret = "array")]
fn arr_flatten(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let mut result = Vec::new();
    for item in arr {
        if let Some(inner) = item.as_array() {
            result.extend(inner.iter().copied());
        } else {
            result.push(item);
        }
    }
    make_array(ctx, result)
}

/// Zip two arrays into pairs.
#[xcl_fn(name = "arr.zip", args = "a: array; b: array", ret = "array")]
fn arr_zip(ctx: &mut DomainCtx) -> Value {
    let a = get_array(ctx.arg(0));
    let b = get_array(ctx.arg(1));
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    for (ai, bi) in a.iter().zip(b.iter()) {
        let pair = vec![*ai, *bi];
        result.push(make_array(ctx, pair));
    }
    make_array(ctx, result)
}

/// Remove duplicates, preserving order.
#[xcl_fn(name = "arr.unique", args = "a: array", ret = "array")]
fn arr_unique(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let mut result = Vec::new();
    for item in arr {
        if !result.iter().any(|existing: &Value| existing.eq_(item)) {
            result.push(item);
        }
    }
    make_array(ctx, result)
}

/// Chunk array into groups of size n.
#[xcl_fn(name = "arr.chunk", args = "a: array; n: int", ret = "array")]
fn arr_chunk(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let n = ctx.arg(1).as_i48() as usize;
    if n == 0 { return make_array(ctx, Vec::new()); }
    let mut result = Vec::new();
    for chunk in arr.chunks(n) {
        result.push(make_array(ctx, chunk.to_vec()));
    }
    make_array(ctx, result)
}

/// Interleave two arrays.
#[xcl_fn(name = "arr.interleave", args = "a: array; b: array", ret = "array")]
fn arr_interleave(ctx: &mut DomainCtx) -> Value {
    let a = get_array(ctx.arg(0));
    let b = get_array(ctx.arg(1));
    let mut result = Vec::with_capacity(a.len() + b.len());
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() || bi < b.len() {
        if ai < a.len() { result.push(a[ai]); ai += 1; }
        if bi < b.len() { result.push(b[bi]); bi += 1; }
    }
    make_array(ctx, result)
}

// ═══════════════════════════════════════════════════════════
//  Grouping
// ═══════════════════════════════════════════════════════════

/// Group elements by a key function, returns a dict of arrays.
#[xcl_fn(name = "arr.group_by", args = "a: array; f: any", ret = "map")]
fn arr_group_by(ctx: &mut DomainCtx) -> Value {
    let arr = get_array(ctx.arg(0));
    let f = ctx.arg(1);
    let mut groups: std::collections::HashMap<String, Vec<Value>> = std::collections::HashMap::new();
    for item in arr {
        let key = ctx.call(f, &[item]);
        let key_str = key.as_string().to_string();
        groups.entry(key_str).or_default().push(item);
    }
    // Convert to XCL map
    let mut map = std::collections::HashMap::with_capacity(groups.len());
    for (key, values) in groups {
        map.insert(key, make_array(ctx, values));
    }
    Value::from_map_boxed(map)
}
