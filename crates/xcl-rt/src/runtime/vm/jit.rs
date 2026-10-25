//! Native-loop planner (feature `jit`).
//!
//! `prepare_jit` finds every fused `while` header in the root bytecode and asks
//! `xcl-jit` to compile it; `prepare_jit_functions` does the same for whole
//! integer functions. The compiled units are cached for the process lifetime, so
//! the cost is paid once per bytecode.

use super::*;

/// A loop compiled to native code (feature `jit`).
#[derive(Clone, Copy)]
pub(super) struct JitEntry {
    /// Interpreter ip to resume at after a normal exit.
    pub(super) exit_ip: usize,
    /// Native entry point.
    pub(super) func: xcl_jit::LoopFn,
    /// First index into `JitUnit::accesses`.
    pub(super) access_start: u32,
    /// Number of access plans for this loop.
    pub(super) access_len: u32,
}

/// All native loops compiled for one bytecode unit.
pub(super) struct JitUnit {
    pub(super) entries: Vec<JitEntry>,
    /// `shadow[ip]` is `0` when no loop starts at `ip`, otherwise `index + 1`.
    /// A flat table keeps the dispatch check to a bounds test and one load.
    pub(super) shadow: Vec<u32>,
    /// Memory-access plans, in entry order.
    pub(super) accesses: Vec<xcl_jit::AccessPlan>,
    /// One runtime cache per compiled loop.
    pub(super) caches: Vec<xcl_jit::LoopCache>,
}

/// The maximum number of compiled loops tracked per unit. The bail-out mask is
/// a `u64`, one bit per loop.
pub(super) const JIT_MAX_LOOPS: usize = 64;

/// Finds and compiles every hot integer loop of `bc`.
///
/// The compiled code is cached by `xcl-jit` for the life of the process, so
/// repeated `run` calls on the same bytecode pay the compile cost once.
pub(super) fn prepare_jit(bc: &Bytecode) -> Option<JitUnit> {
    // Cheap rejection first: most units have no fused `while` header at all, so
    // they must not pay for an instruction copy or a Cranelift lookup.
    let has_while = bc.instructions.iter().any(|i| {
        matches!(
            i.op(),
            crate::runtime::vm::ops::consts::OP_WHILE_LT_INT
                | crate::runtime::vm::ops::consts::OP_WHILE_LT_INT_WIDE
        )
    });
    if !has_while {
        return None;
    }
    let code: Vec<u64> = bc.instructions.iter().map(|i| i.0).collect();
    let specs = xcl_jit::find_loops(&code, &bc.wide_operands);
    if specs.is_empty() {
        return None;
    }
    let nlocals = bc.local_names.len().max(16);
    let mut entries = Vec::with_capacity(specs.len().min(JIT_MAX_LOOPS));
    let mut accesses: Vec<xcl_jit::AccessPlan> = Vec::new();
    let mut caches: Vec<xcl_jit::LoopCache> = Vec::new();
    let mut shadow = vec![0u32; code.len()];
    for spec in specs.iter().take(JIT_MAX_LOOPS) {
        if let Some(compiled) = xcl_jit::ensure_compiled(&code, &bc.wide_operands, spec, nlocals) {
            let idx = entries.len();
            let access_start = accesses.len() as u32;
            accesses.extend_from_slice(&compiled.accesses);
            entries.push(JitEntry {
                exit_ip: compiled.exit_ip,
                func: compiled.func,
                access_start,
                access_len: compiled.accesses.len() as u32,
            });
            caches.push(xcl_jit::LoopCache::default());
            shadow[spec.start] = (idx + 1) as u32;
        }
    }
    if entries.is_empty() {
        return None;
    }
    Some(JitUnit {
        entries,
        shadow,
        accesses,
        caches,
    })
}

/// Compiles integer-only functions (for example `fib`) into native entries.
///
/// The eligible set is small and strict: parameter-only locals, forward
/// branches, integer arithmetic, and exact-arity calls to other compiled
/// functions. Everything else stays on the interpreter.
pub(super) fn prepare_jit_functions(bc: &Bytecode) -> HashMap<Box<str>, xcl_jit::JitFunction> {
    let mut owned: Vec<(&str, Vec<u64>, Vec<i64>, &[Box<str>], usize, usize)> = Vec::new();
    for (name, func) in bc.user_functions.iter() {
        let Some(fb) = &func.bytecode else {
            continue;
        };
        let code: Vec<u64> = fb.instructions.iter().map(|i| i.0).collect();
        let wide: Vec<i64> = fb.wide_operands.to_vec();
        owned.push((
            name.as_ref(),
            code,
            wide,
            &fb.string_table[..],
            func.params.len(),
            fb.local_names.len().max(func.params.len()),
        ));
    }
    let inputs: Vec<xcl_jit::FunctionInput<'_>> = owned
        .iter()
        .map(
            |(name, code, wide, strings, params, locals)| xcl_jit::FunctionInput {
                name,
                code,
                wide,
                strings,
                params: *params,
                locals: *locals,
            },
        )
        .collect();
    xcl_jit::ensure_functions(&inputs).unwrap_or_default()
}

/// Largest array the JIT will scan to prove every element is an integer.
/// Above this, the loop stays interpreted rather than paying an O(n) scan.
pub(super) const JIT_ARRAY_SCAN_MAX: usize = 4096;

/// Fills a loop's access cache from the current frame registers.
///
/// Returns `false` when a container is not the expected type (for example a
/// map where the loop expects an array); the caller then leaves the loop to
/// the interpreter.
pub(super) fn fill_loop_cache(
    regs: &[Value],
    bc: &Bytecode,
    unit: &mut JitUnit,
    idx: usize,
    base: usize,
) -> bool {
    use crate::runtime::value::TAG_INT;

    let entry = unit.entries[idx];
    let start = entry.access_start as usize;
    let end = start + entry.access_len as usize;
    let plans = match unit.accesses.get(start..end) {
        Some(p) => p,
        None => return false,
    };
    let cache = match unit.caches.get_mut(idx) {
        Some(c) => c,
        None => return false,
    };
    for (plan, slot) in plans.iter().zip(cache.accesses.iter_mut()) {
        let Some(&container) = regs.get(base + plan.object_slot as usize) else {
            return false;
        };
        match plan.kind {
            xcl_jit::AccessKind::ArrayIndex => {
                let Some(arr) = container.as_array() else {
                    return false;
                };
                // Re-validate only when the array's version (or length)
                // changed. A short loop entered many times therefore scans
                // the elements once, not once per entry.
                let version = container.array_version().unwrap_or(0).wrapping_add(1);
                let mut stamp = slot.value;
                let same_object = slot.object == container.0;
                let validated = same_object && stamp == version && slot.len == arr.len();
                if !validated {
                    if arr.len() > JIT_ARRAY_SCAN_MAX || !arr.iter().all(|v| v.tag() == TAG_INT) {
                        return false;
                    }
                    stamp = version;
                }
                *slot = xcl_jit::AccessCache {
                    object: container.0,
                    data: arr.as_ptr().cast::<u64>(),
                    len: arr.len(),
                    value: stamp,
                };
            }
            xcl_jit::AccessKind::MapField => {
                let Some(&key) = bc.string_values.get(plan.key_slot as usize) else {
                    return false;
                };
                let Some(field) = container.as_map().and_then(|m| m.get_fast_value(key)) else {
                    return false;
                };
                if field.tag() != TAG_INT {
                    return false;
                }
                *slot = xcl_jit::AccessCache {
                    object: container.0,
                    data: core::ptr::null(),
                    len: 0,
                    value: field.0,
                };
            }
        }
    }
    true
}
