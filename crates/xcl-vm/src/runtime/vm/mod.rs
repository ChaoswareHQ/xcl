//! High-performance stack-based virtual machine runtime for executing XCL bytecode.
//!
//! # Overview
//!
//! The [`VM`] executes compiled [`Bytecode`] using a contiguous
//! operand stack, a call frame stack, and a high-speed bump arena ([`bumpalo::Bump`]) for
//! short-lived allocations.
//!
//! # Architecture & Execution Model
//!
//! * **Stack & Registers**: The VM maintains a contiguous value stack for evaluation.
//!   Local variables are stored directly in small-vector register arrays inside each `Frame`,
//!   avoiding stack pointer recalculation overhead.
//! * **Domain Method Dispatch**: Methods on values and objects are dispatched through
//!   domain slots ([`DomainSlot`]), providing fast `O(1)` index lookup for core domains
//!   and plugin methods.
//! * **Bump Allocation Lifecycle**: Temporary strings, arrays, and dictionaries created
//!   during program evaluation are allocated from [`VM::arena`]. Resetting the arena
//!   after a run provides batch deallocation without individual garbage collection pauses.
//! * **Reentrancy**: [`VM::call_value`] supports reentrant execution of closures and functions,
//!   enabling native Rust callbacks to invoke XCL code seamlessly.
//! * **Deferred Cleanup (`defer`)**: Frame-scoped closures registered with `defer { ... }`
//!   or [`VM::push_defer`] are guaranteed to execute in LIFO order upon normal return or early exit.
//!
//! # Examples
//!
//! Running bytecode with a virtual machine:
//!
//! ```
//! use xcl_vm::runtime::VM;
//! use xcl_vm::compiler::lexer::lex;
//! use xcl_vm::compiler::parser::parse;
//! use xcl_vm::bytecode::compile;
//!
//! let mut vm = VM::new();
//! let tokens = lex("$x = 10; $y = 20; $x + $y", "<eval>");
//! let ast = parse(&tokens, "$x = 10; $y = 20; $x + $y");
//! let bytecode = compile(&ast);
//!
//! let result = vm.run(&bytecode);
//! assert_eq!(result.as_i48(), 30);
//! ```

pub mod ops;

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::format;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::string::String;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec;
#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(feature = "std")]
use crate::HashMap;
#[cfg(not(feature = "std"))]
use hashbrown::HashMap;

// Thread-local bump arena pool for reusing memory across VM instances.
// Holds at most one `Bump` per thread. When a `VM` is dropped, its arena is
// reset and returned to this pool (if small enough) so the next `VM::new()`
// reuses the same pages instead of faulting in fresh memory.
#[cfg(feature = "std")]
std::thread_local! {
    static ARENA_POOL: core::cell::RefCell<Option<bumpalo::Bump>> =
        const { core::cell::RefCell::new(None) };
}

use crate::bytecode::*;
use crate::runtime::value::{
    BuiltinImpl, ImplDescriptor, NativeEntry, TraitDescriptor, TypeDescriptor, Value,
};
use bumpalo::Bump;

/// FNV-1a offset basis.
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;

/// Continues an FNV-1a hash state over `bytes`.
///
/// FNV-1a is resumable, so `fast_hash_step(fast_hash(a), b) == fast_hash(a ++ b)`.
/// String concatenation uses this to avoid re-hashing the left operand, which
/// would make `s = s + x` in a loop O(n²) on hashing alone.
#[inline(always)]
pub fn fast_hash_step(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

/// Fast string hash for field name lookups (FNV-1a).
#[inline(always)]
pub fn fast_hash(bytes: &[u8]) -> u64 {
    fast_hash_step(FNV_OFFSET, bytes)
}

/// Tracks opcode dispatch frequency for profiling.
/// Gated behind `cfg(feature = "opcode-stats")` for zero overhead in normal builds.
#[cfg(feature = "opcode-stats")]
struct OpcodeCounter {
    counts: [u64; 256],
}

#[cfg(feature = "opcode-stats")]
impl OpcodeCounter {
    fn new() -> Self {
        Self { counts: [0; 256] }
    }

    #[inline(always)]
    fn record(&mut self, op: u8) {
        self.counts[op as usize] += 1;
    }

    fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// Returns a sorted list of (opcode, count) pairs, highest count first.
    fn top(&self, n: usize) -> Vec<(u8, u64)> {
        let mut pairs: Vec<(u8, u64)> = self
            .counts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| if c > 0 { Some((i as u8, c)) } else { None })
            .collect();
        pairs.sort_by_key(|&(_, c)| core::cmp::Reverse(c));
        pairs.truncate(n);
        pairs
    }
}

/// Action the VM dispatch loop should take after executing an instruction.
/// What the dispatch loop should do after an instruction.
///
/// This used to be a separate enum with the same five variants as
/// [`OpcodeResult`], built by converting the handler's result on every single
/// instruction. They are now the same type.
type Action = crate::runtime::opcode::OpcodeResult;

/// Number of entries in the direct-mapped callee cache.
///
/// Sixteen slots (256 bytes per VM) keeps alternating call sites — e.g.
/// `is_even` / `is_odd` — from evicting each other; four slots thrashed and
/// made every tail call miss.
pub(crate) const CALL_CACHE_SLOTS: usize = 16;
/// Mask for [`CALL_CACHE_SLOTS`] (a power of two).
pub(crate) const CALL_CACHE_MASK: usize = CALL_CACHE_SLOTS - 1;

/// A call frame on the VM's frame stack.
///
/// # Lifetime invariant
///
/// A frame's bytecode pointer may be dereferenced **only while the frame is
/// active**, i.e. at an index `< self.frame_depth`, and only during the `run()`
/// or `call_value()` invocation that activated it.
///
/// That is sound because every bytecode that can become a frame is kept alive
/// for the whole of that invocation by one of:
/// 1. the root: `run(&mut self, bc: &Bytecode)` borrows it for the call;
/// 2. callees: `FuncValue.bytecode: Option<Arc<Bytecode>>` owned by globals,
///    call_cache, closures, the operand stack, or the arena (reset only at
///    the start of `run`);
/// 3. `Bytecode.user_functions` / `Bytecode.closures` of the root bytecode.
///
/// After `run()` / `call_value()` returns, `frame_depth` is back to its entry
/// value, so the now-dangling pointers in `frames` are never dereferenced.
pub(crate) struct Frame {
    /// Bytecode this frame executes.
    ///
    /// Borrowed, not owned: see the lifetime invariant above. A frame
    /// may only dereference this while it is active (`index < frame_depth`).
    code_ptr: *const Bytecode,
    /// This frame's local slots inside the VM's shared register file.
    ///
    /// Copyable and pointer-based: handlers need to read/write registers while
    /// still touching other VM fields (`arena`, `stack`), which a `&mut [Value]`
    /// borrowed through a method would forbid. One contiguous register file for
    /// all frames (Luau's `base`/`regs[base + i]` design) means entering a call
    /// is a `resize`, not a fresh vector per depth.
    locals: LocalSlice,
    /// Start of this frame's registers, as an index into `VM::regs`.
    local_base: u32,
    ip: u32,
    /// Iteration buffer for `for x in array`.
    ///
    /// `Vec` rather than `SmallVec` on purpose: three inline `SmallVec`s made
    /// `Frame` ~240 bytes, and every call reinitializes all three. At 24 bytes
    /// each the frame shrinks to ~136 bytes, which matters because frames are
    /// allocated per call depth and touched on every call/return.
    iter: Vec<Value>,
    iter_idx: u32,
    range_cur: i64,
    range_end: i64,
    stack_base: u32,
    /// Deferred closures to run when this frame returns (LIFO order).
    defer_stack: Vec<Value>,
    /// Positional arguments passed into this frame (`$1`, `$#`, `$@`, `$*`).
    argv: Vec<Value>,
}

impl Frame {
    /// Dereference the borrowed bytecode pointer.
    ///
    /// # Safety
    /// Only call while this frame is active; see the module-level invariant.
    #[inline(always)]
    fn code(&self) -> &Bytecode {
        // SAFETY: caller guarantees this frame is active (index < frame_depth)
        // during run() or call_value(), so the pointer is valid per the
        // lifetime invariant documented on Frame.
        unsafe { &*self.code_ptr }
    }
}

// SAFETY: the pointer targets a heap-allocated `Bytecode`, not memory owned by
// the frame or by the `Vec<Frame>`. Sending a VM with `frame_depth == 0` moves
// only dangling pointers; the next `run` re-initialises them before any
// dereference.
unsafe impl Send for Frame {}

/// A frame's view of the shared register file (`VM::regs[base..base + len]`).
#[derive(Clone, Copy)]
pub(crate) struct LocalSlice {
    ptr: *mut Value,
    len: u32,
}

impl LocalSlice {
    const EMPTY: Self = Self {
        ptr: core::ptr::NonNull::<Value>::dangling().as_ptr(),
        len: 0,
    };

    /// # Safety
    /// `i` must be a valid slot index for this frame.
    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, i: usize) -> &'static Value {
        unsafe { &*self.ptr.add(i) }
    }

    /// # Safety
    /// `i` must be a valid slot index for this frame.
    #[inline(always)]
    pub(crate) unsafe fn get_unchecked_mut(&self, i: usize) -> &'static mut Value {
        unsafe { &mut *self.ptr.add(i) }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub(crate) fn as_slice(&self) -> &'static [Value] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len as usize) }
    }
}

impl core::ops::Index<usize> for LocalSlice {
    type Output = Value;
    #[inline(always)]
    fn index(&self, i: usize) -> &Value {
        unsafe { self.get_unchecked(i) }
    }
}

// SAFETY: the pointer addresses `VM::regs`, a heap allocation that does not
// move when the VM is moved, and `Value` is a plain 64-bit word. Sending a VM
// to another thread therefore sends a valid pointer to its own allocation.
unsafe impl Send for LocalSlice {}
unsafe impl Sync for LocalSlice {}

impl core::ops::IndexMut<usize> for LocalSlice {
    #[inline(always)]
    fn index_mut(&mut self, i: usize) -> &mut Value {
        unsafe { self.get_unchecked_mut(i) }
    }
}

/// Returns the raw instruction bounds of a frame's bytecode.
///
/// The dispatch loop keeps these in locals; a raw pointer avoids borrowing
/// `VM` for the duration of the loop (handlers need `&mut VM`).
#[inline(always)]
fn frame_code_bounds(frame: &Frame) -> (*const Instr, usize) {
    // SAFETY: frame_code_bounds is only called for active frames
    // (index < frame_depth) during run() or call_value().
    let code = frame.code();
    (code.instructions.as_ptr(), code.instructions.len())
}

/// A loop compiled to native code (feature `jit`).
#[cfg(feature = "jit")]
#[derive(Clone, Copy)]
struct JitEntry {
    /// Interpreter ip to resume at after a normal exit.
    exit_ip: usize,
    /// Native entry point.
    func: xcl_jit::LoopFn,
    /// First index into `JitUnit::accesses`.
    access_start: u32,
    /// Number of access plans for this loop.
    access_len: u32,
}

/// All native loops compiled for one bytecode unit.
#[cfg(feature = "jit")]
struct JitUnit {
    entries: Vec<JitEntry>,
    /// `shadow[ip]` is `0` when no loop starts at `ip`, otherwise `index + 1`.
    /// A flat table keeps the dispatch check to a bounds test and one load.
    shadow: Vec<u32>,
    /// Memory-access plans, in entry order.
    accesses: Vec<xcl_jit::AccessPlan>,
    /// One runtime cache per compiled loop.
    caches: Vec<xcl_jit::LoopCache>,
}

/// The plan passed to the dispatch loop.
#[cfg(feature = "jit")]
type JitPlan<'a> = Option<&'a mut JitUnit>;
/// Without the feature, the dispatch loop has no plan at all.
#[cfg(not(feature = "jit"))]
type JitPlan<'a> = Option<&'a mut ()>;

/// The maximum number of compiled loops tracked per unit. The bail-out mask is
/// a `u64`, one bit per loop.
#[cfg(feature = "jit")]
const JIT_MAX_LOOPS: usize = 64;

/// Finds and compiles every hot integer loop of `bc`.
///
/// The compiled code is cached by `xcl-jit` for the life of the process, so
/// repeated `run` calls on the same bytecode pay the compile cost once.
#[cfg(feature = "jit")]
fn prepare_jit(bc: &Bytecode) -> Option<JitUnit> {
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
#[cfg(feature = "jit")]
fn prepare_jit_functions(bc: &Bytecode) -> HashMap<Box<str>, xcl_jit::JitFunction> {
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
#[cfg(feature = "jit")]
const JIT_ARRAY_SCAN_MAX: usize = 4096;

/// Fills a loop's access cache from the current frame registers.
///
/// Returns `false` when a container is not the expected type (for example a
/// map where the loop expects an array); the caller then leaves the loop to
/// the interpreter.
#[cfg(feature = "jit")]
fn fill_loop_cache(
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

/// A dispatch table slot containing all methods registered for a specific domain namespace.
#[derive(Clone)]
pub struct DomainSlot {
    /// Domain identifier name (e.g., `"fs"`, `"net"`, `"math"`).
    pub name: Box<str>,
    /// Array of method implementations indexed by method slot id.
    pub methods: Vec<Option<BuiltinImpl>>,
    /// Fast string-to-slot-id index mapping method names to offsets in `methods`.
    pub index: HashMap<Box<str>, u32>,
    /// If true, methods in this domain are auto-imported: bare identifiers like `map`
    /// will resolve to `collections.map` when not found in the global scope.
    /// Set to false for domains like `https` where method names (e.g., `get`) are too common.
    pub auto_import: bool,
}

/// The XCL Virtual Machine runtime instance.
///
/// Holds the evaluation operand stack, global variable tables, native domain dispatch tables,
/// memory allocation arena, and call frames.
pub struct VM {
    /// Operand stack used for bytecode evaluation.
    pub stack: Vec<Value>,
    /// Shared register file: locals of every live frame, contiguous.
    ///
    /// Frames index it with `local_base + slot` (Luau keeps registers in its
    /// stack for the same reason: entering a call is a `resize`, returning is a
    /// `truncate`, and all frames' locals share cache lines).
    regs: Vec<Value>,
    globals: Vec<Value>,
    global_index: HashMap<Box<str>, u32>,
    /// Table of globally available builtin and native functions.
    pub global_table: Vec<Option<BuiltinImpl>>,
    /// Fast string index mapping domain names to slot offsets in [`VM::domain_table`].
    pub domain_index: HashMap<Box<str>, u32>,
    /// Table of registered domain method slots.
    pub domain_table: Vec<DomainSlot>,
    /// Registry mapping type names to static type descriptors.
    pub type_index: HashMap<Box<str>, &'static TypeDescriptor>,
    /// Registry mapping trait names to static trait descriptors.
    pub trait_index: HashMap<Box<str>, &'static TraitDescriptor>,
    /// Flat list of all registered trait implementation descriptors.
    pub impl_index: Vec<&'static ImplDescriptor>,
    /// Flat list of registered native entries (for override_by_trait lookups).
    pub native_entries: Vec<&'static NativeEntry>,
    /// Flat list of registered type descriptors (used by `$super` lookups).
    pub type_entries: Vec<&'static TypeDescriptor>,
    /// Fast bump allocator arena for temporary heap objects created during execution.
    pub arena: Bump,
    pub(crate) frames: Vec<Frame>,
    closures: Arc<[crate::runtime::value::FuncValue]>,
    /// Stack-based context for domain blocks (e.g., `browser "brave" { ... }`).
    /// Each key maps to a stack of values, enabling nested domain contexts.
    pub(crate) context_stack: HashMap<String, Vec<Value>>,
    /// Dispatch table for extension opcodes (codes 128..=255).
    opcode_table: [Option<super::opcode::OpcodeFn>; 256],
    #[cfg(feature = "opcode-stats")]
    opcode_counter: OpcodeCounter,
    /// Name of the currently running script (`$0`).
    script_name: Box<str>,
    /// Top-level positional script arguments.
    script_args: Vec<Value>,
    /// Direct-mapped [`CALL_CACHE_SLOTS`]-entry callee cache for fast
    /// recursive call dispatch.
    call_cache: [(Value, Value); CALL_CACHE_SLOTS],
    /// Compiled integer functions for this run (name → native entry).
    #[cfg(feature = "jit")]
    jit_functions: HashMap<Box<str>, xcl_jit::JitFunction>,
    /// Native entry for each [`call_cache`](Self::call_cache) slot, if the
    /// resolved callee was compiled by the function JIT. Kept in lockstep with
    /// the callee cache so a hot call site pays one tuple load, not a hash.
    #[cfg(feature = "jit")]
    jit_call_cache: [Option<xcl_jit::JitFunction>; CALL_CACHE_SLOTS],
    /// Number of live call frames.
    ///
    /// `frames[..frame_depth]` are live; the rest are recycled slots kept
    /// around for reuse. Frames are never moved out of the vector: entering a
    /// call reinitializes the slot at `frame_depth` in place, returning
    /// decrements it. The old design pushed/popped `Frame` by value through a
    /// separate pool, which moved ~200 bytes three times per call (~20% of all
    /// retired instructions on recursive workloads).
    frame_depth: usize,
    /// Previously installed bytecode (for idempotent function installation).
    installed_code: Option<Arc<[Instr]>>,
    /// Stable slot indices assigned to user functions of the current bytecode.
    function_slots: Vec<u32>,
    /// Function names installed in `global_index` (for cleanup on bytecode switch).
    function_names: Vec<Box<str>>,
    /// Index in `globals` where the current bytecode's function values start.
    function_base: u32,
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

impl VM {
    /// Creates a new, uninitialized virtual machine instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::VM;
    ///
    /// let vm = VM::new();
    /// assert!(vm.stack.is_empty());
    /// ```
    pub fn new() -> Self {
        // Item 1: Try to reuse a pooled arena from a previous VM.
        let arena = {
            #[cfg(feature = "std")]
            {
                ARENA_POOL.with(|pool| pool.borrow_mut().take().unwrap_or_else(Bump::new))
            }
            #[cfg(not(feature = "std"))]
            {
                Bump::new()
            }
        };
        Self {
            stack: Vec::with_capacity(64),
            regs: Vec::with_capacity(64),
            globals: Vec::new(),
            global_index: Default::default(),
            global_table: Vec::new(),
            domain_index: Default::default(),
            domain_table: Vec::new(),
            type_index: Default::default(),
            trait_index: Default::default(),
            impl_index: Vec::new(),
            native_entries: Vec::new(),
            type_entries: Vec::new(),
            arena,
            frames: Vec::with_capacity(16),
            closures: Arc::new([]),
            context_stack: HashMap::new(),
            opcode_table: [None; 256],
            #[cfg(feature = "opcode-stats")]
            opcode_counter: OpcodeCounter::new(),
            script_name: "<input>".into(),
            script_args: Vec::new(),
            call_cache: [(Value::nil(), Value::nil()); CALL_CACHE_SLOTS],
            #[cfg(feature = "jit")]
            jit_functions: HashMap::new(),
            #[cfg(feature = "jit")]
            jit_call_cache: [None; CALL_CACHE_SLOTS],
            frame_depth: 0,
            installed_code: None,
            function_slots: Vec::new(),
            function_names: Vec::new(),
            function_base: 0,
        }
    }

    /// Sets the script name (`$0`) and top-level positional arguments (`$1..$N`).
    ///
    /// Call this before [`VM::run`]. Script arguments apply to the root frame;
    /// function frames use their own call arguments.
    pub fn set_script_args(&mut self, name: &str, args: Vec<Value>) {
        self.script_name = name.into();
        self.script_args = args;
    }

    /// Registers native functions from a static slice.
    pub fn register_natives(&mut self, entries: &'static [NativeEntry]) {
        use crate::runtime::value::{BuiltinImpl, NativeKind};
        for entry in entries {
            let impl_ = match entry.kind {
                NativeKind::Pure => {
                    let f: crate::runtime::value::BuiltinFn =
                        unsafe { core::mem::transmute(entry.ptr.0) };
                    BuiltinImpl::Pure(f)
                }
                NativeKind::Ctx => {
                    let f: crate::runtime::value::BuiltinCtxFn =
                        unsafe { core::mem::transmute(entry.ptr.0) };
                    BuiltinImpl::Ctx(f)
                }
            };

            if let Some(domain) = entry.domain {
                let full_name = format!("{}.{}", domain, entry.method);

                let didx = *self.domain_index.entry(domain.into()).or_insert_with(|| {
                    let idx = self.domain_table.len() as u32;
                    self.domain_table.push(DomainSlot {
                        name: domain.into(),
                        methods: Vec::new(),
                        index: HashMap::new(),
                        auto_import: false,
                    });
                    idx
                });
                let slot = &mut self.domain_table[didx as usize];
                if entry.auto_import {
                    slot.auto_import = true;
                }
                let midx = slot.methods.len() as u32;
                slot.methods.push(Some(impl_));
                slot.index.insert(entry.method.into(), midx);

                if entry.auto_import {
                    let idx = self.global_table.len() as u32;
                    self.global_table.push(Some(impl_));
                    self.global_index.entry(entry.method.into()).or_insert(idx);
                }

                let idx = self.global_table.len() as u32;
                self.global_table.push(Some(impl_));
                self.global_index.insert(full_name.into(), idx);
            } else {
                let idx = self.global_table.len() as u32;
                self.global_table.push(Some(impl_));
                self.global_index.insert(entry.method.into(), idx);
            }
            self.native_entries.push(entry);
        }
    }

    /// Registers type descriptors from a static slice.
    pub fn register_types(&mut self, entries: &'static [TypeDescriptor]) {
        for desc in entries {
            self.type_index.insert(desc.name.into(), desc);
            self.type_entries.push(desc);
        }
    }

    /// Registers trait descriptors from a static slice.
    pub fn register_traits(&mut self, entries: &'static [TraitDescriptor]) {
        for desc in entries {
            self.trait_index.insert(desc.name.into(), desc);
        }
    }

    /// Registers impl descriptors from a static slice.
    pub fn register_impls(&mut self, entries: &'static [ImplDescriptor]) {
        self.impl_index.clear();
        for desc in entries {
            self.impl_index.push(desc);
        }
    }

    /// Registers opcode entries from a static slice.
    pub fn register_opcodes(&mut self, entries: &'static [super::opcode::OpcodeEntry]) {
        for entry in entries.iter() {
            self.opcode_table[entry.code as usize] = Some(entry.handler);
        }
    }

    /// Runs the top frame's deferred closures in LIFO order.
    ///
    /// Returns immediately when there is nothing deferred, which is the common
    /// case; every function return goes through here.
    #[inline(always)]
    fn run_defers(&mut self) {
        let Some(frame) = self.top_frame_mut() else {
            return;
        };
        if frame.defer_stack.is_empty() {
            return;
        }
        let mut stack = core::mem::take(&mut frame.defer_stack);
        stack.reverse();
        for closure in stack {
            let _ = self.call_value(closure, &[]);
        }
    }

    /// Pops a value from the operand stack.
    pub fn pop_value(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::nil())
    }

    /// Pushes a value onto the operand stack.
    pub fn push_value(&mut self, value: Value) {
        self.stack.push(value);
    }

    /// Number of live call frames.
    #[inline(always)]
    fn frame_count(&self) -> usize {
        self.frame_depth
    }

    /// The live frame at the top of the call stack, if any.
    #[inline(always)]
    fn top_frame(&self) -> Option<&Frame> {
        match self.frame_depth {
            0 => None,
            n => Some(unsafe { self.frames.get_unchecked(n - 1) }),
        }
    }

    /// Mutable access to the live frame at the top of the call stack.
    #[inline(always)]
    fn top_frame_mut(&mut self) -> Option<&mut Frame> {
        match self.frame_depth {
            0 => None,
            n => Some(unsafe { self.frames.get_unchecked_mut(n - 1) }),
        }
    }

    /// Acquires the frame slot for a new call and returns its index.
    ///
    /// The slot is reset in place, so no `Frame` is ever moved. Callers then
    /// fill in `stack_base`, `locals` and `argv` through `self.frames[idx]`.
    #[inline]
    fn acquire_frame(&mut self, bc: &Bytecode) -> usize {
        let idx = self.frame_depth;
        if idx == self.frames.len() {
            self.frames.push(Frame {
                code_ptr: bc as *const Bytecode,
                locals: LocalSlice::EMPTY,
                local_base: 0,
                ip: 0,
                iter: Vec::new(),
                iter_idx: 0,
                range_cur: 0,
                range_end: 0,
                stack_base: 0,
                defer_stack: Vec::new(),
                argv: Vec::new(),
            });
        }
        self.frame_depth = idx + 1;

        let frame = unsafe { self.frames.get_unchecked_mut(idx) };
        frame.code_ptr = bc as *const Bytecode;
        frame.ip = 0;
        // Item 3: only clear when non-empty — avoids writing length on every call.
        if !frame.iter.is_empty() {
            frame.iter.clear();
        }
        frame.iter_idx = 0;
        frame.range_cur = 0;
        frame.range_end = 0;
        if !frame.defer_stack.is_empty() {
            frame.defer_stack.clear();
        }
        if !frame.argv.is_empty() {
            frame.argv.clear();
        }
        // `locals`/`local_base` are set by `bind_registers`, which every caller
        // runs next.
        idx
    }

    /// Allocates `n` registers for frame `idx` at the top of the register file
    /// and binds the frame to them.
    ///
    /// `copied_params` is the number of slots the caller writes immediately
    /// after this call (parameters taken from the operand stack). Those slots
    /// are not written here, which removes one store per parameter from the
    /// call path. The remaining `[copied_params..n)` slots are nil-filled.
    /// A count of `0` (the root frame) nil-fills everything.
    #[inline(always)]
    // Clippy cannot see that every element is written before it is read; the
    // SAFETY comment below is the proof. `Value` is `Copy` and has no `Drop`.
    #[allow(clippy::uninit_vec)]
    fn bind_registers(&mut self, idx: usize, n: usize, copied_params: usize) -> usize {
        let base = self.regs.len();
        self.regs.reserve(n);
        // SAFETY: `Value` is `Copy` and has no `Drop`. `reserve` guarantees
        // capacity for `base + n`. Every slot in `[base, base + n)` is written
        // before it can be read: the caller writes
        // `[0, copied_params)` right after this call, and the loop below
        // writes the rest. `write` does not read the old (possibly
        // uninitialised) contents.
        unsafe {
            self.regs.set_len(base + n);
            let start = self.regs.as_mut_ptr().add(base + copied_params.min(n));
            let end = self.regs.as_mut_ptr().add(base + n);
            let mut p = start;
            while p < end {
                p.write(Value::nil());
                p = p.add(1);
            }
        }
        let frame = unsafe { self.frames.get_unchecked_mut(idx) };
        frame.local_base = base as u32;
        frame.locals = LocalSlice {
            ptr: unsafe { self.regs.as_mut_ptr().add(base) },
            len: n as u32,
        };
        base
    }

    /// Drops the registers of a frame that is returning.
    #[inline(always)]
    fn release_registers(&mut self, idx: usize) {
        let base = unsafe { self.frames.get_unchecked(idx) }.local_base as usize;
        self.regs.truncate(base);
    }

    /// Rebinds frame `idx` to `n` registers at `base`, growing the file if the
    /// (tail-called) function needs more slots than the current one.
    ///
    /// `copied_params` slots are overwritten by the caller; the tail is
    /// nil-filled.
    #[inline(always)]
    #[allow(clippy::uninit_vec)]
    fn rebind_registers(&mut self, idx: usize, base: usize, n: usize, copied_params: usize) {
        self.regs.truncate(base);
        self.regs.reserve(n);
        // SAFETY: same argument as `bind_registers`: `Value` has no `Drop`, the
        // capacity is reserved, and every slot is written before it is read.
        unsafe {
            self.regs.set_len(base + n);
            let start = self.regs.as_mut_ptr().add(base + copied_params.min(n));
            let end = self.regs.as_mut_ptr().add(base + n);
            let mut p = start;
            while p < end {
                p.write(Value::nil());
                p = p.add(1);
            }
        }
        let frame = unsafe { self.frames.get_unchecked_mut(idx) };
        frame.local_base = base as u32;
        frame.locals = LocalSlice {
            ptr: unsafe { self.regs.as_mut_ptr().add(base) },
            len: n as u32,
        };
    }

    /// Reads a local variable from the current frame.
    pub fn local_get(&self, top: usize, slot: usize) -> Value {
        self.frames[top].locals[slot]
    }

    /// Writes a local variable in the current frame.
    pub fn local_set(&mut self, top: usize, slot: usize, value: Value) {
        self.frames[top].locals[slot] = value;
    }

    /// Returns a slice of local variables in the current frame.
    pub fn frame_locals(&self, top: usize) -> &[Value] {
        self.frames[top].locals.as_slice()
    }

    /// Returns a mutable slice of local variables in the current frame.
    #[allow(clippy::mut_from_ref)]
    pub fn frame_locals_mut(&mut self, top: usize) -> &mut [Value] {
        let frame = &self.frames[top];
        let len = frame.locals.len();
        unsafe { core::slice::from_raw_parts_mut(frame.locals.ptr, len) }
    }

    /// Looks up a globally registered builtin or native function by name.
    ///
    /// # Arguments
    ///
    /// * `name` - Function name or full dotted identifier (e.g. `"print"` or `"math.sin"`).
    pub fn resolve_builtin(&self, name: &str) -> Option<&BuiltinImpl> {
        self.global_index
            .get(name)
            .and_then(|&idx| self.global_table.get(idx as usize).and_then(|e| e.as_ref()))
    }

    /// Looks up a method registered within a specific domain namespace.
    ///
    /// # Arguments
    ///
    /// * `domain` - Domain name (e.g. `"fs"`).
    /// * `method` - Method name (e.g. `"read_file"`).
    pub fn resolve_domain_method(&self, domain: &str, method: &str) -> Option<&BuiltinImpl> {
        let &didx = self.domain_index.get(domain)?;
        let slot = self.domain_table.get(didx as usize)?;
        let &midx = slot.index.get(method)?;
        slot.methods.get(midx as usize).and_then(|e| e.as_ref())
    }

    /// Looks up a universal method (domain=None) by name.
    ///
    /// Universal methods are natives registered without a domain (for example
    /// plugin-provided helpers); the `$` axioms are VM keywords and do not go
    /// through this table.
    fn resolve_universal_method(&self, method: &str) -> Option<&BuiltinImpl> {
        self.global_index
            .get(method)
            .and_then(|&idx| self.global_table.get(idx as usize).and_then(|e| e.as_ref()))
    }

    /// Save current global function mappings and override them with new implementations.
    /// Returns the saved state for later restoration via `restore_globals`.
    pub fn save_and_override_globals(
        &mut self,
        overrides: &[(&'static str, crate::runtime::value::BuiltinImpl)],
    ) -> Vec<(Box<str>, Option<u32>)> {
        let mut saved = Vec::with_capacity(overrides.len());
        for (name, impl_) in overrides {
            let existing = self
                .global_index
                .insert((*name).into(), self.global_table.len() as u32);
            saved.push(((*name).into(), existing));
            self.global_table.push(Some(*impl_));
        }
        saved
    }

    /// Restore global function mappings from a saved state.
    pub fn restore_globals(&mut self, saved: &[(Box<str>, Option<u32>)]) {
        for (name, existing) in saved.iter().rev() {
            self.global_table.pop();
            if let Some(idx) = *existing {
                self.global_index.insert(name.clone(), idx);
            } else {
                self.global_index.remove(name.as_ref());
            }
        }
    }

    #[inline(always)]
    fn pop(&mut self) -> Value {
        unsafe { self.stack.pop().unwrap_unchecked() }
    }
    #[inline(always)]
    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    /// Executes a compiled [`Bytecode`] unit to completion and returns the final [`Value`].
    ///
    /// Resets the operand stack and call frames before starting execution.
    ///
    /// # Arguments
    ///
    /// * `bc` - Compiled bytecode unit to execute.
    ///
    /// # Returns
    ///
    /// The final evaluated [`Value`] remaining on the stack, or `nil` if the stack was empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::runtime::VM;
    /// use xcl_vm::compiler::lexer::lex;
    /// use xcl_vm::compiler::parser::parse;
    /// use xcl_vm::bytecode::compile;
    ///
    /// let mut vm = VM::new();
    /// let tokens = lex("50 * 2", "<test>");
    /// let ast = parse(&tokens, "50 * 2");
    /// let bc = compile(&ast);
    ///
    /// let result = vm.run(&bc);
    /// assert_eq!(result.as_i48(), 100);
    /// ```
    ///
    /// # Contract
    ///
    /// Calling `run()` invalidates any `Value` previously obtained from this VM
    /// (the arena is reset). The result is escaped before return so it survives
    /// the next `run()` call.
    pub fn run(&mut self, bc: &Bytecode) -> Value {
        #[cfg(feature = "jit")]
        {
            self.jit_functions = prepare_jit_functions(bc);
            if let Some(mut jit) = prepare_jit(bc) {
                return self.run_inner::<true>(bc, Some(&mut jit));
            }
        }
        self.run_inner::<false>(bc, None)
    }

    /// Records a resolved callee in the call cache and, when the callee has a
    /// compiled integer body, in the parallel JIT slot cache.
    ///
    /// Both tables are indexed by the same hash slot and written together, so
    /// the dispatch fast path can consult `jit_call_cache` without re-hashing
    /// the callee name.
    #[cfg(feature = "jit")]
    #[inline]
    pub(crate) fn cache_callee(&mut self, slot: usize, callee: Value, func: Value) {
        self.call_cache[slot] = (callee, func);
        self.jit_call_cache[slot] = func
            .as_func()
            .and_then(|f| self.jit_functions.get(&*f.name).copied());
    }

    /// Without the JIT feature only the interpreter cache exists.
    #[cfg(not(feature = "jit"))]
    #[inline(always)]
    pub(crate) fn cache_callee(&mut self, slot: usize, callee: Value, func: Value) {
        self.call_cache[slot] = (callee, func);
    }

    /// Dispatch loop, monomorphised on whether a native loop is installed.
    ///
    /// `JIT == false` contains no JIT code at all, so interpreted programs pay
    /// nothing for the feature being compiled in.
    fn run_inner<const JIT: bool>(&mut self, bc: &Bytecode, jit: JitPlan<'_>) -> Value {
        #[cfg(feature = "jit")]
        let mut jit = jit;
        #[cfg(not(feature = "jit"))]
        let _ = jit;
        self.stack.clear();
        self.frame_depth = 0;
        if self.frames.len() > 4096 {
            self.frames.truncate(4096);
        }
        // Item 2: clear growing state so repeated runs leave nothing behind.
        self.context_stack.clear();
        self.call_cache = [(Value::nil(), Value::nil()); CALL_CACHE_SLOTS];
        #[cfg(feature = "jit")]
        {
            self.jit_call_cache = [None; CALL_CACHE_SLOTS];
        }
        self.arena.reset();
        self.closures = Arc::clone(&bc.closures);

        // Install (or refresh) the bytecode's top-level functions.
        //
        // Slots are stable across runs: `global_index` maps a name to
        // `global_table.len() + globals_index`, which is exactly what `OP_CALL`
        // subtracts the table length from. Function values live in the arena,
        // so they must be rebuilt after the arena reset above.
        let current_code = bc.instructions.clone();
        let is_same_bytecode = self
            .installed_code
            .as_ref()
            .map(|prev| Arc::ptr_eq(prev, &current_code))
            .unwrap_or(false);

        if !is_same_bytecode {
            // Drop the previous bytecode's function names and values.
            for name in self.function_names.drain(..) {
                self.global_index.remove(&name);
            }
            self.function_slots.clear();
            self.globals.truncate(self.function_base as usize);
        }

        // Compiler globals (enum variants, type names) occupy slots
        // `0..global_names.len()` and come before the function values.
        let compiler_globals = bc.global_names.len();
        if self.globals.len() < compiler_globals {
            self.globals.resize(compiler_globals, Value::nil());
        }
        let function_base = self.globals.len() as u32;

        if !is_same_bytecode {
            let table_len = self.global_table.len() as u32;
            for (i, (name, func)) in bc.user_functions.iter().enumerate() {
                let slot = function_base + i as u32;
                self.function_slots.push(slot);
                self.function_names.push(name.as_ref().into());
                self.globals
                    .push(Value::from_func_shared(Arc::clone(func), &self.arena));
                self.global_index
                    .insert(name.as_ref().into(), table_len + slot);
            }
            self.function_base = function_base;
            self.installed_code = Some(current_code);
        } else {
            // Same bytecode — rewrite each slot with a fresh Value (arena was reset).
            for (i, (_name, func)) in bc.user_functions.iter().enumerate() {
                let slot = self.function_slots[i] as usize;
                self.globals[slot] = Value::from_func_shared(Arc::clone(func), &self.arena);
            }
        }

        let n = bc.local_names.len().max(16);
        self.regs.clear();
        // SAFETY: bc is borrowed by run() and lives for the entire call.
        let root = self.acquire_frame(bc);
        // Item 3: root frame has no parameters, so nil-fill every slot.
        self.bind_registers(root, n, 0);
        {
            let frame = &mut self.frames[root];
            frame.stack_base = 0;
            frame.argv.extend_from_slice(&self.script_args);
        }

        // Dispatch state kept in locals instead of behind `frames[top]`.
        //
        // The instruction pointer and the current code slice are the two things
        // every instruction needs. The previous loop reloaded `frames[top]`,
        // `frame.ip` and `frame.code.instructions` (three dependent loads) and
        // wrote `ip` back through the frame for every single instruction.
        // Now `ip` lives in a register and is only written to the frame when a
        // call is entered (the return address), which is the only time another
        // frame needs it.
        let mut top = self.frame_count() - 1;
        let mut ip: usize = 0;
        let (mut code_ptr, mut code_len) = frame_code_bounds(&self.frames[top]);
        let mut locals = self.frames[top].locals;

        // Native loop state. The shadow table maps an instruction to its
        // compiled loop; `jit_disabled` remembers loops that bailed out during
        // this run. It applies only to the root frame's own bytecode. With
        // `JIT == false` every use of these is eliminated.
        #[cfg(feature = "jit")]
        let (jit_shadow_ptr, jit_shadow_len) = match jit.as_deref() {
            Some(u) => (u.shadow.as_ptr(), u.shadow.len()),
            None => (core::ptr::null(), 0),
        };
        #[cfg(feature = "jit")]
        let mut jit_disabled: u64 = 0;
        #[cfg(feature = "jit")]
        let root_code_ptr = bc.instructions.as_ptr();

        loop {
            if ip >= code_len {
                // Run deferred closures before returning
                self.run_defers();
                let r = self.stack.pop().unwrap_or(Value::nil());
                let frame = unsafe { self.frames.get_unchecked(top) };
                let returning_base = frame.stack_base;
                let local_base = frame.local_base as usize;
                self.regs.truncate(local_base);
                self.frame_depth -= 1;
                debug_assert_eq!(self.frame_depth, top);
                if self.frame_depth == 0 {
                    return Value::escape(r);
                }
                top -= 1;
                let caller = unsafe { self.frames.get_unchecked(top) };
                let base = returning_base;
                ip = caller.ip as usize;
                (code_ptr, code_len) = frame_code_bounds(caller);
                locals = caller.locals;
                self.stack.truncate(base as usize);
                self.stack.push(r);
                continue;
            }

            let instr = unsafe { *code_ptr.add(ip) };

            // Enter a native loop when the root frame reaches a compiled
            // header. The check is gated on the loop-header opcode, so
            // interpreted instructions pay one compare rather than a shadow
            // load. A bailout (non-integer local, multiply overflow, division
            // by zero) leaves the register file untouched, so the interpreter
            // re-executes the loop; that loop is disabled for the rest of this
            // run. In the `JIT == false` instantiation this block is gone.
            #[cfg(feature = "jit")]
            if JIT && top == 0 && code_ptr == root_code_ptr && jit_shadow_len != 0 {
                let op = instr.op();
                if op == ops::consts::OP_WHILE_LT_INT || op == ops::consts::OP_WHILE_LT_INT_WIDE {
                    // SAFETY: `ip < code_len`, and the shadow belongs to the
                    // root bytecode, so its length is `code_len`.
                    let slot = unsafe { *jit_shadow_ptr.add(ip) };
                    let idx = slot.wrapping_sub(1) as usize;
                    if slot != 0 && idx < JIT_MAX_LOOPS && jit_disabled & (1u64 << idx) == 0 {
                        if let Some(unit) = jit.as_deref_mut() {
                            let entry = unit.entries[idx];
                            let base = self.frames[top].local_base as usize;
                            if !fill_loop_cache(&self.regs, bc, unit, idx, base) {
                                jit_disabled |= 1u64 << idx;
                            } else {
                                let ptr = unsafe { self.regs.as_mut_ptr().add(base) }.cast::<u64>();
                                // SAFETY: `ptr` addresses the root frame's registers,
                                // the frame is active, and the JIT only touches its locals.
                                let cache_ptr: *mut xcl_jit::LoopCache = &raw mut unit.caches[idx];
                                let rc = unsafe { (entry.func)(ptr, cache_ptr) };
                                if rc == 0 {
                                    ip = entry.exit_ip;
                                    continue;
                                }
                                jit_disabled |= 1u64 << idx;
                            }
                        }
                    }
                }
            }

            let action = self.dispatch(locals, top, instr);
            match action {
                Action::Next => ip += 1,
                Action::Jump(to) => ip = to as usize,
                Action::TailCall => {
                    // The frame rebound to another function's bytecode.
                    ip = 0;
                    let frame = unsafe { self.frames.get_unchecked(top) };
                    (code_ptr, code_len) = frame_code_bounds(frame);
                    locals = frame.locals;
                }
                Action::Return => {
                    // Run deferred closures before returning
                    self.run_defers();
                    let r = self.stack.pop().unwrap_or(Value::nil());
                    let frame = unsafe { self.frames.get_unchecked(top) };
                    let returning_base = frame.stack_base;
                    let local_base = frame.local_base as usize;
                    self.regs.truncate(local_base);
                    self.frame_depth -= 1;
                    if self.frame_depth == 0 {
                        return Value::escape(r);
                    }
                    top -= 1;
                    let caller = unsafe { self.frames.get_unchecked(top) };
                    let base = returning_base;
                    ip = caller.ip as usize;
                    (code_ptr, code_len) = frame_code_bounds(caller);
                    locals = caller.locals;
                    self.stack.truncate(base as usize);
                    self.stack.push(r);
                }
                Action::PushFrame => {
                    // The callee frame is already on the stack. Record where the
                    // caller resumes; nothing else needs the caller's ip.
                    unsafe { self.frames.get_unchecked_mut(top) }.ip = (ip + 1) as u32;
                    top += 1;
                    ip = 0;
                    let frame = unsafe { self.frames.get_unchecked(top) };
                    (code_ptr, code_len) = frame_code_bounds(frame);
                    locals = frame.locals;
                }
            }
        }
    }

    #[inline(always)]
    fn dispatch(&mut self, locals: LocalSlice, top: usize, i: Instr) -> Action {
        #[cfg(feature = "opcode-stats")]
        self.opcode_counter.record(i.op());

        // Core opcode dispatch
        if let Some(result) = ops::dispatch_core(self, locals, top, i) {
            return result;
        }

        // Extension opcode lookup
        let op_code = i.op();
        if let Some(handler) = self.opcode_table[op_code as usize] {
            handler(self, top, i)
        } else {
            Action::Next
        }
    }
}

/// Item 1: Return the bump arena to the thread-local pool so the next VM
/// reuses the same pages instead of faulting in fresh memory.
impl Drop for VM {
    fn drop(&mut self) {
        #[cfg(feature = "std")]
        {
            let mut arena = core::mem::replace(&mut self.arena, Bump::new());
            arena.reset();
            // Only pool arenas ≤ 64 MB; larger arenas are dropped so RSS
            // cannot grow without bound.
            if arena.allocated_bytes() <= 64 * 1024 * 1024 {
                ARENA_POOL.with(|pool| {
                    pool.borrow_mut().replace(arena);
                });
            }
        }
        #[cfg(not(feature = "std"))]
        {
            // Under no_std the pool is unavailable; the arena is simply dropped.
            let _ = &self.arena;
        }
    }
}

impl VM {
    fn enter_call(&mut self, func: &crate::runtime::value::FuncValue, args: &[Value]) -> Action {
        let Some(bc) = &func.bytecode else {
            self.push(Value::nil());
            return Action::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let base = self.stack.len() as u32;

        let idx = self.acquire_frame(bc);
        let copy = func.params.len().min(args.len());
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy].copy_from_slice(&args[..copy]);

        let frame = &mut self.frames[idx];
        frame.ip = 0;
        frame.stack_base = base;

        // Prepare argv (only if function uses special vars)
        if func.uses_special_vars {
            frame.argv.extend_from_slice(args);
        }

        // Return PushFrame so the dispatch loop doesn't advance ip
        Action::PushFrame
    }

    /// Enters a call whose callee and arguments are still on the operand stack.
    ///
    /// `arg_base` is the callee's slot; the arguments occupy
    /// `arg_base + 1 .. arg_base + 1 + nargs`. The operand stack is truncated to
    /// `arg_base` so the return value lands exactly where the callee was.
    ///
    /// This avoids materializing an intermediate argument buffer for the common
    /// case of calling a script function.
    fn enter_call_at(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        arg_base: usize,
        nargs: usize,
    ) -> crate::runtime::opcode::OpcodeResult {
        let Some(bc) = &func.bytecode else {
            self.stack.truncate(arg_base);
            self.push(Value::nil());
            return crate::runtime::opcode::OpcodeResult::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let arg_start = arg_base + 1;

        let idx = self.acquire_frame(bc);
        let copy = func.params.len().min(nargs);
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy]
            .copy_from_slice(&self.stack[arg_start..arg_start + copy]);

        {
            let frame = &mut self.frames[idx];
            frame.ip = 0;
            frame.stack_base = arg_start as u32;

            if func.uses_special_vars {
                frame
                    .argv
                    .extend_from_slice(&self.stack[arg_start..arg_start + nargs]);
            }
        }

        self.stack.truncate(arg_base);
        crate::runtime::opcode::OpcodeResult::PushFrame
    }

    /// Enters a call whose arguments are on the operand stack but whose callee
    /// is **not** (fused named calls embed it in the instruction).
    ///
    /// `arg_start` is the first argument; the result replaces it on return.
    fn enter_call_args_at(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        arg_start: usize,
        nargs: usize,
    ) -> crate::runtime::opcode::OpcodeResult {
        let Some(bc) = &func.bytecode else {
            self.stack.truncate(arg_start);
            self.push(Value::nil());
            return crate::runtime::opcode::OpcodeResult::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let copy = func.params.len().min(nargs);

        let idx = self.acquire_frame(bc);
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy]
            .copy_from_slice(&self.stack[arg_start..arg_start + copy]);

        {
            let frame = &mut self.frames[idx];
            frame.ip = 0;
            frame.stack_base = arg_start as u32;

            if func.uses_special_vars {
                frame
                    .argv
                    .extend_from_slice(&self.stack[arg_start..arg_start + nargs]);
            }
        }

        self.stack.truncate(arg_start);
        crate::runtime::opcode::OpcodeResult::PushFrame
    }

    /// Same as `enter_call` but returns [`OpcodeResult`] for use from opcode handlers.
    fn enter_call_from_op(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        args: &[Value],
    ) -> crate::runtime::opcode::OpcodeResult {
        match self.enter_call(func, args) {
            Action::PushFrame => crate::runtime::opcode::OpcodeResult::PushFrame,
            Action::Next => crate::runtime::opcode::OpcodeResult::Next,
            _ => crate::runtime::opcode::OpcodeResult::Next,
        }
    }

    /// Tail-call entry: reuses the current frame instead of pushing a new one.
    fn enter_tail_call(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        args: &[Value],
    ) -> crate::runtime::opcode::OpcodeResult {
        let Some(bc) = &func.bytecode else {
            if let Some(frame) = self.top_frame() {
                self.stack.truncate(frame.stack_base as usize);
            }
            self.push(Value::nil());
            return crate::runtime::opcode::OpcodeResult::Next;
        };

        let n = bc.local_names.len().max(func.params.len());

        // Reuse the current frame instead of pushing a new one
        let top = match self.frame_depth.checked_sub(1) {
            Some(i) => i,
            None => return crate::runtime::opcode::OpcodeResult::Next,
        };
        let frame = &mut self.frames[top];

        // SAFETY: this frame is active (index < frame_depth) during run().
        frame.code_ptr = Arc::as_ptr(bc);
        frame.ip = 0;
        frame.iter.clear();
        frame.iter_idx = 0;
        frame.range_cur = 0;
        frame.range_end = 0;
        frame.defer_stack.clear();

        // Rebind the frame's registers (the tail-called function may have a
        // different local count) and write the new arguments.
        let base = frame.local_base as usize;
        let copy = func.params.len().min(args.len());
        self.rebind_registers(top, base, n, copy);
        self.regs[base..base + copy].copy_from_slice(&args[..copy]);

        // Prepare argv (only if function uses special vars)
        let frame = &mut self.frames[top];
        frame.argv.clear();
        if func.uses_special_vars {
            frame.argv.extend_from_slice(args);
        }

        // Truncate operand stack to this frame's base
        self.stack.truncate(frame.stack_base as usize);

        // Jump to ip=0 so the dispatch loop starts the new code from the beginning
        crate::runtime::opcode::OpcodeResult::TailCall
    }

    /// Reentrantly invokes an XCL function or closure value with the given arguments.
    ///
    /// Runs the VM dispatch loop until the called function's frame returns, then returns its result.
    ///
    /// # Arguments
    ///
    /// * `func` - The [`Value`] representing the function or closure to invoke.
    /// * `args` - Positional and named arguments passed to the function.
    ///
    /// # Returns
    ///
    /// The [`Value`] returned by the invoked function, or `nil` if `func` is not callable.
    pub fn call_value(&mut self, func: Value, args: &[Value]) -> Value {
        let func_ref = match func.as_func() {
            Some(f) => f,
            None => return Value::nil(),
        };

        let bc = match &func_ref.bytecode {
            Some(b) => Arc::clone(b),
            None => return Value::nil(),
        };

        // Save and replace closures table so OP_LOAD_LAMBDA works for nested closures
        let saved_closures = core::mem::replace(&mut self.closures, Arc::clone(&bc.closures));

        // Prepare locals: params first (matching how enter_call does it)
        let n = bc.local_names.len().max(func_ref.params.len());

        let initial_depth = self.frame_depth;
        let initial_regs = self.regs.len();
        let stack_base = self.stack.len() as u32;

        let idx = self.acquire_frame(&bc);
        let copy = func_ref.params.len().min(args.len());
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy].copy_from_slice(&args[..copy]);
        {
            let frame = &mut self.frames[idx];
            frame.stack_base = stack_base;
            frame.argv.extend_from_slice(args);
        }

        // Run the VM dispatch loop until the frame we just pushed returns
        loop {
            // Recalculate top every iteration because PushFrame/Return change frame count
            let top = match self.frame_depth.checked_sub(1) {
                Some(i) => i,
                None => {
                    self.closures = saved_closures;
                    self.frame_depth = initial_depth;
                    self.regs.truncate(initial_regs);
                    return Value::nil();
                }
            };

            // If we've returned to the caller depth, the closure is done
            if self.frame_depth <= initial_depth {
                break;
            }

            let frame = &self.frames[top];
            // SAFETY: frame is active (index < frame_depth) during call_value().
            if frame.ip as usize >= frame.code().instructions.len() {
                // Run deferred closures before returning
                self.run_defers();
                // Frame ran off the end — treat top of stack as return value
                let result = self.stack.pop().unwrap_or(Value::nil());
                let returning_base = self.top_frame().map(|f| f.stack_base);
                self.release_registers(top);
                self.frame_depth -= 1;
                if self.frame_depth <= initial_depth {
                    self.stack
                        .truncate(returning_base.unwrap_or(stack_base) as usize);
                    self.closures = saved_closures;
                    return Value::escape(result);
                }
                // Nested call returned; hand the value to the outer frame
                let caller = &mut self.frames[self.frame_depth - 1];
                caller.ip += 1;
                let base = returning_base.unwrap_or(caller.stack_base);
                self.stack.truncate(base as usize);
                self.stack.push(result);
                continue;
            }

            // SAFETY: frame is active (index < frame_depth) during call_value().
            let instr = unsafe { *frame.code().instructions.get_unchecked(frame.ip as usize) };
            let action = self.dispatch(frame.locals, top, instr);
            match action {
                Action::Next => {
                    let top = self.frame_depth.checked_sub(1).unwrap_or(top);
                    self.frames[top].ip += 1;
                }
                Action::Jump(to) => {
                    let top = self.frame_depth.checked_sub(1).unwrap_or(top);
                    self.frames[top].ip = to;
                }
                Action::TailCall => {
                    let top = self.frame_depth.checked_sub(1).unwrap_or(top);
                    self.frames[top].ip = 0;
                }
                Action::Return => {
                    // Run deferred closures before returning
                    self.run_defers();
                    let result = self.stack.pop().unwrap_or(Value::nil());
                    let returning_base = self.top_frame().map(|f| f.stack_base);
                    self.release_registers(top);
                    self.frame_depth -= 1;
                    if self.frame_depth <= initial_depth {
                        self.stack
                            .truncate(returning_base.unwrap_or(stack_base) as usize);
                        self.closures = saved_closures;
                        return Value::escape(result);
                    }
                    if let Some(caller) = self.frames.get_mut(self.frame_depth - 1) {
                        caller.ip += 1;
                    }
                    let caller_base = self.frames[self.frame_depth - 1].stack_base;
                    self.stack
                        .truncate(returning_base.unwrap_or(caller_base) as usize);
                    self.stack.push(result);
                }
                Action::PushFrame => {
                    // Frame already pushed by enter_call; just continue
                    continue;
                }
            }
        }

        self.closures = saved_closures;
        self.regs.truncate(initial_regs);
        Value::nil()
    }

    /// Returns the opcode dispatch counts as a `HashMap<opcode, count>`.
    ///
    /// Only available when the `opcode-stats` feature is enabled.
    #[cfg(feature = "opcode-stats")]
    pub fn opcode_counts(&self) -> HashMap<u8, u64> {
        let mut map = HashMap::new();
        for (op, &count) in self.opcode_counter.counts.iter().enumerate() {
            if count > 0 {
                map.insert(op as u8, count);
            }
        }
        map
    }

    /// Pushes a closure onto the current execution frame's defer stack.
    ///
    /// Deferred closures are executed in LIFO order when the current function frame exits.
    ///
    /// # Arguments
    ///
    /// * `closure` - An XCL function or closure [`Value`] to run upon return.
    pub fn push_defer(&mut self, closure: Value) {
        if let Some(frame) = self.top_frame_mut() {
            frame.defer_stack.push(closure);
        }
    }

    /// Prints a formatted histogram of the top `n` opcodes sorted by dispatch count.
    ///
    /// Only available when the `opcode-stats` feature is enabled.
    #[cfg(feature = "opcode-stats")]
    pub fn print_opcode_histogram(&self, top_n: usize) {
        let total = self.opcode_counter.total();
        let top = self.opcode_counter.top(top_n);
        eprintln!("\n=== Opcode Dispatch Histogram (top {}) ===", top_n);
        eprintln!("Total dispatches: {}", total);
        eprintln!("{:<12} {:>12} {:>8}", "Opcode", "Count", "%");
        eprintln!("{:-<34}", "");
        for (op, count) in &top {
            let pct = *count as f64 / total as f64 * 100.0;
            let name = opcode_name(*op);
            eprintln!("{:<12} {:>12} {:>7.2}%", name, count, pct);
        }
        eprintln!("====================================\n");
    }
}

/// Returns a human-readable name for an opcode, or the hex value if unknown.
#[cfg(feature = "opcode-stats")]
fn opcode_name(op: u8) -> String {
    crate::runtime::opcode::opcode_name(op)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("0x{:02X}", op))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::compile;
    use crate::compiler::lexer::lex;
    use crate::compiler::parser::parse;

    fn eval_with_args(src: &str, name: &str, args: &[&str]) -> Value {
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let mut vm = VM::new();
        let vals: Vec<Value> = args
            .iter()
            .map(|s| Value::from_str_boxed((*s).to_string()))
            .collect();
        vm.set_script_args(name, vals);
        let result = vm.run(&bc);
        // Tests may inspect heap values (strings/maps) backed by the VM arena.
        // Keep the arena alive for the duration of the test process.
        core::mem::forget(vm);
        result
    }

    #[test]
    fn test_arg_count() {
        let result = eval_with_args("$#", "script.xcl", &["a", "b", "c"]);
        assert_eq!(result.as_i48(), 3);
    }

    #[test]
    fn test_positional_arg() {
        let result = eval_with_args("$1", "script.xcl", &["a"]);
        assert!(
            result.is_ptr(),
            "expected pointer, got tag={:016X}",
            result.0
        );
        assert_eq!(result.as_str(), Some("a"));
    }

    #[test]
    fn test_positional_arg_out_of_range() {
        let result = eval_with_args("$10", "script.xcl", &["a"]);
        assert!(result.is_nil());
    }

    #[test]
    fn test_script_name() {
        let result = eval_with_args("$0", "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("script.xcl"));
    }

    #[test]
    fn test_args_array() {
        // Keep the VM alive: `$@` allocates its array in the VM arena.
        let src = "$@";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let mut vm = VM::new();
        let vals: Vec<Value> = ["a", "b"]
            .iter()
            .map(|s| Value::from_str_boxed((*s).to_string()))
            .collect();
        vm.set_script_args("script.xcl", vals);
        let result = vm.run(&bc);
        let arr = result.as_array().expect("$@ should return an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str(), Some("a"));
        assert_eq!(arr[1].as_str(), Some("b"));
    }

    #[test]
    fn test_args_joined() {
        let result = eval_with_args("$*", "script.xcl", &["a", "b", "c d"]);
        assert_eq!(result.as_str(), Some("a b c d"));
    }

    #[test]
    #[should_panic(expected = "not supported")]
    fn test_unsupported_dollar_dollar() {
        eval_with_args("$$", "script.xcl", &[]);
    }

    #[test]
    #[should_panic(expected = "not supported")]
    fn test_unsupported_dollar_question() {
        eval_with_args("$?", "script.xcl", &[]);
    }

    #[test]
    #[should_panic(expected = "not supported")]
    fn test_unsupported_dollar_bang() {
        eval_with_args("$!", "script.xcl", &[]);
    }

    #[test]
    #[should_panic(expected = "not supported")]
    fn test_unsupported_dollar_dash() {
        eval_with_args("$-", "script.xcl", &[]);
    }

    #[test]
    #[should_panic(expected = "read-only")]
    fn test_assignment_read_only() {
        eval_with_args("$1 = 5", "script.xcl", &[]);
    }

    #[test]
    fn test_special_var_as_bare_call_arg() {
        let src = r#"
            id($x) { return $x }
            id $1
        "#;
        let result = eval_with_args(src, "script.xcl", &["hello"]);
        assert_eq!(result.as_str(), Some("hello"));
    }

    #[test]
    fn test_special_vars_in_function_scope() {
        let src = r#"
            f($a, $b) {
                if $# == 3 { return $3 }
                return "bad"
            }
            f("x", "y", "z")
        "#;
        let result = eval_with_args(src, "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("z"));
    }

    #[test]
    fn test_script_name_inside_function() {
        let src = r#"
            f() { return $0 }
            f()
        "#;
        let result = eval_with_args(src, "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("script.xcl"));
    }

    #[test]
    fn test_bare_calls_do_not_merge_across_newlines() {
        let src = "f \"a\"\nf \"b\"\n";
        let tokens = crate::compiler::lexer::lex(src, "<test>");
        let ast = crate::compiler::parser::parse(&tokens, src);
        assert_eq!(ast.len(), 2, "expected two statements, got {:?}", ast);
    }

    #[test]
    fn test_nested_call_argument_does_not_swallow_next_line() {
        let src = r#"
            f($x) { return $x }
            g($x) { return $x }
            h($x, $y) { return $y }
            h(f("a"), g("b"))
            f("c")
        "#;
        let tokens = crate::compiler::lexer::lex(src, "<test>");
        let ast = crate::compiler::parser::parse(&tokens, src);
        // 3 function definitions + 2 call statements
        assert_eq!(ast.len(), 5, "unexpected statement count: {:?}", ast);
    }

    // ── Bitwise OR/XOR/NOT, exponentiation, ternary tests ──

    #[test]
    fn test_bitor() {
        let result = eval_with_args("1 | 2", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 3);
    }

    #[test]
    fn test_bitxor() {
        let result = eval_with_args("6 ^ 3", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 5);
    }

    #[test]
    fn test_bitnot() {
        let result = eval_with_args("~0", "script.xcl", &[]);
        assert_eq!(result.as_i48(), -1);
    }

    #[test]
    fn test_pow_basic() {
        let result = eval_with_args("2 ** 10", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 1024);
    }

    #[test]
    fn test_pow_negative_exp() {
        let result = eval_with_args("2 ** -1", "script.xcl", &[]);
        assert!((result.as_f64() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pow_right_associative() {
        // 2 ** 3 ** 2 = 2 ** 9 = 512
        let result = eval_with_args("2 ** 3 ** 2", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 512);
    }

    #[test]
    fn test_pow_mixed_float_operands() {
        let result = eval_with_args("2.0 ** 3", "script.xcl", &[]);
        assert!(result.is_float());
        assert!((result.as_f64() - 8.0).abs() < f64::EPSILON);

        let result = eval_with_args("2 ** 3.0", "script.xcl", &[]);
        assert!(result.is_float());
        assert!((result.as_f64() - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pow_i48_boundary() {
        // 2^40 fits in the 48-bit signed payload.
        let result = eval_with_args("2 ** 40", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 1_099_511_627_776);

        // 2^47 does not fit; it must fall back to float.
        let result = eval_with_args("2 ** 47", "script.xcl", &[]);
        assert!(result.is_float());
        assert!((result.as_f64() - 140_737_488_355_328.0).abs() < 1.0);
    }

    #[test]
    fn test_precedence_bitor_bitand() {
        // 1 | 2 & 3 → 1 | (2 & 3) → 1 | 2 → 3
        let result = eval_with_args("1 | 2 & 3", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 3);
    }

    #[test]
    fn test_precedence_bitxor_bitand() {
        // 1 ^ 2 & 3 → 1 ^ (2 & 3) → 1 ^ 2 → 3
        let result = eval_with_args("1 ^ 2 & 3", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 3);
    }

    #[test]
    fn test_precedence_bitor_bitxor() {
        // 1 | 2 ^ 3 → (1 | 2) ^ 3 → 3 ^ 3 → 0... wait, task says 1
        // Actually: | binds looser than ^, so 1 | 2 ^ 3 → 1 | (2 ^ 3) → 1 | 1 → 1
        let result = eval_with_args("1 | 2 ^ 3", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 1);
    }

    #[test]
    fn test_compound_bitor() {
        let result = eval_with_args("$x = 5; $x |= 3; $x", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 7);
    }

    #[test]
    fn test_compound_bitxor() {
        let result = eval_with_args("$x = 5; $x ^= 3; $x", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 6);
    }

    #[test]
    fn test_compound_pow() {
        let result = eval_with_args("$x = 3; $x **= 2; $x", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 9);
    }

    #[test]
    fn test_conditional_true() {
        let result = eval_with_args("1 if true else 2", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 1);
    }

    #[test]
    fn test_conditional_false() {
        let result = eval_with_args("1 if false else 2", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 2);
    }

    #[test]
    fn test_conditional_right_associative() {
        let result = eval_with_args("1 if false else 2 if true else 3", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 2);
    }

    #[test]
    fn test_conditional_string_example() {
        let result = eval_with_args(
            r#"$num = 6; $label = "WEEKEND!" if $num > 5 else "Workday"; $label"#,
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_str(), Some("WEEKEND!"));
    }

    #[test]
    fn test_if_statement_not_swallowed_by_conditional() {
        // The newline before `if` must keep it as a statement, not a ternary.
        let src = "$x = 1\nif true { $x = 2 }\n$x";
        let result = eval_with_args(src, "script.xcl", &[]);
        assert_eq!(result.as_i48(), 2);
    }

    #[test]
    fn test_lambda_body_bitor() {
        // $f = |$x| $x | 1; $f 2 → 3
        let result = eval_with_args("$f = |$x| $x | 1; $f 2", "script.xcl", &[]);
        assert_eq!(result.as_i48(), 3);
    }

    #[test]
    fn test_logical_or_still_works() {
        let result = eval_with_args("false || true", "script.xcl", &[]);
        assert!(result.as_bool());
    }

    #[test]
    fn test_sum_type_parsing() {
        // Shape = A | B should produce a TypeDef, not a binary expression
        let src = "Shape = A | B";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            crate::compiler::ast::AstNode::TypeDef { name, .. } => {
                assert_eq!(name, "Shape");
            }
            other => panic!("Expected TypeDef, got {:?}", other),
        }
    }

    #[test]
    fn test_while_loop_sum() {
        let result = eval_with_args(
            "$s = 0; $i = 0; while $i < 5 { $s = $s + $i; $i = $i + 1 }; $s",
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_i48(), 10);
    }

    #[test]
    fn test_for_number_loop() {
        let result = eval_with_args(
            "$s = 0; for $i in 0..10 { $s = $s + $i }; $s",
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_i48(), 45);
    }

    #[test]
    fn test_named_call_in_while() {
        let result = eval_with_args(
            "inc($x) { return $x + 1 }; $i = 0; $r = 0; while $i < 5 { $r = inc($r); $i = $i + 1 }; $r",
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_i48(), 5);
    }

    #[test]
    fn test_parenthesized_lambda_arg() {
        // f (|$x| $x) should parse as a call with a lambda argument
        let src = "f (|$x| $x)";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            crate::compiler::ast::AstNode::Call { callee, args, .. } => {
                match callee.as_ref() {
                    crate::compiler::ast::AstNode::Ident { name, .. } => {
                        assert_eq!(name, "f");
                    }
                    other => panic!("Expected Ident callee, got {:?}", other),
                }
                assert_eq!(args.len(), 1);
                match &args[0].value {
                    crate::compiler::ast::AstNode::Lambda { .. } => {}
                    other => panic!("Expected Lambda arg, got {:?}", other),
                }
            }
            other => panic!("Expected Call, got {:?}", other),
        }
    }

    // ── String concat fast path tests ──

    #[test]
    fn test_string_concat_two_strings() {
        let result = eval_with_args(r#""hello" + "world""#, "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("helloworld"));
    }

    #[test]
    fn test_string_concat_int_and_string() {
        let result = eval_with_args(r#"1 + "x""#, "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("1x"));
    }

    #[test]
    fn test_string_concat_string_and_int() {
        let result = eval_with_args(r#""x" + 1"#, "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("x1"));
    }

    #[test]
    fn test_string_concat_loop() {
        // Concatenate in a loop to exercise the fast path repeatedly
        let result = eval_with_args(
            "$s = \"\"; for $i in 0..100 { $s = $s + \"a\" }; $s",
            "script.xcl",
            &[],
        );
        let s = result.as_str().expect("concat loop should return string");
        assert_eq!(s.len(), 100);
        assert!(s.chars().all(|c| c == 'a'));
    }

    // ── Map lookup fast path tests ──

    #[test]
    fn test_map_field_read() {
        let result = eval_with_args(
            r#"$m = {name: "Alice", age: 30}; $m.name"#,
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_str(), Some("Alice"));
    }

    #[test]
    fn test_map_index_read() {
        let result = eval_with_args(
            r#"$m = {name: "Bob", age: 25}; $m["name"]"#,
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_str(), Some("Bob"));
    }

    #[test]
    fn test_map_field_read_loop() {
        // Read a field in a loop to exercise the fast path
        let result = eval_with_args(
            r#"$m = {x: 42}; $s = 0; for $i in 0..1000 { $s = $s + $m.x }; $s"#,
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_i48(), 42000);
    }

    #[test]
    fn test_map_create_and_read() {
        let result = eval_with_args(
            r#"$m = {a: 1, b: 2, c: 3}; $m.a + $m.b + $m.c"#,
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_i48(), 6);
    }

    // ── String representation tests (arena bytes, cached hash) ──

    #[test]
    fn test_string_equality_across_constructions() {
        // Strings built via from_str, from_str_boxed, and concatenation
        // with the same bytes should compare equal.
        let boxed = Value::from_str_boxed("hello".to_string());
        let arena = bumpalo::Bump::new();
        let arena_str = Value::from_str("hello".to_string(), &arena);
        assert_eq!(boxed.as_str(), Some("hello"));
        assert_eq!(arena_str.as_str(), Some("hello"));
        // Equality via Value::eq_
        assert!(Value::eq_(boxed, boxed));
        assert!(Value::eq_(arena_str, arena_str));
        // Cross-construction: same bytes, different pointers
        assert_eq!(boxed.as_str(), arena_str.as_str());
        core::mem::forget(arena);
    }

    #[test]
    fn test_str_hash_consistency() {
        let boxed = Value::from_str_boxed("test-key".to_string());
        let arena = bumpalo::Bump::new();
        let arena_str = Value::from_str("test-key".to_string(), &arena);

        // Both should have the same cached hash
        let h1 = boxed.str_hash().expect("boxed string should have hash");
        let h2 = arena_str.str_hash().expect("arena string should have hash");
        assert_eq!(h1, h2, "same bytes must produce same cached hash");

        // Non-string values return None
        assert!(Value::from_i48(42).str_hash().is_none());
        assert!(Value::nil().str_hash().is_none());
        assert!(Value::bool(true).str_hash().is_none());
        core::mem::forget(arena);
    }

    #[test]
    fn test_concatenation_produces_correct_value() {
        let result = eval_with_args(r#""hello" + " world""#, "script.xcl", &[]);
        assert_eq!(result.as_str(), Some("hello world"));
    }

    #[test]
    fn test_concatenation_loop_correctness() {
        // 3-part concatenation in a loop stays correct
        let result = eval_with_args(
            r#"$s = ""; for $i in 0..50 { $s = $s + "a" + "b" }; $s"#,
            "script.xcl",
            &[],
        );
        let s = result.as_str().expect("concat loop should return string");
        assert_eq!(s.len(), 100);
        for c in s.chars() {
            assert!(c == 'a' || c == 'b');
        }
    }

    #[test]
    fn test_map_with_runtime_key() {
        // A map keyed by a string built at runtime exercises the cached-hash
        // path with a key that is not the compiler's interned constant.
        // We build the key via concatenation, then use it for lookup.
        let result = eval_with_args(
            r#"$k = "x" + "y"; $m = {xy: 42}; $m[$k]"#,
            "script.xcl",
            &[],
        );
        assert_eq!(result.as_i48(), 42);
    }

    // ── Item 2 + Item 6: Memory-stability test ──

    #[test]
    fn test_memory_stability_500_runs() {
        // Run the same compiled program 500 times on one VM and assert that
        // globals.len(), global_index.len(), and frames.len() after run 500
        // equal their values after run 2.
        let src = r#"
            f($n) {
                if $n <= 0 { return 0 }
                return f($n - 1) + 1
            }
            $a = [1, 2, 3]
            $a = $a + [4]
            f(10)
        "#;
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let mut vm = VM::new();

        // Sample the state after the second run on the *same* VM. Anything that
        // grows per run (globals, function registrations, frames) shows up here.
        let mut after_second: Option<(usize, usize, usize)> = None;
        for run in 1..=500 {
            let _ = vm.run(&bc);
            if run == 2 {
                after_second = Some((vm.globals.len(), vm.global_index.len(), vm.frames.len()));
            }
        }
        let (globals_2, global_index_2, frames_2) = after_second.expect("two runs");

        assert_eq!(
            vm.globals.len(),
            globals_2,
            "globals.len() after 500 runs ({}) must equal after 2 runs ({})",
            vm.globals.len(),
            globals_2
        );
        assert_eq!(
            vm.global_index.len(),
            global_index_2,
            "global_index.len() after 500 runs ({}) must equal after 2 runs ({})",
            vm.global_index.len(),
            global_index_2
        );
        assert_eq!(
            vm.frames.len(),
            frames_2,
            "frames.len() after 500 runs ({}) must equal after 2 runs ({})",
            vm.frames.len(),
            frames_2
        );
        assert!(
            vm.context_stack.is_empty(),
            "context_stack must be empty after repeated runs"
        );
    }

    // ── Tier 2: frame code pointer tests ──

    #[test]
    fn frame_code_pointer_survives_repeated_runs() {
        // Compile three different programs and run them round-robin 50 times
        // each on one VM to catch stale pointer issues.
        let src_recursion = "f($n) { if $n <= 0 { return 1 } return f($n - 1) * 2 }; f(10)";
        let src_closure = "$f = |$x| $x * 3; $f 7";
        let src_tail = "ping($n) { if $n <= 0 { return 7 } return pong ($n - 1) }; pong($n) { if $n <= 0 { return 9 } return ping ($n - 1) }; ping 5";

        let tokens1 = lex(src_recursion, "<test>");
        let ast1 = parse(&tokens1, src_recursion);
        let bc1 = compile(&ast1);

        let tokens2 = lex(src_closure, "<test>");
        let ast2 = parse(&tokens2, src_closure);
        let bc2 = compile(&ast2);

        let tokens3 = lex(src_tail, "<test>");
        let ast3 = parse(&tokens3, src_tail);
        let bc3 = compile(&ast3);

        let mut vm = VM::new();
        for _ in 0..50 {
            assert_eq!(vm.run(&bc1).as_i48(), 1024, "recursion failed");
            assert_eq!(vm.run(&bc2).as_i48(), 21, "closure failed");
            assert_eq!(vm.run(&bc3).as_i48(), 9, "tail call failed");
        }
    }

    #[test]
    fn tail_call_across_bytecodes_after_run() {
        // Run a tail-call program, then a different program, then the tail-call
        // program again on the same VM.
        let src_tail = "ping($n) { if $n <= 0 { return 7 } return pong ($n - 1) }; pong($n) { if $n <= 0 { return 9 } return ping ($n - 1) }; ping 4";
        let src_other = "$x = 100; $x + 1";

        let tokens1 = lex(src_tail, "<test>");
        let ast1 = parse(&tokens1, src_tail);
        let bc1 = compile(&ast1);

        let tokens2 = lex(src_other, "<test>");
        let ast2 = parse(&tokens2, src_other);
        let bc2 = compile(&ast2);

        let mut vm = VM::new();
        assert_eq!(vm.run(&bc1).as_i48(), 7, "first tail call");
        assert_eq!(vm.run(&bc2).as_i48(), 101, "other program");
        assert_eq!(vm.run(&bc1).as_i48(), 7, "tail call again");
    }

    #[test]
    fn call_value_after_run() {
        // Keep a function Value returned by run, call it via call_value after
        // the VM has run a different program.
        let src1 = "$f = |$x| $x + 10; $f";
        let src2 = "42 * 2";

        let tokens1 = lex(src1, "<test>");
        let ast1 = parse(&tokens1, src1);
        let bc1 = compile(&ast1);

        let tokens2 = lex(src2, "<test>");
        let ast2 = parse(&tokens2, src2);
        let bc2 = compile(&ast2);

        let mut vm = VM::new();
        let func = vm.run(&bc1);
        assert!(func.as_func().is_some(), "expected function from first run");

        // Run a different program
        assert_eq!(vm.run(&bc2).as_i48(), 84, "different program");

        // Call the saved function via call_value
        let result = vm.call_value(func, &[Value::from_i48(5)]);
        assert_eq!(result.as_i48(), 15, "call_value after run");
    }

    #[test]
    fn vm_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<VM>();
    }

    // ── JIT (feature `jit`) ──

    #[cfg(feature = "jit")]
    #[test]
    fn jit_compiles_integer_loops_and_matches_an_oracle() {
        // Counting loop: the JIT must produce the same value as the oracle.
        let src = "$i = 0\n$s = 0\nwhile $i < 1000 {\n    $s = $s + $i\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        assert!(prepare_jit(&bc).is_some(), "counting loop should compile");
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 499_500);

        // Mock LCG: compiler output checked against a Rust oracle.
        let src = "$x = 42\n$i = 0\nwhile $i < 100000 {\n    $x = ($x * 37 + 12345) % 1000003\n    $i = $i + 1\n}\n$x";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        assert!(prepare_jit(&bc).is_some(), "lcg loop should compile");
        let mut expect = 42i64;
        for _ in 0..100_000 {
            expect = (expect * 37 + 12_345) % 1_000_003;
        }
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), expect);

        // Bitwise loop: shifts, or, and.
        let src = "$x = 12345\n$i = 0\nwhile $i < 200000 {\n    $x = (($x << 5) | ($x >> 3)) & 16777215\n    $x = ($x + $i) & 16777215\n    $i = $i + 1\n}\n$x";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        assert!(prepare_jit(&bc).is_some(), "bitwise loop should compile");
        let mut x = 12345i64;
        let mut i = 0i64;
        while i < 200_000 {
            x = ((x << 5) | (x >> 3)) & 16_777_215;
            x = (x + i) & 16_777_215;
            i += 1;
        }
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), x);
    }

    #[cfg(feature = "jit")]
    #[test]
    fn jit_compiles_every_loop_in_a_unit() {
        let src = "$a = 0\n$i = 0\nwhile $i < 100 {\n    $a = $a + $i\n    $i = $i + 1\n}\n$b = 0\n$j = 0\nwhile $j < 100 {\n    $b = $b + 1\n    $j = $j + 1\n}\n$a + $b";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let unit = prepare_jit(&bc).expect("both loops compile");
        assert_eq!(unit.entries.len(), 2, "expected two compiled loops");
        assert_eq!(
            unit.shadow.iter().filter(|&&s| s != 0).count(),
            2,
            "both headers must be in the shadow table"
        );
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 4950 + 100);
    }

    #[cfg(feature = "jit")]
    #[test]
    fn jit_compiles_array_index_loops() {
        let src = "$a = [10,20,30,40,50]\n$i = 0\n$s = 0\nwhile $i < 5 {\n    $s = $s + $a[$i]\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let unit = prepare_jit(&bc).expect("array loop compiles");
        assert_eq!(unit.entries.len(), 1);
        assert_eq!(unit.entries[0].access_len, 1, "one array access site");
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 150);

        // Negative indices are normalised exactly like the interpreter.
        let src = "$a = [10,20,30]\n$i = -3\n$s = 0\nwhile $i < 0 {\n    $s = $s + $a[$i]\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 60);
    }

    #[cfg(feature = "jit")]
    #[test]
    fn jit_falls_back_when_the_container_is_not_an_array() {
        // The loop is structurally compilable, but the local is a map. The
        // cache fill must fail and the interpreter must produce the result.
        let src = "$m = { a: 1, b: 2 }\n$i = 0\n$s = 0\nwhile $i < 10 {\n    $s = $s + $m[\"a\"]\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 10);
    }

    #[cfg(feature = "jit")]
    #[test]
    fn jit_compiles_map_field_loops() {
        let src = "$m = { a: 1, b: 2, c: 3 }\n$i = 0\n$s = 0\nwhile $i < 10 {\n    $s = $s + $m.a\n    $s = $s + $m.b\n    $s = $s + $m.c\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let unit = prepare_jit(&bc).expect("map loop compiles");
        assert_eq!(unit.entries.len(), 1);
        assert_eq!(unit.entries[0].access_len, 3, "three field reads");
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 60);

        // A non-integer field makes the cache fill fail; the interpreter must
        // still produce the right value.
        let src = "$m = { a: 1.5 }\n$i = 0\n$s = 0\nwhile $i < 10 {\n    $s = $s + $m.a\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_f64(), 15.0);
    }

    #[cfg(feature = "jit")]
    #[test]
    fn jit_rejects_non_integer_loops() {
        // A float multiplication inside the body: outside the integer subset.
        let src = "$x = 1\n$i = 0\nwhile $i < 100 {\n    $x = $x * 1.5\n    $i = $i + 1\n}\n$x";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        assert!(prepare_jit(&bc).is_none(), "float loop must not compile");
        let mut vm = VM::new();
        assert!(vm.run(&bc).is_float());

        // A `for` loop is not a `while` header.
        let src = "$s = 0\nfor $i in 0..100 {\n    $s = $s + $i\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        assert!(prepare_jit(&bc).is_none(), "range loop must not compile");
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 4950);
    }

    #[cfg(feature = "jit")]
    #[test]
    fn jit_bails_out_without_committing_state() {
        // The counter starts as a float: the entry guard must bail and leave
        // the register file untouched for the interpreter.
        let src = "$i = 1.5\n$s = 0\nwhile $i < 5 {\n    $s = $s + 1\n    $i = $i + 1\n}\n$s";
        let tokens = lex(src, "<test>");
        let ast = parse(&tokens, src);
        let bc = compile(&ast);
        assert!(
            prepare_jit(&bc).is_some(),
            "loop is compilable; guard decides"
        );
        let mut vm = VM::new();
        assert_eq!(vm.run(&bc).as_i48(), 4);
    }
}
