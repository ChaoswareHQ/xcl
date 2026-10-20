//! `xcl-jit` — a Cranelift-backed JIT for XCL integer loops.
//!
//! # Scope
//!
//! The JIT compiles **one `while` loop** of a bytecode unit. A loop qualifies
//! when:
//!
//! * its header is `OP_WHILE_LT_INT` or `OP_WHILE_LT_INT_WIDE`;
//! * the body is a single basic block (the only branch is the back-edge
//!   `OP_JUMP`);
//! * every operation is integer-only (no floats, strings, arrays, maps or
//!   calls);
//! * the loop is entered with integer values in every local it reads before
//!   writing.
//!
//! Those conditions describe the numeric hot loops (`int_loop`, `lcg`,
//! `bitwise`, and similar) and nothing else. Everything outside the scope
//! stays on the interpreter; the JIT is an accelerator, never a replacement.
//!
//! # Safety model
//!
//! The generated function works directly on the VM register file: a `*mut u64`
//! of NaN-boxed `Value`s. Locals live in machine registers for the whole loop
//! and are written back **only** when the loop exits normally, so a bailout
//! (`return 1`) never leaves partially updated state. The caller then
//! re-executes the loop from its header in the interpreter.
//!
//! Entry guards check the tag of every local that the loop reads before it
//! writes; a non-integer value bails out. Arithmetic that the interpreter would
//! promote (i64 multiply overflow) or turn into a float (division by zero,
//! modulo by zero) also bails out, so integer-only execution never changes
//! observable behaviour.
//!
//! # ABI
//!
//! ```text
//! unsafe extern "C" fn(regs: *mut u64) -> u64
//!   0 => loop finished, resume the interpreter at `exit_ip`
//!   1 => bail out, resume at the loop header
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::mem;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, UserFuncName, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};

pub mod functions;

pub use functions::{FunctionFn, FunctionInput, JitFunction, ensure_functions};

// ── NaN-boxing layout (mirrors `xcl-vm/src/runtime/value.rs`) ──────────────
//
// `Value` is `u64`; integers are `TAG_INT | i48`. Decoding is a sign-extend
// from 48 bits, encoding is a mask plus the tag.
const TAG_INT: u64 = 0x7FFB_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const PAYLOAD: u64 = 0x0000_FFFF_FFFF_FFFF;

// ── Opcodes (mirrors `xcl-vm/src/runtime/vm/ops.rs`) ──────────────────────
const OP_PUSH_INT: u8 = 4;
const OP_LOAD_LOCAL: u8 = 8;
const OP_STORE_LOCAL: u8 = 9;
const OP_ADD: u8 = 12;
const OP_SUB: u8 = 13;
const OP_MUL: u8 = 14;
const OP_DIV: u8 = 15;
const OP_MOD: u8 = 16;
const OP_JUMP: u8 = 28;
const OP_LOAD_LOCAL_LOAD_LOCAL: u8 = 60;
const OP_ADD_LOCAL_LOCAL: u8 = 61;
const OP_SUB_LOCAL_LOCAL: u8 = 62;
const OP_LOAD_LOCAL_STORE_LOCAL: u8 = 65;
const OP_WHILE_LT_INT_WIDE: u8 = 69;
const OP_INC_LOCAL: u8 = 80;
const OP_DEC_LOCAL: u8 = 81;
const OP_MUL_LOCAL_LOCAL: u8 = 82;
const OP_DIV_LOCAL_LOCAL: u8 = 83;
const OP_ADD_INT_LOCAL: u8 = 91;
const OP_ADD_LOCAL_STORE: u8 = 92;
const OP_SUB_INT_LOCAL: u8 = 93;
const OP_MUL_INT_LOCAL: u8 = 94;
const OP_WHILE_LT_INT: u8 = 95;
const OP_MUL_LOCAL_STORE: u8 = 96;
const OP_DIV_LOCAL_STORE: u8 = 97;
const OP_SUB_LOCAL_STORE: u8 = 98;
const OP_ADD_LOCAL_LOCAL_STORE: u8 = 99;
const OP_MUL_LOCAL_LOCAL_STORE: u8 = 100;
const OP_SUB_LOCAL_LOCAL_STORE: u8 = 101;
const OP_DIV_LOCAL_LOCAL_STORE: u8 = 102;
const OP_MOD_LOCAL_LOCAL_STORE: u8 = 107;
const OP_LOAD_CONST_LOCAL: u8 = 108;
const OP_GET_INDEX_LOCAL_LOCAL_STORE: u8 = 105;
const OP_GET_FIELD_LOCAL_STORE: u8 = 106;
const OP_ADD_FIELD_LOCAL_STORE: u8 = 124;
const OP_SHL: u8 = 115;
const OP_SHR: u8 = 116;
const OP_BITAND: u8 = 117;
const OP_BITOR: u8 = 119;
const OP_BITXOR: u8 = 120;

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

/// The compiled entry point. See the module docs for the return protocol.
///
/// The second argument is the loop's [`LoopCache`]; loops without memory
/// accesses ignore it.
pub type LoopFn = unsafe extern "C" fn(*mut u64, *mut LoopCache) -> u64;

/// Maximum memory-access sites tracked per loop.
pub const JIT_MAX_ACCESSES: usize = 16;

/// Kind of memory access compiled into a loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    /// `locals[dest] = locals[object][locals[index]]`, array case only.
    ArrayIndex,
    /// `locals[dest] = locals[object].field`, with the field resolved at entry.
    MapField,
}

/// One memory-access site in a loop body, in instruction order.
///
/// The VM uses this to fill the [`LoopCache`] before entering native code.
#[derive(Clone, Copy, Debug)]
pub struct AccessPlan {
    /// Kind of access.
    pub kind: AccessKind,
    /// Local holding the container object.
    pub object_slot: u32,
    /// Local holding the index (arrays).
    pub index_slot: u32,
    /// String-table index of the field name (maps).
    pub key_slot: u32,
}

/// Runtime cache for one access site.
///
/// `object` is the raw NaN-boxed local value observed when the loop was
/// entered. The native code compares it against the live local; a mismatch
/// bails to the interpreter, so a pointer can never outlive its object.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AccessCache {
    /// Raw `Value` of the container (identity guard).
    pub object: u64,
    /// Array element buffer.
    pub data: *const u64,
    /// Number of elements.
    pub len: usize,
    /// Cached scalar value (maps, later).
    pub value: u64,
}

impl Default for AccessCache {
    fn default() -> Self {
        Self {
            object: 0,
            data: core::ptr::null(),
            len: 0,
            value: 0,
        }
    }
}

/// Per-loop cache handed to the native function.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LoopCache {
    /// One slot per access site, in [`AccessPlan`] order.
    pub accesses: [AccessCache; JIT_MAX_ACCESSES],
}

impl Default for LoopCache {
    fn default() -> Self {
        Self {
            accesses: [AccessCache::default(); JIT_MAX_ACCESSES],
        }
    }
}

/// A compilable loop inside a bytecode unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopSpec {
    /// Instruction index of the `while` header.
    pub start: usize,
    /// Instruction index of the back-edge `OP_JUMP`.
    pub end: usize,
    /// Interpreter instruction to resume at when the loop exits normally.
    pub exit_ip: usize,
    /// Local slot tested by the header.
    pub slot: u32,
    /// Loop limit.
    pub limit: i64,
}

/// A loop compiled to native code, kept alive by the process-wide cache.
#[derive(Clone)]
pub struct CompiledLoop {
    /// Native entry point.
    pub func: LoopFn,
    /// Resume address for a normal exit.
    pub exit_ip: usize,
    /// Memory-access sites in instruction order.
    pub accesses: std::sync::Arc<[AccessPlan]>,
}

// ── Cache ─────────────────────────────────────────────────────────────────
//
// Compilation costs ~0.2-1 ms, which is significant for short scripts and for
// criterion's per-iteration setup. The cache is keyed by a hash of the loop's
// instructions and constants, so a re-used bytecode compiles exactly once per
// process (per thread: `JITModule` is not `Send`).

struct CacheEntry {
    _module: JITModule,
    compiled: CompiledLoop,
}

thread_local! {
    static CACHE: RefCell<HashMap<u64, CacheEntry>> = RefCell::new(HashMap::new());
}

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

// ── Discovery ─────────────────────────────────────────────────────────────

fn is_branch(op: u8) -> bool {
    matches!(
        op,
        OP_JUMP | 29 | 30 | 32 | 42 | 43 | 44 | 45 | 90 | 123 | 67 | 68
    )
}

fn supported_body_op(op: u8) -> bool {
    matches!(
        op,
        OP_PUSH_INT
            | OP_LOAD_LOCAL
            | OP_STORE_LOCAL
            | OP_ADD
            | OP_SUB
            | OP_MUL
            | OP_DIV
            | OP_MOD
            | OP_LOAD_LOCAL_LOAD_LOCAL
            | OP_ADD_LOCAL_LOCAL
            | OP_SUB_LOCAL_LOCAL
            | OP_MUL_LOCAL_LOCAL
            | OP_DIV_LOCAL_LOCAL
            | OP_LOAD_LOCAL_STORE_LOCAL
            | OP_INC_LOCAL
            | OP_DEC_LOCAL
            | OP_ADD_INT_LOCAL
            | OP_SUB_INT_LOCAL
            | OP_MUL_INT_LOCAL
            | OP_ADD_LOCAL_STORE
            | OP_SUB_LOCAL_STORE
            | OP_MUL_LOCAL_STORE
            | OP_DIV_LOCAL_STORE
            | OP_ADD_LOCAL_LOCAL_STORE
            | OP_SUB_LOCAL_LOCAL_STORE
            | OP_MUL_LOCAL_LOCAL_STORE
            | OP_DIV_LOCAL_LOCAL_STORE
            | OP_MOD_LOCAL_LOCAL_STORE
            | OP_GET_INDEX_LOCAL_LOCAL_STORE
            | OP_GET_FIELD_LOCAL_STORE
            | OP_ADD_FIELD_LOCAL_STORE
            | OP_LOAD_CONST_LOCAL
            | OP_SHL
            | OP_SHR
            | OP_BITAND
            | OP_BITOR
            | OP_BITXOR
    )
}

/// Finds the first compilable `while` loop in `code`.
///
/// `wide` carries the wide operand table. Returns `None` when the unit has no
/// loop, or when the first loop found is outside the supported subset (the
/// caller must not fall back to a later loop: keeping the decision
/// deterministic matters more than coverage).
pub fn find_loop(code: &[u64], wide: &[i64]) -> Option<LoopSpec> {
    for h in 0..code.len() {
        let op = op_of(code[h]);
        if op == OP_WHILE_LT_INT || op == OP_WHILE_LT_INT_WIDE {
            return loop_at(code, wide, h);
        }
    }
    None
}

/// Finds every compilable `while` loop in `code`, in instruction order.
///
/// Unlike [`find_loop`], an unsupported loop does not stop the search: later
/// loops (including loops nested inside an unsupported outer loop) are still
/// compiled. Each returned loop runs on its own; the interpreter handles the
/// surrounding control flow.
pub fn find_loops(code: &[u64], wide: &[i64]) -> Vec<LoopSpec> {
    let mut out = Vec::new();
    for h in 0..code.len() {
        let op = op_of(code[h]);
        if (op == OP_WHILE_LT_INT || op == OP_WHILE_LT_INT_WIDE)
            && let Some(spec) = loop_at(code, wide, h)
        {
            out.push(spec);
        }
    }
    out
}

/// Validates the loop whose header is at instruction `h`.
fn loop_at(code: &[u64], wide: &[i64], h: usize) -> Option<LoopSpec> {
    let op = op_of(code[h]);
    if op != OP_WHILE_LT_INT && op != OP_WHILE_LT_INT_WIDE {
        return None;
    }
    let (slot, limit, exit_wide) = if op == OP_WHILE_LT_INT {
        (a_of(code[h]), b_of(code[h]) as i64, c_of(code[h]) as usize)
    } else {
        (
            a_of(code[h]),
            *wide.get(b_of(code[h]) as usize)?,
            c_of(code[h]) as usize,
        )
    };
    let exit_ip = *wide.get(exit_wide)? as usize;
    if exit_ip <= h {
        return None;
    }
    // Find the back edge and validate the body.
    let mut compilable = false;
    let mut j = h + 1;
    while j < code.len() {
        let o = op_of(code[j]);
        if o == OP_JUMP {
            if a_of(code[j]) as usize == h {
                compilable = true;
            }
            break;
        }
        if is_branch(o) {
            break;
        }
        if o == OP_LOAD_CONST_LOCAL && c_of(code[j]) == 1 {
            // Float constant: outside the integer subset.
            break;
        }
        if !supported_body_op(o) {
            break;
        }
        j += 1;
    }
    if !compilable {
        return None;
    }
    for &ins in &code[h + 1..j] {
        if !supported_body_op(op_of(ins)) || (op_of(ins) == OP_LOAD_CONST_LOCAL && c_of(ins) == 1) {
            return None;
        }
    }

    // Array accesses need a stable container: reject a loop that writes the
    // array local, or that uses an object local as an integer.
    let mut written = BTreeSet::new();
    for &ins in &code[h + 1..j] {
        let (_, defs) = access_sets(ins, &written);
        for d in defs {
            written.insert(d);
        }
    }
    let mut object_slots = BTreeSet::new();
    let mut access_count = 0usize;
    for &ins in &code[h + 1..j] {
        let op = op_of(ins);
        if matches!(
            op,
            OP_GET_INDEX_LOCAL_LOCAL_STORE | OP_GET_FIELD_LOCAL_STORE | OP_ADD_FIELD_LOCAL_STORE
        ) {
            access_count += 1;
            let obj = c_of(ins) as usize;
            if written.contains(&obj) {
                return None;
            }
            object_slots.insert(obj);
        }
    }
    if access_count > JIT_MAX_ACCESSES || object_slots.contains(&(slot as usize)) {
        return None;
    }
    for &ins in &code[h + 1..j] {
        let op = op_of(ins);
        if matches!(
            op,
            OP_GET_INDEX_LOCAL_LOCAL_STORE | OP_GET_FIELD_LOCAL_STORE | OP_ADD_FIELD_LOCAL_STORE
        ) {
            continue;
        }
        let (reads, _) = access_sets(ins, &written);
        if reads.iter().any(|r| object_slots.contains(r)) {
            return None;
        }
    }

    Some(LoopSpec {
        start: h,
        end: j,
        exit_ip,
        slot,
        limit,
    })
}

// ── Compilation ───────────────────────────────────────────────────────────

pub(crate) fn make_module() -> Result<JITModule, String> {
    let mut flags = settings::builder();
    flags.set("is_pic", "false").map_err(|e| e.to_string())?;
    flags.set("opt_level", "speed").map_err(|e| e.to_string())?;
    flags
        .set("unwind_info", "false")
        .map_err(|e| e.to_string())?;
    let isa_builder = cranelift_native::builder().map_err(|e| e.to_string())?;
    let isa = isa_builder
        .finish(settings::Flags::new(flags))
        .map_err(|e| e.to_string())?;
    Ok(JITModule::new(JITBuilder::with_isa(
        isa,
        default_libcall_names(),
    )))
}

fn trunc48(b: &mut FunctionBuilder, v: Value) -> Value {
    let lo = b.ins().ishl_imm_s(v, 16);
    b.ins().sshr_imm_s(lo, 16)
}

/// Emits `return 1` (bail out without committing any local).
fn ret1(b: &mut FunctionBuilder) {
    let one = b.ins().iconst(types::I64, 1);
    b.ins().return_(&[one]);
}

/// Collects the memory-access sites of a loop body, in instruction order.
fn collect_accesses(code: &[u64], spec: &LoopSpec) -> Vec<AccessPlan> {
    let mut out = Vec::new();
    for &ins in &code[spec.start + 1..spec.end] {
        match op_of(ins) {
            OP_GET_INDEX_LOCAL_LOCAL_STORE => out.push(AccessPlan {
                kind: AccessKind::ArrayIndex,
                object_slot: c_of(ins) as u32,
                index_slot: b_of(ins) as u32,
                key_slot: 0,
            }),
            OP_GET_FIELD_LOCAL_STORE | OP_ADD_FIELD_LOCAL_STORE => out.push(AccessPlan {
                kind: AccessKind::MapField,
                object_slot: c_of(ins) as u32,
                index_slot: 0,
                key_slot: b_of(ins) as u32,
            }),
            _ => {}
        }
    }
    out
}

/// Entry-block values for one access site.
#[derive(Clone, Copy)]
enum AccessHandle {
    /// Array element buffer and length.
    Array { data: Value, len: Value },
    /// Decoded integer field value (maps).
    Map { value: Value },
}

/// Validates and compiles `spec`. Returns the compiled loop and keeps the
/// `JITModule` alive inside `entry_module`.
fn compile_loop(
    code: &[u64],
    wide: &[i64],
    spec: &LoopSpec,
    nlocals: usize,
) -> Result<(JITModule, CompiledLoop), String> {
    let mut module = make_module()?;

    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(types::I64));
    sig.params.push(AbiParam::new(types::I64));
    sig.returns.push(AbiParam::new(types::I64));

    let mut ctx = module.make_context();
    ctx.func.signature = sig.clone();
    ctx.func.name = UserFuncName::user(0, 0);

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);

    let entry = b.create_block();
    let header = b.create_block();
    let body = b.create_block();
    let exit = b.create_block();
    let bail = b.create_block();

    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    b.seal_block(entry);
    let regs = b.block_params(entry)[0];
    let cache = b.block_params(entry)[1];

    let vars: Vec<Variable> = (0..nlocals).map(|_| b.declare_var(types::I64)).collect();
    let var = |slot: usize| vars[slot];

    // Access plans are collected before the entry guard so object locals can
    // be excluded from the integer guard.
    let accesses = collect_accesses(code, spec);
    let object_slots: BTreeSet<usize> = accesses.iter().map(|a| a.object_slot as usize).collect();

    // Determine which locals the body reads before writing (they need an entry
    // guard) and which slots it writes (they need write-back).
    let mut written: BTreeSet<usize> = BTreeSet::new();
    let mut guards: BTreeSet<usize> = BTreeSet::new();
    guards.insert(spec.slot as usize);
    let mut dirty: BTreeSet<usize> = BTreeSet::new();
    for &ins in &code[spec.start + 1..spec.end] {
        let (reads, defs) = access_sets(ins, &written);
        for r in reads {
            if !written.contains(&r) && !object_slots.contains(&r) {
                guards.insert(r);
            }
        }
        for d in defs {
            written.insert(d);
            dirty.insert(d);
        }
    }

    // A counter that is only incremented once per iteration is bounded by the
    // header test (`counter < limit`), so its increment cannot leave i48 and a
    // non-negative entry value stays non-negative for the whole loop.
    let counter = if spec.limit <= MASK_POSITIVE_I48 {
        let mut defs = 0usize;
        let mut only_inc = false;
        for &ins in &code[spec.start + 1..spec.end] {
            let (_, d) = access_sets(ins, &written);
            if d.contains(&(spec.slot as usize)) {
                defs += 1;
                only_inc = op_of(ins) == OP_INC_LOCAL && a_of(ins) as usize == spec.slot as usize;
            }
        }
        if defs == 1 && only_inc {
            Some(spec.slot as usize)
        } else {
            None
        }
    } else {
        None
    };

    // Entry: guard and decode every local.
    let mut ok = b.ins().iconst(types::I8, 1);
    for slot in 0..nlocals {
        let off = (slot * 8) as i32;
        let raw = b.ins().load(types::I64, MemFlagsData::trusted(), regs, off);
        if guards.contains(&slot) {
            let tag = b.ins().band_imm_s(raw, TAG_MASK as i64);
            let is_int = b.ins().icmp_imm_s(IntCC::Equal, tag, TAG_INT as i64);
            ok = b.ins().band(ok, is_int);
        }
        let lo = b.ins().ishl_imm_s(raw, 16);
        let dec = b.ins().sshr_imm_s(lo, 16);
        b.def_var(var(slot), dec);
    }
    let mut access_handles = Vec::with_capacity(accesses.len());
    for (site, plan) in accesses.iter().enumerate() {
        // The container local is never written in the loop, so its identity and
        // the data pointer/length are loop-invariant: check and load them once.
        let obj_raw = b.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            regs,
            (plan.object_slot as usize * 8) as i32,
        );
        let expected = b.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            cache,
            (site * 32) as i32,
        );
        let same = b.ins().icmp(IntCC::Equal, obj_raw, expected);
        ok = b.ins().band(ok, same);
        match plan.kind {
            AccessKind::ArrayIndex => {
                let data = b.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    cache,
                    (site * 32 + 8) as i32,
                );
                let len = b.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    cache,
                    (site * 32 + 16) as i32,
                );
                access_handles.push(AccessHandle::Array { data, len });
            }
            AccessKind::MapField => {
                let raw = b.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    cache,
                    (site * 32 + 24) as i32,
                );
                let tag = b.ins().band_imm_s(raw, TAG_MASK as i64);
                let is_int = b.ins().icmp_imm_s(IntCC::Equal, tag, TAG_INT as i64);
                ok = b.ins().band(ok, is_int);
                let lo = b.ins().ishl_imm_s(raw, 16);
                let dec = b.ins().sshr_imm_s(lo, 16);
                access_handles.push(AccessHandle::Map { value: dec });
            }
        }
    }
    if let Some(c) = counter
        && accesses.iter().any(|a| a.index_slot as usize == c)
    {
        let c0 = b.use_var(var(c));
        let zero = b.ins().iconst(types::I64, 0);
        let nonneg = b.ins().icmp(IntCC::SignedGreaterThanOrEqual, c0, zero);
        ok = b.ins().band(ok, nonneg);
    }
    b.ins().brif(ok, header, &[], bail, &[]);

    // Header: while counter < limit.
    b.switch_to_block(header);
    let counter_value = b.use_var(var(spec.slot as usize));
    let limit = b.ins().iconst(types::I64, spec.limit);
    let cond = b.ins().icmp(IntCC::SignedLessThan, counter_value, limit);
    b.ins().brif(cond, body, &[], exit, &[]);

    // Body.
    b.switch_to_block(body);
    let mut stack: Vec<JitVal> = Vec::new();
    let mut failed = false;
    let mut access_idx = 0usize;
    for &ins in &code[spec.start + 1..spec.end] {
        if !emit(
            &mut b,
            ins,
            wide,
            var,
            &mut stack,
            bail,
            counter,
            &access_handles,
            &mut access_idx,
        ) {
            failed = true;
            break;
        }
    }
    if failed || !stack.is_empty() {
        // A candidate that passed `find_loop` must compile; treat a mismatch
        // as a hard "not compilable" rather than emitting wrong code.
        ret1(&mut b);
    } else {
        // Loop-carried variables must be defined on the back edge.
        for slot in 0..nlocals {
            let v = b.use_var(var(slot));
            b.def_var(var(slot), v);
        }
        b.ins().jump(header, &[]);
    }

    // Normal exit: write back only the locals the loop actually wrote.
    b.switch_to_block(exit);
    for &slot in &dirty {
        let v = b.use_var(var(slot));
        let masked = b.ins().band_imm_s(v, PAYLOAD as i64);
        let boxed = b.ins().bor_imm_s(masked, TAG_INT as i64);
        b.ins()
            .store(MemFlagsData::trusted(), boxed, regs, (slot * 8) as i32);
    }
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().return_(&[zero]);

    // Bail-out.
    b.switch_to_block(bail);
    let one = b.ins().iconst(types::I64, 1);
    b.ins().return_(&[one]);

    b.seal_all_blocks();
    b.finalize(module.target_config());

    let id = module
        .declare_function("xcl_loop", Linkage::Local, &sig)
        .map_err(|e| e.to_string())?;
    module
        .define_function(id, &mut ctx)
        .map_err(|e| e.to_string())?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(|e| e.to_string())?;
    let ptr = module.get_finalized_function(id);
    // SAFETY: the declared signature is (i64) -> i64 and the function is kept
    // alive by the module stored in the cache entry.
    let func: LoopFn = unsafe { mem::transmute(ptr) };
    Ok((
        module,
        CompiledLoop {
            func,
            exit_ip: spec.exit_ip,
            accesses: accesses.into(),
        },
    ))
}

/// Local slots read and written by one instruction.
///
/// `written` is the set of slots already defined in this iteration; it is only
/// used to decide whether a read is a loop-carried value. The returned `reads`
/// must include every slot the instruction actually reads.
fn access_sets(i: u64, _written: &BTreeSet<usize>) -> (Vec<usize>, Vec<usize>) {
    let a = a_of(i) as usize;
    let bb = b_of(i) as usize;
    let c = c_of(i) as usize;
    match op_of(i) {
        OP_PUSH_INT | OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_MOD | OP_SHL | OP_SHR | OP_BITAND
        | OP_BITOR | OP_BITXOR | OP_JUMP => (Vec::new(), Vec::new()),
        OP_LOAD_LOCAL => (vec![a], Vec::new()),
        OP_STORE_LOCAL => (Vec::new(), vec![a]),
        OP_LOAD_LOCAL_LOAD_LOCAL => (vec![a, bb], Vec::new()),
        OP_LOAD_LOCAL_STORE_LOCAL => (vec![a], vec![bb]),
        OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL | OP_DIV_LOCAL_LOCAL => {
            (vec![a, bb], Vec::new())
        }
        OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE
        | OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL => (vec![a, bb], vec![a]),
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE => (vec![bb, c], vec![a]),
        OP_GET_INDEX_LOCAL_LOCAL_STORE => (vec![bb, c], vec![a]),
        OP_GET_FIELD_LOCAL_STORE => (vec![c], vec![a]),
        OP_ADD_FIELD_LOCAL_STORE => (vec![a, c], vec![a]),
        OP_INC_LOCAL | OP_DEC_LOCAL => (vec![a], vec![a]),
        OP_LOAD_CONST_LOCAL => (Vec::new(), vec![a]),
        _ => (Vec::new(), Vec::new()),
    }
}

/// A value on the JIT's virtual stack.
///
/// `wide` means the 48-bit truncation the interpreter applies after arithmetic
/// has **not** been emitted yet. It is deferred so a following mask can clear
/// the high bits for free: `trunc48(x) & mask == x & mask` for a mask below
/// `2^47`. Any other consumer calls [`narrow`] first.
#[derive(Clone, Copy)]
struct JitVal {
    v: Value,
    wide: bool,
    /// Exact integer when this value is a literal; used to recognise masks.
    konst: Option<i64>,
}

impl JitVal {
    /// A value that already has the interpreter's 48-bit representation.
    fn narrow(v: Value) -> Self {
        Self {
            v,
            wide: false,
            konst: None,
        }
    }

    /// A literal constant, which is both narrow and known exactly.
    fn literal(v: Value, konst: i64) -> Self {
        Self {
            v,
            wide: false,
            konst: Some(konst),
        }
    }
}

/// Largest mask that keeps a value in the non-negative i48 range.
const MASK_POSITIVE_I48: i64 = (1i64 << 47) - 1;

/// Applies the deferred 48-bit truncation, if any.
fn narrow(b: &mut FunctionBuilder, x: JitVal) -> Value {
    if x.wide { trunc48(b, x.v) } else { x.v }
}

/// Emits one body instruction. Returns `false` if the opcode is unsupported
/// (which would be a bug: `find_loop` already filtered the body).
#[allow(clippy::too_many_arguments)]
fn emit(
    b: &mut FunctionBuilder,
    i: u64,
    wide: &[i64],
    var: impl Fn(usize) -> Variable,
    stack: &mut Vec<JitVal>,
    _bail: cranelift_codegen::ir::Block,
    counter: Option<usize>,
    access_handles: &[AccessHandle],
    access_idx: &mut usize,
) -> bool {
    let a = a_of(i) as usize;
    let bb = b_of(i) as usize;
    let c = c_of(i) as usize;
    let op = op_of(i);
    let pop = |stack: &mut Vec<JitVal>| stack.pop().expect("jit stack underflow");
    match op {
        OP_PUSH_INT => {
            let k = a_of(i) as i64;
            let v = b.ins().iconst(types::I64, k);
            stack.push(JitVal::literal(v, k));
        }
        OP_LOAD_LOCAL => {
            let v = b.use_var(var(a));
            stack.push(JitVal::narrow(v));
        }
        OP_STORE_LOCAL => {
            let v = pop(stack);
            let v = narrow(b, v);
            b.def_var(var(a), v);
        }
        OP_LOAD_LOCAL_LOAD_LOCAL => {
            let x = b.use_var(var(a));
            let y = b.use_var(var(bb));
            stack.push(JitVal::narrow(x));
            stack.push(JitVal::narrow(y));
        }
        OP_LOAD_LOCAL_STORE_LOCAL => {
            let v = b.use_var(var(a));
            b.def_var(var(bb), v);
        }
        OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_MOD | OP_SHL | OP_SHR | OP_BITAND | OP_BITOR
        | OP_BITXOR => {
            let rhs = pop(stack);
            let lhs = pop(stack);
            if let Some(v) = arith(b, op, lhs, rhs) {
                stack.push(v);
            } else {
                // Division/modulo needs the zero check; the caller re-runs the
                // loop in the interpreter, so a hard bail is correct.
                ret1(b);
            }
        }
        OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL | OP_DIV_LOCAL_LOCAL => {
            let x = JitVal::narrow(b.use_var(var(a)));
            let y = JitVal::narrow(b.use_var(var(bb)));
            let bin = match op {
                OP_ADD_LOCAL_LOCAL => OP_ADD,
                OP_SUB_LOCAL_LOCAL => OP_SUB,
                OP_MUL_LOCAL_LOCAL => OP_MUL,
                _ => OP_DIV,
            };
            if let Some(v) = arith(b, bin, x, y) {
                stack.push(v);
            } else {
                ret1(b);
            }
        }
        OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE => {
            let x = JitVal::narrow(b.use_var(var(a)));
            let y = JitVal::narrow(b.use_var(var(bb)));
            let bin = match op {
                OP_ADD_LOCAL_STORE => OP_ADD,
                OP_SUB_LOCAL_STORE => OP_SUB,
                OP_MUL_LOCAL_STORE => OP_MUL,
                _ => OP_DIV,
            };
            if let Some(v) = arith(b, bin, x, y) {
                let v = narrow(b, v);
                b.def_var(var(a), v);
            } else {
                ret1(b);
            }
        }
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE => {
            let x = JitVal::narrow(b.use_var(var(c)));
            let y = JitVal::narrow(b.use_var(var(bb)));
            let bin = match op {
                OP_ADD_LOCAL_LOCAL_STORE => OP_ADD,
                OP_SUB_LOCAL_LOCAL_STORE => OP_SUB,
                OP_MUL_LOCAL_LOCAL_STORE => OP_MUL,
                OP_DIV_LOCAL_LOCAL_STORE => OP_DIV,
                _ => OP_MOD,
            };
            if let Some(v) = arith(b, bin, x, y) {
                let v = narrow(b, v);
                b.def_var(var(a), v);
            } else {
                ret1(b);
            }
        }
        OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL => {
            let x = JitVal::narrow(b.use_var(var(a)));
            let k = bb as i64;
            let y = JitVal::literal(b.ins().iconst(types::I64, k), k);
            let bin = match op {
                OP_ADD_INT_LOCAL => OP_ADD,
                OP_SUB_INT_LOCAL => OP_SUB,
                _ => OP_MUL,
            };
            if let Some(v) = arith(b, bin, x, y) {
                let v = narrow(b, v);
                b.def_var(var(a), v);
            } else {
                ret1(b);
            }
        }
        OP_INC_LOCAL | OP_DEC_LOCAL => {
            let x = b.use_var(var(a));
            let one = b.ins().iconst(types::I64, 1);
            let v = if op == OP_INC_LOCAL {
                b.ins().iadd(x, one)
            } else {
                b.ins().isub(x, one)
            };
            // The header guarantees `x < limit`; a single increment therefore
            // stays inside i48 and needs no sign-extension.
            let v = if op == OP_INC_LOCAL && counter == Some(a) {
                v
            } else {
                trunc48(b, v)
            };
            b.def_var(var(a), v);
        }
        OP_LOAD_CONST_LOCAL => {
            let kind = c_of(i);
            let k = match kind {
                0 => bb as i64,
                6 => match wide.get(bb) {
                    Some(&w) => w,
                    None => return false,
                },
                _ => return false,
            };
            let v = b.ins().iconst(types::I64, k);
            b.def_var(var(a), v);
        }
        OP_GET_INDEX_LOCAL_LOCAL_STORE => {
            let dest = a;
            let site = *access_idx;
            *access_idx += 1;
            let Some(AccessHandle::Array { data, len }) = access_handles.get(site).copied() else {
                ret1(b);
                return true;
            };
            let idx_slot = b_of(i) as usize;

            // Normalise a negative index, then bounds-check it. When the index
            // is the loop counter it is known non-negative from the entry guard.
            let idx = b.use_var(var(idx_slot));
            let norm = if counter == Some(idx_slot) {
                idx
            } else {
                let zero = b.ins().iconst(types::I64, 0);
                let negative = b.ins().icmp(IntCC::SignedLessThan, idx, zero);
                let adjusted = b.ins().iadd(idx, len);
                b.ins().select(negative, adjusted, idx)
            };
            let zero = b.ins().iconst(types::I64, 0);
            let non_negative = b.ins().icmp(IntCC::SignedGreaterThanOrEqual, norm, zero);
            let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, norm, len);
            let ok = b.ins().band(non_negative, in_bounds);
            let ok_block = b.create_block();
            let oob = b.create_block();
            b.ins().brif(ok, ok_block, &[], oob, &[]);
            b.switch_to_block(oob);
            ret1(b);
            b.switch_to_block(ok_block);

            // The cache fill guaranteed every element is an integer.
            let offset = b.ins().ishl_imm_s(norm, 3);
            let addr = b.ins().iadd(data, offset);
            let elem = b.ins().load(types::I64, MemFlagsData::trusted(), addr, 0);
            let lo = b.ins().ishl_imm_s(elem, 16);
            let decoded = b.ins().sshr_imm_s(lo, 16);
            b.def_var(var(dest), decoded);
        }
        OP_GET_FIELD_LOCAL_STORE => {
            let dest = a;
            let site = *access_idx;
            *access_idx += 1;
            let Some(AccessHandle::Map { value }) = access_handles.get(site).copied() else {
                ret1(b);
                return true;
            };
            b.def_var(var(dest), value);
        }
        OP_ADD_FIELD_LOCAL_STORE => {
            let dest = a;
            let site = *access_idx;
            *access_idx += 1;
            let Some(AccessHandle::Map { value }) = access_handles.get(site).copied() else {
                ret1(b);
                return true;
            };
            let acc = JitVal::narrow(b.use_var(var(dest)));
            let addend = JitVal::narrow(value);
            if let Some(v) = arith(b, OP_ADD, acc, addend) {
                let v = narrow(b, v);
                b.def_var(var(dest), v);
            } else {
                ret1(b);
            }
        }
        OP_JUMP => {}
        _ => return false,
    }
    true
}

/// Emits an integer binary operation with the interpreter's exact semantics.
///
/// The 48-bit truncation is deferred (see [`JitVal`]): bitwise operations do
/// not need their operands truncated, and a constant mask below `2^47` clears
/// the deferred flag outright.
///
/// Returns `None` for a division/modulo by zero (the interpreter would produce
/// a float; the caller bails to it).
fn arith(b: &mut FunctionBuilder, op: u8, x: JitVal, y: JitVal) -> Option<JitVal> {
    match op {
        OP_ADD => {
            let xv = narrow(b, x);
            let yv = narrow(b, y);
            let r = b.ins().iadd(xv, yv);
            Some(JitVal {
                v: r,
                wide: true,
                konst: None,
            })
        }
        OP_SUB => {
            let xv = narrow(b, x);
            let yv = narrow(b, y);
            let r = b.ins().isub(xv, yv);
            Some(JitVal {
                v: r,
                wide: true,
                konst: None,
            })
        }
        OP_MUL => {
            let xv = narrow(b, x);
            let yv = narrow(b, y);
            let (r, overflow) = b.ins().smul_overflow(xv, yv);
            // On i64 overflow the interpreter promotes to float; bail instead.
            let bail = b.create_block();
            let cont = b.create_block();
            b.ins().brif(overflow, bail, &[], cont, &[]);
            b.switch_to_block(bail);
            ret1(b);
            b.switch_to_block(cont);
            Some(JitVal {
                v: r,
                wide: true,
                konst: None,
            })
        }
        OP_DIV | OP_MOD => {
            let xv = narrow(b, x);
            let yv = narrow(b, y);
            // Zero divisor -> interpreter produces inf/NaN. Bail before the
            // native division, which would trap.
            let is_zero = b.ins().icmp_imm_s(IntCC::Equal, yv, 0);
            let bail = b.create_block();
            let cont = b.create_block();
            b.ins().brif(is_zero, bail, &[], cont, &[]);
            b.switch_to_block(bail);
            ret1(b);
            b.switch_to_block(cont);
            let r = if op == OP_DIV {
                b.ins().sdiv(xv, yv)
            } else {
                b.ins().srem(xv, yv)
            };
            // `i48::MIN / -1` is the one division result that does not fit in
            // 48 signed bits; keep it deferred so a store truncates it.
            Some(JitVal {
                v: r,
                wide: op == OP_DIV,
                konst: None,
            })
        }
        OP_SHL | OP_SHR => {
            let xv = narrow(b, x);
            let yv = narrow(b, y);
            // `shift >= 64` yields 0 in the interpreter.
            let big = b
                .ins()
                .icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, yv, 64);
            let amt = b.ins().band_imm_s(yv, 63);
            let shifted = if op == OP_SHL {
                b.ins().ishl(xv, amt)
            } else {
                b.ins().sshr(xv, amt)
            };
            let zero = b.ins().iconst(types::I64, 0);
            let r = b.ins().select(big, zero, shifted);
            Some(JitVal {
                v: r,
                // A right shift of a 48-bit value stays inside 48 bits.
                wide: op == OP_SHL,
                konst: None,
            })
        }
        OP_BITAND => {
            let mask = x.konst.or(y.konst);
            let r = b.ins().band(x.v, y.v);
            let wide = match mask {
                // A non-negative mask below 2^47 makes the result positive and
                // therefore already a valid i48: no truncation is needed.
                Some(m) if (0..=MASK_POSITIVE_I48).contains(&m) => false,
                // A mask with bit 47 set can produce a value that is not the
                // sign extension of its low 48 bits; keep it deferred.
                Some(_) => true,
                // Two sign-extended i48 values AND to a sign-extended i48 value.
                None => x.wide || y.wide,
            };
            Some(JitVal {
                v: r,
                wide,
                konst: None,
            })
        }
        OP_BITOR | OP_BITXOR => {
            let r = if op == OP_BITOR {
                b.ins().bor(x.v, y.v)
            } else {
                b.ins().bxor(x.v, y.v)
            };
            Some(JitVal {
                v: r,
                wide: x.wide || y.wide,
                konst: None,
            })
        }
        _ => None,
    }
}

/// Compiles (or returns the cached compilation of) `spec`.
pub fn ensure_compiled(
    code: &[u64],
    wide: &[i64],
    spec: &LoopSpec,
    nlocals: usize,
) -> Option<CompiledLoop> {
    if spec.start >= spec.end || spec.end >= code.len() || nlocals == 0 {
        return None;
    }
    let mut key = 0xCBF2_9CE4_8422_2325u64;
    key = fnv(key, &(nlocals as u64).to_le_bytes());
    for &i in &code[spec.start..=spec.end] {
        key = fnv(key, &i.to_le_bytes());
    }
    key = fnv(key, &spec.limit.to_le_bytes());
    key = fnv(key, &spec.exit_ip.to_le_bytes());

    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(e) = cache.get(&key) {
            return Some(e.compiled.clone());
        }
        match compile_loop(code, wide, spec, nlocals) {
            Ok((module, compiled)) => {
                cache.insert(
                    key,
                    CacheEntry {
                        _module: module,
                        compiled: compiled.clone(),
                    },
                );
                Some(compiled)
            }
            Err(_) => None,
        }
    })
}

/// Runs a compiled loop. The caller guarantees `regs` points at a register file
/// with at least the number of locals the loop was compiled for.
///
/// # Safety
/// `compiled.func` writes to `regs` (see [`LoopSpec`]); the pointer must be
/// valid and exclusively borrowed for the duration of the call for the slots
/// the loop was compiled against.
pub unsafe fn run(compiled: &CompiledLoop, regs: *mut u64, cache: *mut LoopCache) -> u64 {
    // SAFETY: the caller upholds the pointer and aliasing requirements.
    unsafe { (compiled.func)(regs, cache) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instr(op: u8, a: u32, b: u16, c: u8) -> u64 {
        (op as u64) | ((a as u64) << 8) | ((b as u64) << 40) | ((c as u64) << 56)
    }

    fn boxed_int(v: i64) -> u64 {
        TAG_INT | ((v as u64) & PAYLOAD)
    }

    fn unbox(v: u64) -> i64 {
        (v as i64) << 16 >> 16
    }

    #[test]
    fn compiles_and_runs_a_counting_loop() {
        // $i = 0; $s = 0
        // while $i < 1000 { $s = $s + $i; $i = $i + 1 }
        // locals: 0 = i, 1 = s
        let code = vec![
            instr(OP_LOAD_CONST_LOCAL, 0, 0, 0),
            instr(OP_LOAD_CONST_LOCAL, 1, 0, 0),
            instr(OP_WHILE_LT_INT, 0, 1000, 0),
            instr(OP_ADD_LOCAL_STORE, 1, 0, 0),
            instr(OP_INC_LOCAL, 0, 0, 0),
            instr(OP_JUMP, 2, 0, 0),
        ];
        let wide = vec![7];
        let spec = find_loop(&code, &wide).expect("loop");
        assert_eq!(spec.start, 2);
        assert_eq!(spec.end, 5);
        assert_eq!(spec.exit_ip, 7);

        let compiled = ensure_compiled(&code, &wide, &spec, 2).expect("compile");
        let mut regs = vec![boxed_int(0), boxed_int(0)];
        let mut cache = LoopCache::default();
        // SAFETY: local register file with two slots.
        let rc = unsafe { run(&compiled, regs.as_mut_ptr(), &mut cache) };
        assert_eq!(rc, 0);
        assert_eq!(unbox(regs[0]), 1000);
        assert_eq!(unbox(regs[1]), 499_500);
    }

    #[test]
    fn finds_all_loops_and_skips_unsupported_ones() {
        // A float loop first, then an integer loop. `find_loop` keeps the old
        // "first loop decides" contract, `find_loops` skips the float one.
        let code = vec![
            instr(OP_WHILE_LT_INT, 0, 10, 0),    // header 0, exit -> wide[0]=3
            instr(OP_LOAD_CONST_LOCAL, 1, 0, 1), // float constant: unsupported
            instr(OP_JUMP, 0, 0, 0),
            instr(OP_WHILE_LT_INT, 0, 10, 1), // header 3, exit -> wide[1]=6
            instr(OP_INC_LOCAL, 0, 0, 0),
            instr(OP_JUMP, 3, 0, 0),
        ];
        let wide = vec![3, 6];

        assert!(
            find_loop(&code, &wide).is_none(),
            "the first loop is a float loop"
        );
        let loops = find_loops(&code, &wide);
        assert_eq!(loops.len(), 1, "only the integer loop qualifies");
        assert_eq!(loops[0].start, 3);
        assert_eq!(loops[0].exit_ip, 6);
    }

    #[test]
    fn bails_out_on_a_non_integer_local() {
        let code = vec![
            instr(OP_WHILE_LT_INT, 0, 10, 0),
            instr(OP_INC_LOCAL, 0, 0, 0),
            instr(OP_JUMP, 0, 0, 0),
        ];
        let wide = vec![3];
        let spec = find_loop(&code, &wide).expect("loop");
        let compiled = ensure_compiled(&code, &wide, &spec, 1).expect("compile");
        // A float tag (any non-int) must bail without writing anything.
        let mut regs = vec![0x7FF8_0000_0000_0000];
        let mut cache = LoopCache::default();
        // SAFETY: one-slot register file.
        let rc = unsafe { run(&compiled, regs.as_mut_ptr(), &mut cache) };
        assert_eq!(rc, 1);
        assert_eq!(regs[0], 0x7FF8_0000_0000_0000);
    }
}
