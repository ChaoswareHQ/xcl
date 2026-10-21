//! Integer-function JIT: whole `if`/`return`/recursive functions.
//!
//! The loop JIT compiles one basic block. This module handles a different
//! shape: small integer functions such as
//!
//! ```xcl
//! fib($n) {
//!     if $n < 2 { return $n }
//!     return (fib ($n - 1)) + (fib ($n - 2))
//! }
//! ```
//!
//! Eligible functions are compiled into **one** `JITModule`, so a recursive
//! call is a direct native call. The VM only enters a compiled function when
//! the argument count matches the declared parameters and every argument is an
//! integer; anything else falls back to the interpreter.
//!
//! Scope (deliberately narrow, extended only with tests):
//!
//! * integer locals only, at most four parameters;
//! * forward branches only (`IF_LT_INT`, `IF_EQ_INT`, `JUMP`), no loops;
//! * `ADD`/`SUB`, small-constant add/sub, local loads/stores, integer constants;
//! * direct `CALL_NAME` to another compiled function, with the exact parameter
//!   count;
//! * every local is definitely assigned on every path before it is read
//!   (see [`definite_locals`]).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::mem;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, UserFuncName, Value};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{Linkage, Module};

use crate::make_module;

/// The native entry point for a compiled function.
///
/// Four argument slots cover every supported function; unused slots are zero.
pub type FunctionFn = unsafe extern "C" fn(i64, i64, i64, i64) -> i64;

/// A compiled function and its declared parameter count.
#[derive(Clone, Copy)]
pub struct JitFunction {
    /// Native entry point.
    pub func: FunctionFn,
    /// Number of parameters the function expects.
    pub nparams: u32,
}

/// One function offered to [`ensure_functions`].
pub struct FunctionInput<'a> {
    /// Function name as it appears in `CALL_NAME`.
    pub name: &'a str,
    /// Instruction words.
    pub code: &'a [u64],
    /// Wide operand table.
    pub wide: &'a [i64],
    /// String table used by `CALL_NAME`.
    pub strings: &'a [Box<str>],
    /// Declared parameter count.
    pub params: usize,
    /// Number of local slots.
    pub locals: usize,
}

// ── Opcodes (mirrors `xcl-vm/src/runtime/vm/ops.rs`) ──────────────────────

const OP_PUSH_INT: u8 = 4;
const OP_LOAD_LOCAL: u8 = 8;
const OP_STORE_LOCAL: u8 = 9;
const OP_ADD: u8 = 12;
const OP_SUB: u8 = 13;
const OP_JUMP: u8 = 28;
const OP_RETURN: u8 = 33;
const OP_HALT: u8 = 34;
const OP_LOAD_LOCAL_LOAD_LOCAL: u8 = 60;
const OP_CALL_NAME: u8 = 67;
const OP_IF_EQ_INT: u8 = 104;
const OP_LOAD_CONST_LOCAL: u8 = 108;
const OP_PUSH_LOCAL_INT_OP: u8 = 125;
const OP_IF_LT_INT: u8 = 126;

#[inline(always)]
fn op_of(i: u64) -> u8 {
    (i & 0xFF) as u8
}
#[inline(always)]
fn a_of(i: u64) -> u32 {
    ((i >> 8) & 0xFFFF_FFFF) as u32
}
#[inline(always)]
fn b_of(i: u64) -> u16 {
    ((i >> 40) & 0xFFFF) as u16
}
#[inline(always)]
fn c_of(i: u64) -> u8 {
    ((i >> 56) & 0xFF) as u8
}

fn supported(op: u8) -> bool {
    matches!(
        op,
        OP_PUSH_INT
            | OP_LOAD_LOCAL
            | OP_STORE_LOCAL
            | OP_ADD
            | OP_SUB
            | OP_JUMP
            | OP_RETURN
            | OP_HALT
            | OP_LOAD_LOCAL_LOAD_LOCAL
            | OP_CALL_NAME
            | OP_IF_EQ_INT
            | OP_LOAD_CONST_LOCAL
            | OP_PUSH_LOCAL_INT_OP
            | OP_IF_LT_INT
    )
}

fn trunc48(b: &mut FunctionBuilder, v: Value) -> Value {
    let lo = b.ins().ishl_imm_s(v, 16);
    b.ins().sshr_imm_s(lo, 16)
}

// ── Cache ─────────────────────────────────────────────────────────────────

struct FunctionSet {
    _module: JITModule,
    map: HashMap<Box<str>, JitFunction>,
}

thread_local! {
    static CACHE: std::cell::RefCell<HashMap<u64, FunctionSet>> =
        std::cell::RefCell::new(HashMap::new());
}

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Compiles every eligible function in `funcs` (or returns the cached set).
///
/// Functions that fail validation, or that call a function which is not
/// compiled, are dropped; the rest are still useful. The returned map is a
/// snapshot of name → entry point. The module itself stays alive in a
/// thread-local cache, because executable memory must outlive the map.
pub fn ensure_functions(funcs: &[FunctionInput<'_>]) -> Option<HashMap<Box<str>, JitFunction>> {
    if funcs.is_empty() {
        return None;
    }
    let params_by_name: HashMap<&str, usize> = funcs.iter().map(|f| (f.name, f.params)).collect();
    if params_by_name.len() != funcs.len() {
        return None;
    }

    // Start with the structurally valid functions, then drop any function that
    // calls one which is not compiled (a fixpoint over the call graph).
    let mut eligible: HashSet<&str> = funcs
        .iter()
        .filter(|f| validate(f, &params_by_name).is_some())
        .map(|f| f.name)
        .collect();
    loop {
        let mut changed = false;
        let current: Vec<&str> = eligible.iter().copied().collect();
        for name in current {
            let f = funcs.iter().find(|f| f.name == name)?;
            if calls(f)
                .into_iter()
                .any(|callee| !eligible.contains(callee))
            {
                eligible.remove(name);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let selected: Vec<&FunctionInput<'_>> =
        funcs.iter().filter(|f| eligible.contains(f.name)).collect();
    if selected.is_empty() {
        return None;
    }

    let mut key = 0xCBF2_9CE4_8422_2325u64;
    for f in &selected {
        key = fnv(key, f.name.as_bytes());
        key = fnv(key, &(f.params as u64).to_le_bytes());
        key = fnv(key, &(f.locals as u64).to_le_bytes());
        for &i in f.code {
            key = fnv(key, &i.to_le_bytes());
        }
        for &w in f.wide {
            key = fnv(key, &w.to_le_bytes());
        }
        // The callee of a `CALL_NAME` is resolved through the string table at
        // compile time, so the table is part of the compiled code's identity.
        for s in f.strings {
            key = fnv(key, s.as_bytes());
            key = fnv(key, &[0]);
        }
    }

    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(set) = cache.get(&key) {
            return Some(set.map.clone());
        }
        let set = compile_set(&selected)?;
        let map = set.map.clone();
        cache.insert(key, set);
        Some(map)
    })
}

/// Names referenced by `CALL_NAME` in this function.
fn calls<'a>(f: &'a FunctionInput<'a>) -> Vec<&'a str> {
    let mut out = Vec::new();
    for &ins in f.code {
        if op_of(ins) == OP_CALL_NAME
            && let Some(name) = f.strings.get(b_of(ins) as usize)
        {
            out.push(name.as_ref());
        }
    }
    out
}

// ── Validation ────────────────────────────────────────────────────────────

fn read_slots(ins: u64, out: &mut Vec<usize>) {
    let a = a_of(ins) as usize;
    let b = b_of(ins) as usize;
    match op_of(ins) {
        OP_LOAD_LOCAL | OP_PUSH_LOCAL_INT_OP | OP_IF_LT_INT | OP_IF_EQ_INT => out.push(a),
        OP_LOAD_LOCAL_LOAD_LOCAL => {
            out.push(a);
            out.push(b);
        }
        _ => {}
    }
}

/// Control-flow effect of one instruction.
#[derive(Clone, Copy)]
enum Flow {
    /// Falls through to the next instruction.
    Next,
    /// Unconditional jump.
    Jump(usize),
    /// Conditional: `fall` when the test is true, `target` otherwise.
    Branch(usize, usize),
    /// `RETURN` or `HALT`: no successors.
    Stop,
}

fn flow(f: &FunctionInput<'_>, ip: usize) -> Flow {
    let ins = f.code[ip];
    match op_of(ins) {
        OP_RETURN | OP_HALT => Flow::Stop,
        OP_JUMP => Flow::Jump(a_of(ins) as usize),
        OP_IF_LT_INT | OP_IF_EQ_INT => {
            let target = f.wide[c_of(ins) as usize] as usize;
            Flow::Branch(ip + 1, target)
        }
        _ => Flow::Next,
    }
}

/// Instructions reachable from `ip = 0`.
fn reachable_ips(f: &FunctionInput<'_>) -> Vec<bool> {
    let n = f.code.len();
    let mut seen = vec![false; n];
    let mut work = vec![0usize];
    while let Some(ip) = work.pop() {
        if ip >= n || seen[ip] {
            continue;
        }
        seen[ip] = true;
        match flow(f, ip) {
            Flow::Next => work.push(ip + 1),
            Flow::Jump(target) => work.push(target),
            Flow::Branch(fall, target) => {
                work.push(fall);
                work.push(target);
            }
            Flow::Stop => {}
        }
    }
    seen
}

/// Proves that every local is assigned on **every** path before it is read.
///
/// This is a forward dataflow pass over the leader blocks. Every branch in an
/// eligible function goes forward, so visiting blocks in instruction order
/// settles the intersection in one pass: by the time a block is reached, all of
/// its predecessors have contributed their live-in set.
fn definite_locals(f: &FunctionInput<'_>, leaders: &BTreeSet<usize>) -> bool {
    let n = f.code.len();
    let starts: Vec<usize> = leaders.iter().copied().collect();
    let mut live_in: Vec<Option<BTreeSet<usize>>> = vec![None; n];
    live_in[0] = Some((0..f.params).collect());

    let mut buf = Vec::new();
    for (bi, &start) in starts.iter().enumerate() {
        let Some(mut avail) = live_in[start].take() else {
            // Unreachable block: nothing to prove.
            continue;
        };
        let end = starts.get(bi + 1).copied().unwrap_or(n);
        let mut exits = [usize::MAX; 2];
        for ip in start..end {
            let ins = f.code[ip];
            buf.clear();
            read_slots(ins, &mut buf);
            for &slot in &buf {
                if slot >= f.locals || !avail.contains(&slot) {
                    return false;
                }
            }
            if matches!(op_of(ins), OP_STORE_LOCAL | OP_LOAD_CONST_LOCAL) {
                let slot = a_of(ins) as usize;
                if slot >= f.locals {
                    return false;
                }
                avail.insert(slot);
            }
            match flow(f, ip) {
                Flow::Next => {}
                Flow::Jump(target) => {
                    exits[0] = target;
                    break;
                }
                Flow::Branch(fall, target) => {
                    exits = [fall, target];
                    break;
                }
                Flow::Stop => break,
            }
        }
        for succ in exits {
            if succ == usize::MAX || succ >= n {
                continue;
            }
            match &mut live_in[succ] {
                Some(existing) => {
                    existing.retain(|slot| avail.contains(slot));
                }
                None => live_in[succ] = Some(avail.clone()),
            }
        }
    }
    true
}

fn validate(f: &FunctionInput<'_>, params_by_name: &HashMap<&str, usize>) -> Option<()> {
    if f.params > 4 || f.code.is_empty() {
        return None;
    }

    let n = f.code.len();
    let mut leaders: BTreeSet<usize> = BTreeSet::new();
    leaders.insert(0);
    for (ip, &ins) in f.code.iter().enumerate() {
        let op = op_of(ins);
        if !supported(op) {
            return None;
        }
        match op {
            OP_IF_LT_INT | OP_IF_EQ_INT => {
                let target = *f.wide.get(c_of(ins) as usize)? as usize;
                // `target == ip + 1` is a no-op branch that would give one
                // successor two edges from the same `brif`; there is nothing
                // to gain from it.
                if target <= ip + 1 || target >= n {
                    return None;
                }
                leaders.insert(target);
            }
            OP_JUMP => {
                let target = a_of(ins) as usize;
                if target <= ip || target >= n {
                    return None;
                }
                leaders.insert(target);
            }
            // Emission supports add (0) and sub (1) only.
            OP_PUSH_LOCAL_INT_OP if c_of(ins) > 1 => {
                return None;
            }
            // 0 = small integer in `b`, 6 = wide integer in `b`.
            OP_LOAD_CONST_LOCAL if !matches!(c_of(ins), 0 | 6) => {
                return None;
            }
            _ => {}
        }
    }
    if !definite_locals(f, &leaders) {
        return None;
    }

    let mut depth: i32 = 0;
    let mut reachable = true;
    for (ip, &ins) in f.code.iter().enumerate() {
        if leaders.contains(&ip) {
            if depth != 0 {
                return None;
            }
            reachable = true;
        }
        if !reachable {
            continue;
        }
        match op_of(ins) {
            OP_PUSH_INT | OP_LOAD_LOCAL | OP_PUSH_LOCAL_INT_OP | OP_LOAD_CONST_LOCAL => depth += 1,
            OP_LOAD_LOCAL_LOAD_LOCAL => depth += 2,
            OP_STORE_LOCAL | OP_ADD | OP_SUB => depth -= 1,
            OP_CALL_NAME => {
                let nargs = a_of(ins) as usize;
                let name = f.strings.get(b_of(ins) as usize)?;
                let params = *params_by_name.get(name.as_ref())?;
                if nargs != params {
                    return None;
                }
                depth = depth - nargs as i32 + 1;
            }
            OP_RETURN => {
                if depth != 1 {
                    return None;
                }
                depth = 0;
                reachable = false;
            }
            OP_HALT => {
                depth = 0;
                reachable = false;
            }
            OP_IF_LT_INT | OP_IF_EQ_INT | OP_JUMP if depth != 0 => {
                return None;
            }
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    Some(())
}

// ── Compilation ───────────────────────────────────────────────────────────

fn compile_set(funcs: &[&FunctionInput<'_>]) -> Option<FunctionSet> {
    let mut module = make_module().ok()?;

    let mut sig = module.make_signature();
    for _ in 0..4 {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let mut ids = Vec::with_capacity(funcs.len());
    for i in 0..funcs.len() {
        let name = format!("xcl_fn_{i}");
        let id = module.declare_function(&name, Linkage::Local, &sig).ok()?;
        ids.push(id);
    }
    let by_name: HashMap<&str, usize> =
        funcs.iter().enumerate().map(|(i, f)| (f.name, i)).collect();

    for (i, f) in funcs.iter().enumerate() {
        let mut ctx = module.make_context();
        ctx.func.signature = sig.clone();
        ctx.func.name = UserFuncName::user(0, i as u32);
        let mut fb_ctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
            let func_refs: Vec<_> = ids
                .iter()
                .map(|&id| module.declare_func_in_func(id, b.func))
                .collect();
            if !emit_function(&mut b, f, &func_refs, &by_name) {
                return None;
            }
            b.seal_all_blocks();
            b.finalize(module.target_config());
        }
        module.define_function(ids[i], &mut ctx).ok()?;
        module.clear_context(&mut ctx);
    }
    module.finalize_definitions().ok()?;

    let mut map = HashMap::with_capacity(funcs.len());
    for (i, f) in funcs.iter().enumerate() {
        let ptr = module.get_finalized_function(ids[i]);
        // SAFETY: the declared signature is `(i64, i64, i64, i64) -> i64`.
        let func: FunctionFn = unsafe { mem::transmute(ptr) };
        map.insert(
            f.name.into(),
            JitFunction {
                func,
                nparams: f.params as u32,
            },
        );
    }
    Some(FunctionSet {
        _module: module,
        map,
    })
}

fn emit_function(
    b: &mut FunctionBuilder,
    f: &FunctionInput<'_>,
    func_refs: &[cranelift_codegen::ir::FuncRef],
    by_name: &HashMap<&str, usize>,
) -> bool {
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    b.seal_block(entry);

    let params = b.block_params(entry).to_vec();
    let vars: Vec<_> = (0..f.locals).map(|_| b.declare_var(types::I64)).collect();
    let var = |slot: usize| vars[slot];
    let zero = b.ins().iconst(types::I64, 0);
    for slot in 0..f.locals {
        let init = if slot < f.params && slot < params.len() {
            params[slot]
        } else {
            zero
        };
        b.def_var(var(slot), init);
    }

    // Blocks are created only for reachable branch targets: a target in dead
    // code would otherwise be emitted with variables that no live path defined.
    let reachable = reachable_ips(f);
    let mut blocks: Vec<Option<cranelift_codegen::ir::Block>> = vec![None; f.code.len()];
    blocks[0] = Some(entry);
    for (ip, &ins) in f.code.iter().enumerate() {
        if !reachable[ip] {
            continue;
        }
        let target = match op_of(ins) {
            OP_IF_LT_INT | OP_IF_EQ_INT => f.wide[c_of(ins) as usize] as usize,
            OP_JUMP => a_of(ins) as usize,
            _ => continue,
        };
        if reachable[target] && blocks[target].is_none() {
            blocks[target] = Some(b.create_block());
        }
    }

    let mut current: Option<cranelift_codegen::ir::Block> = Some(entry);
    let mut stack: Vec<Value> = Vec::new();

    for (ip, &ins) in f.code.iter().enumerate() {
        if let Some(next) = blocks[ip]
            && current != Some(next)
        {
            if let Some(cur) = current {
                b.ins().jump(next, &[]);
                let _ = cur;
            }
            // Also entered when `current` is `None` (after a `RETURN`/`JUMP`):
            // the block still has to become the insertion point.
            b.switch_to_block(next);
            current = Some(next);
            stack.clear();
        }
        if current.is_none() {
            continue;
        }

        let a = a_of(ins) as usize;
        let bb = b_of(ins) as usize;
        let cc = c_of(ins) as usize;
        match op_of(ins) {
            OP_PUSH_INT => {
                let v = b.ins().iconst(types::I64, a_of(ins) as i64);
                stack.push(v);
            }
            OP_LOAD_LOCAL => {
                let v = b.use_var(var(a));
                stack.push(v);
            }
            OP_LOAD_LOCAL_LOAD_LOCAL => {
                let x = b.use_var(var(a));
                let y = b.use_var(var(bb));
                stack.push(x);
                stack.push(y);
            }
            OP_STORE_LOCAL => {
                let Some(v) = stack.pop() else {
                    return false;
                };
                b.def_var(var(a), v);
            }
            OP_ADD | OP_SUB => {
                let Some(y) = stack.pop() else {
                    return false;
                };
                let Some(x) = stack.pop() else {
                    return false;
                };
                let r = if op_of(ins) == OP_ADD {
                    b.ins().iadd(x, y)
                } else {
                    b.ins().isub(x, y)
                };
                let r = trunc48(b, r);
                stack.push(r);
            }
            OP_PUSH_LOCAL_INT_OP => {
                let x = b.use_var(var(a));
                let k = b.ins().iconst(types::I64, bb as i64);
                let r = match cc {
                    0 => b.ins().iadd(x, k),
                    1 => b.ins().isub(x, k),
                    _ => return false,
                };
                let r = trunc48(b, r);
                stack.push(r);
            }
            OP_LOAD_CONST_LOCAL => {
                let k = match cc {
                    0 => bb as i64,
                    6 => match f.wide.get(bb) {
                        Some(&w) => w,
                        None => return false,
                    },
                    _ => return false,
                };
                let v = b.ins().iconst(types::I64, k);
                b.def_var(var(a), v);
            }
            OP_CALL_NAME => {
                let nargs = a_of(ins) as usize;
                let Some(name) = f.strings.get(bb) else {
                    return false;
                };
                let Some(&callee) = by_name.get(name.as_ref()) else {
                    return false;
                };
                let mut args = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    let Some(v) = stack.pop() else {
                        return false;
                    };
                    args.push(v);
                }
                args.reverse();
                let pad = b.ins().iconst(types::I64, 0);
                let mut call_args = [pad; 4];
                for (i, v) in args.iter().enumerate() {
                    call_args[i] = *v;
                }
                let call = b.ins().call(func_refs[callee], &call_args);
                let result = b.inst_results(call)[0];
                stack.push(result);
            }
            OP_IF_LT_INT | OP_IF_EQ_INT => {
                if ip + 1 >= f.code.len() {
                    return false;
                }
                let Some(&target_ip) = f.wide.get(cc) else {
                    return false;
                };
                let Some(target) = blocks.get(target_ip as usize).copied().flatten() else {
                    return false;
                };
                let fall = match blocks[ip + 1] {
                    Some(f) => f,
                    None => {
                        let f = b.create_block();
                        blocks[ip + 1] = Some(f);
                        f
                    }
                };
                let v = b.use_var(var(a));
                let limit = b.ins().iconst(types::I64, bb as i64);
                let cond = if op_of(ins) == OP_IF_LT_INT {
                    b.ins().icmp(IntCC::SignedLessThan, v, limit)
                } else {
                    b.ins().icmp(IntCC::Equal, v, limit)
                };
                b.ins().brif(cond, fall, &[], target, &[]);
                b.switch_to_block(fall);
                current = Some(fall);
                stack.clear();
            }
            OP_JUMP => {
                let Some(target) = blocks[a] else {
                    return false;
                };
                b.ins().jump(target, &[]);
                current = None;
                stack.clear();
            }
            OP_RETURN => {
                let Some(v) = stack.pop() else {
                    return false;
                };
                let v = trunc48(b, v);
                b.ins().return_(&[v]);
                current = None;
                stack.clear();
            }
            OP_HALT => {
                let z = b.ins().iconst(types::I64, 0);
                b.ins().return_(&[z]);
                current = None;
                stack.clear();
            }
            _ => return false,
        }
    }

    if current.is_some() {
        let z = b.ins().iconst(types::I64, 0);
        b.ins().return_(&[z]);
    }
    true
}
