//! String operations domain.
//!
//! Inspired by Raku string methods and Perl string functions.
//! All functions are registered under the `str.*` domain prefix.

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

// ─── Helper ───

fn get_str(v: Value) -> String {
    v.as_string().to_string()
}

// ═══════════════════════════════════════════════════════════
//  Case conversion
// ═══════════════════════════════════════════════════════════

/// Convert to uppercase.
#[xcl_fn(name = "str.upper", args = "s: string", ret = "string")]
fn str_upper(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    Value::from_str(s.to_uppercase(), ctx.arena())
}

/// Convert to lowercase.
#[xcl_fn(name = "str.lower", args = "s: string", ret = "string")]
fn str_lower(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    Value::from_str(s.to_lowercase(), ctx.arena())
}

/// Convert to title case (first letter of each word uppercase).
#[xcl_fn(name = "str.title", args = "s: string", ret = "string")]
fn str_title(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let titled: String = s.chars()
        .flat_map(|c| {
            if c.is_alphanumeric() {
                vec![c.to_uppercase().next().unwrap(), ' ']
            } else {
                vec![c]
            }
        })
        .collect();
    Value::from_str(titled.trim().to_string(), ctx.arena())
}

// ═══════════════════════════════════════════════════════════
//  Whitespace
// ═══════════════════════════════════════════════════════════

/// Trim whitespace from both ends.
#[xcl_fn(name = "str.trim", args = "s: string", ret = "string")]
fn str_trim(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    Value::from_str(s.trim().to_string(), ctx.arena())
}

/// Trim whitespace from the start.
#[xcl_fn(name = "str.trim_start", args = "s: string", ret = "string")]
fn str_trim_start(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    Value::from_str(s.trim_start().to_string(), ctx.arena())
}

/// Trim whitespace from the end.
#[xcl_fn(name = "str.trim_end", args = "s: string", ret = "string")]
fn str_trim_end(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    Value::from_str(s.trim_end().to_string(), ctx.arena())
}

// ═══════════════════════════════════════════════════════════
//  Splitting and joining
// ═══════════════════════════════════════════════════════════

/// Split string by delimiter.
#[xcl_fn(name = "str.split", args = "s: string; delimiter: string", ret = "array")]
fn str_split(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let delimiter = get_str(ctx.arg(1));
    let parts: Vec<Value> = s.split(&delimiter)
        .map(|p| Value::from_str(p.to_string(), ctx.arena()))
        .collect();
    Value::from_array(parts, ctx.arena())
}

/// Join array of strings with a delimiter.
#[xcl_fn(name = "str.join", args = "delimiter: string; a: array", ret = "string")]
fn str_join(ctx: &mut DomainCtx) -> Value {
    let delimiter = get_str(ctx.arg(0));
    let arr = ctx.arg(1).as_array().map(|a| a.clone()).unwrap_or_default();
    let parts: Vec<String> = arr.iter().map(|v| v.as_string().to_string()).collect();
    Value::from_str(parts.join(&delimiter), ctx.arena())
}

/// Split into lines.
#[xcl_fn(name = "str.lines", args = "s: string", ret = "array")]
fn str_lines(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let parts: Vec<Value> = s.lines()
        .map(|l| Value::from_str(l.to_string(), ctx.arena()))
        .collect();
    Value::from_array(parts, ctx.arena())
}

/// Split into words.
#[xcl_fn(name = "str.words", args = "s: string", ret = "array")]
fn str_words(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let parts: Vec<Value> = s.split_whitespace()
        .map(|w| Value::from_str(w.to_string(), ctx.arena()))
        .collect();
    Value::from_array(parts, ctx.arena())
}

// ═══════════════════════════════════════════════════════════
//  Search
// ═══════════════════════════════════════════════════════════

/// Check if string starts with prefix.
#[xcl_fn(name = "str.starts_with", args = "s: string; prefix: string", ret = "bool")]
fn str_starts_with(args: &[Value]) -> Value {
    let s = get_str(args[0]);
    let prefix = get_str(args[1]);
    Value::bool_(s.starts_with(&prefix))
}

/// Check if string ends with suffix.
#[xcl_fn(name = "str.ends_with", args = "s: string; suffix: string", ret = "bool")]
fn str_ends_with(args: &[Value]) -> Value {
    let s = get_str(args[0]);
    let suffix = get_str(args[1]);
    Value::bool_(s.ends_with(&suffix))
}

/// Check if string contains substring.
#[xcl_fn(name = "str.contains", args = "s: string; substring: string", ret = "bool")]
fn str_contains(args: &[Value]) -> Value {
    let s = get_str(args[0]);
    let substring = get_str(args[1]);
    Value::bool_(s.contains(&substring))
}

/// Find index of first occurrence of substring.
#[xcl_fn(name = "str.find", args = "s: string; substring: string", ret = "int")]
fn str_find(args: &[Value]) -> Value {
    let s = get_str(args[0]);
    let substring = get_str(args[1]);
    match s.find(&substring) {
        Some(idx) => Value::from_i48(idx as i64),
        None => Value::from_i48(-1),
    }
}

// ═══════════════════════════════════════════════════════════
//  Manipulation
// ═══════════════════════════════════════════════════════════

/// Replace all occurrences of pattern with replacement.
#[xcl_fn(name = "str.replace", args = "s: string; pattern: string; replacement: string", ret = "string")]
fn str_replace(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let pattern = get_str(ctx.arg(1));
    let replacement = get_str(ctx.arg(2));
    Value::from_str(s.replace(&pattern, &replacement), ctx.arena())
}

/// Extract a slice of the string.
#[xcl_fn(name = "str.slice", args = "s: string; start: int; end: int", ret = "string")]
fn str_slice(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let start = ctx.arg(1).as_i48() as usize;
    let end = ctx.arg(2).as_i48() as usize;
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let start = start.min(len);
    let end = end.min(len);
    let sliced: String = chars[start..end].iter().collect();
    Value::from_str(sliced, ctx.arena())
}

/// Repeat string n times.
#[xcl_fn(name = "str.repeat", args = "s: string; n: int", ret = "string")]
fn str_repeat(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let n = ctx.arg(1).as_i48() as usize;
    Value::from_str(s.repeat(n), ctx.arena())
}

/// Pad string to the left with a character.
#[xcl_fn(name = "str.pad_left", args = "s: string; width: int; pad: string", ret = "string")]
fn str_pad_left(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let width = ctx.arg(1).as_i48() as usize;
    let pad = get_str(ctx.arg(2));
    let pad_char = pad.chars().next().unwrap_or(' ');
    let s_width = s.chars().count();
    if width > s_width {
        let padded = format!("{:>width$}", s, width = width);
        Value::from_str(padded.replace(' ', &pad_char.to_string()), ctx.arena())
    } else {
        Value::from_str(s, ctx.arena())
    }
}

/// Pad string to the right with a character.
#[xcl_fn(name = "str.pad_right", args = "s: string; width: int; pad: string", ret = "string")]
fn str_pad_right(ctx: &mut DomainCtx) -> Value {
    let s = get_str(ctx.arg(0));
    let width = ctx.arg(1).as_i48() as usize;
    let pad = get_str(ctx.arg(2));
    let pad_char = pad.chars().next().unwrap_or(' ');
    let s_width = s.chars().count();
    if width > s_width {
        let padded = format!("{:<width$}", s, width = width);
        Value::from_str(padded.replace(' ', &pad_char.to_string()), ctx.arena())
    } else {
        Value::from_str(s, ctx.arena())
    }
}

// ═══════════════════════════════════════════════════════════
//  Properties
// ═══════════════════════════════════════════════════════════

/// Get the length of a string.
#[xcl_fn(name = "str.len", args = "s: string", ret = "int")]
fn str_len(args: &[Value]) -> Value {
    let s = get_str(args[0]);
    Value::from_i48(s.chars().count() as i64)
}

/// Check if string is empty.
#[xcl_fn(name = "str.is_empty", args = "s: string", ret = "bool")]
fn str_is_empty(args: &[Value]) -> Value {
    let s = get_str(args[0]);
    Value::bool_(s.is_empty())
}
