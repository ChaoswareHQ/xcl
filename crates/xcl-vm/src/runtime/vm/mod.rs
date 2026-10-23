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

/// Script name used by `$0` when none was supplied.
pub const DEFAULT_SCRIPT_NAME: &str = "<input>";

/// Number of entries in the module-space opcode table.
const MODULE_COUNT: usize = crate::runtime::opcode::layout::MODULE_COUNT;

// Thread-local bump arena pool for reusing memory across VM instances.
// Holds at most one `Bump` per thread. When a `VM` is dropped, its arena is
// reset and returned to this pool (if small enough) so the next `VM::new()`
// reuses the same pages instead of faulting in fresh memory.
#[cfg(feature = "std")]
std::thread_local! {
    static ARENA_POOL: core::cell::RefCell<Option<bumpalo::Bump>> =
        const { core::cell::RefCell::new(None) };
}

/// Container buffers recycled with the arena.
///
/// A `VM` owns four heap buffers that are empty or near-empty in a short run:
/// the operand stack, the shared register file, the frame stack, and the
/// global table. Creating and dropping them costs four allocator round trips
/// per VM, which dominates programs whose whole run is a handful of calls.
/// They are returned to a thread-local slot on `Drop` and taken back by the
/// next `VM::new()` on the same thread, exactly like [`ARENA_POOL`].
#[cfg(feature = "std")]
struct VmBuffers {
    stack: Vec<Value>,
    regs: Vec<Value>,
    frames: Vec<Frame>,
    globals: Vec<Value>,
}

#[cfg(feature = "std")]
std::thread_local! {
    static BUFFER_POOL: core::cell::RefCell<Option<VmBuffers>> =
        const { core::cell::RefCell::new(None) };
}

/// Largest register/stack capacity worth keeping alive between VMs.
///
/// 8K values is 128 KiB per buffer; a program that needed more is re-allocated
/// rather than pinning a large block on the thread forever.
#[cfg(feature = "std")]
const POOL_MAX_VALUES: usize = 8192;
/// Largest frame stack worth keeping alive between VMs (`Frame` is ~144 B).
#[cfg(feature = "std")]
const POOL_MAX_FRAMES: usize = 256;

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
    /// `Frame` ~240 bytes, and every call reinitializes all three. Iterating the
    /// live heap array instead of a snapshot was measured ~38% slower on
    /// `array_for_sum_100x100` (one extra pointer chase per element beats one
    /// small copy on entry), so the copy stays.
    iter: Vec<Value>,
    iter_idx: u32,
    /// True when `iter` is a numeric range rather than an array; the bounds live
    /// in `range_cur`/`range_end`.
    iter_is_range: bool,
    range_cur: i64,
    range_end: i64,
    stack_base: u32,
    /// Deferred closures to run when this frame returns (LIFO order).
    defer_stack: Vec<Value>,
    /// First call argument, always stored: `$self` and `$super` need it even
    /// when the body never asks for `$1`..`$#`, so those functions no longer
    /// allocate an `argv` vector per call.
    arg0: Value,
    /// Positional arguments passed into this frame (`$1`, `$#`, `$@`, `$*`).
    ///
    /// Filled only when the body actually uses one of those forms; `$self`,
    /// `$super`, and `$0` read `arg0` / the script name instead.
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

/// The plan passed to the dispatch loop.
#[cfg(feature = "jit")]
type JitPlan<'a> = Option<&'a mut JitUnit>;
/// Without the feature, the dispatch loop has no plan at all.
#[cfg(not(feature = "jit"))]
type JitPlan<'a> = Option<&'a mut ()>;

/// A domain path owned by a plugin.
///
/// Domains register under a flat key (`"util"`) because that is what
/// `NativeEntry::domain` carries. A plugin may publish the same domain under a
/// qualified path (`"std.util"`) so callers can write either spelling; the VM
/// translates the path to the key before dispatch.
#[derive(Clone, Copy, Debug)]
pub struct DomainAlias {
    /// Fully-qualified path, e.g. `"std.util"`.
    pub path: &'static str,
    /// Registered domain key the natives were declared under, e.g. `"util"`.
    pub domain: &'static str,
}

/// A plugin as the VM knows it.
///
/// Registration is static (`linkme`), so a plugin never appears by accident: it
/// lists its own descriptor and the domains it owns. The VM keeps the table so a
/// host can answer "which plugin provides this domain?" without importing the
/// plugin crate, and so qualified paths (`std.util.print`) resolve.
#[derive(Debug)]
pub struct PluginDescriptor {
    /// Plugin name, e.g. `"std"`.
    pub name: &'static str,
    /// One-line description, used by docs and tooling.
    pub summary: &'static str,
    /// Domain paths this plugin publishes.
    pub domains: &'static [DomainAlias],
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
    /// Registered plugins, in registration order.
    plugin_table: Vec<&'static PluginDescriptor>,
    /// `"std.util"` → `"util"`, built from every registered plugin's domains.
    domain_alias: HashMap<&'static str, &'static str>,
    /// Registry mapping type names to static type descriptors.
    pub type_index: HashMap<Box<str>, &'static TypeDescriptor>,
    /// Registry mapping trait names to static trait descriptors.
    pub trait_index: HashMap<Box<str>, &'static TraitDescriptor>,
    /// Flat list of all registered trait implementation descriptors.
    pub impl_index: Vec<&'static ImplDescriptor>,
    /// Flat list of registered native entries (for override_by_trait lookups).
    pub native_entries: Vec<&'static NativeEntry>,
    /// Hot natives indexed by the operand of `OP_CALL_HOT`.
    hot_natives: Vec<HotNative>,
    /// Callable spelling → hot slot, consulted by the compiler.
    hot_table: HotTable,
    /// Flat list of registered type descriptors (used by `$super` lookups).
    pub type_entries: Vec<&'static TypeDescriptor>,
    /// Fast bump allocator arena for temporary heap objects created during execution.
    pub arena: Bump,
    pub(crate) frames: Vec<Frame>,
    /// Stack-based context for domain blocks (e.g., `browser "brave" { ... }`).
    /// Each key maps to a stack of values, enabling nested domain contexts.
    pub(crate) context_stack: HashMap<String, Vec<Value>>,
    /// Dispatch table for extension opcodes (module slots `128..=255`),
    /// allocated on the first [`VM::register_opcodes`] call.
    ///
    /// `None` means "no plugin opcodes registered", which is the case for every
    /// core-only program. Keeping the 1 KiB table out of the `VM` value matters:
    /// a VM that allocates one still has to be zeroed and moved, and runtimes
    /// that create a VM per script end up paying for it on every run.
    opcode_table: Option<
        Box<[Option<super::opcode::OpcodeFn>; crate::runtime::opcode::layout::MODULE_COUNT]>,
    >,
    /// Cold plugin modules reachable through `OP_EXT`, sorted by id.
    cold_modules: Vec<(u32, &'static super::opcode::ColdModule)>,
    #[cfg(feature = "opcode-stats")]
    opcode_counter: OpcodeCounter,
    /// Executed instructions as `(ip, frame depth, opcode)`, in order.
    ///
    /// Recording is opt-in (`trace` feature) so normal builds pay nothing. It
    /// exists for the playground, which replays the list to show a program
    /// running one instruction at a time.
    #[cfg(feature = "trace")]
    trace: Vec<(u32, u8, u8)>,
    /// Name of the currently running script (`$0`).
    ///
    /// `None` means the default `"<input>"`; storing it lazily avoids one boxed
    /// allocation per VM.
    script_name: Option<Box<str>>,
    /// Cached `$0` value for the current run (arena-backed).
    ///
    /// Built on the first `$0` read and reset to nil by `run`, so a program
    /// that never mentions `$0` leaves the arena untouched.
    script_name_value: Value,
    /// Top-level positional script arguments.
    script_args: Vec<Value>,
    /// Direct-mapped [`CALL_CACHE_SLOTS`]-entry callee cache for fast
    /// recursive call dispatch.
    call_cache: [(Value, Value); CALL_CACHE_SLOTS],
    /// Bitmask of [`call_cache`](Self::call_cache) slots written since the last
    /// `run`.
    ///
    /// Only the used slots are cleared at the start of a run. The cache is 256
    /// bytes, and a fresh VM's is already all-nil, so clearing it
    /// unconditionally was pure per-run memory traffic.
    call_cache_dirty: u32,
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
        // Reuse the previous VM's arena on this thread, if one is pooled.
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
        // Reuse the previous VM's containers when one is available.
        #[cfg(feature = "std")]
        let (stack, regs, frames, globals) =
            BUFFER_POOL.with(|pool| match pool.borrow_mut().take() {
                Some(buffers) => (buffers.stack, buffers.regs, buffers.frames, buffers.globals),
                None => (
                    Vec::with_capacity(16),
                    Vec::with_capacity(16),
                    Vec::with_capacity(16),
                    Vec::new(),
                ),
            });
        #[cfg(not(feature = "std"))]
        let (stack, regs, frames, globals) = (
            Vec::with_capacity(16),
            Vec::with_capacity(16),
            Vec::with_capacity(16),
            Vec::new(),
        );
        Self {
            stack,
            regs,
            globals,
            global_index: Default::default(),
            global_table: Vec::new(),
            domain_index: Default::default(),
            domain_table: Vec::new(),
            plugin_table: Vec::new(),
            domain_alias: Default::default(),
            type_index: Default::default(),
            trait_index: Default::default(),
            impl_index: Vec::new(),
            native_entries: Vec::new(),
            hot_natives: Vec::new(),
            hot_table: HotTable::default(),
            type_entries: Vec::new(),
            arena,
            frames,
            context_stack: HashMap::new(),
            opcode_table: None,
            cold_modules: Vec::new(),
            #[cfg(feature = "opcode-stats")]
            opcode_counter: OpcodeCounter::new(),
            #[cfg(feature = "trace")]
            trace: Vec::new(),
            script_name: None,
            script_name_value: Value::nil(),
            script_args: Vec::new(),
            call_cache: [(Value::nil(), Value::nil()); CALL_CACHE_SLOTS],
            call_cache_dirty: 0,
            #[cfg(feature = "jit")]
            jit_functions: HashMap::new(),
            #[cfg(feature = "jit")]
            jit_call_cache: [None; CALL_CACHE_SLOTS],
            frame_depth: 0,
            installed_code: None,
            function_base: 0,
        }
    }

    /// Sets the script name (`$0`) and top-level positional arguments (`$1..$N`).
    ///
    /// Call this before [`VM::run`]. Script arguments apply to the root frame;
    /// function frames use their own call arguments.
    pub fn set_script_args(&mut self, name: &str, args: Vec<Value>) {
        self.script_name = Some(name.into());
        self.script_args = args;
    }

    /// Script name reported by `$0` (`"<input>"` until [`VM::set_script_args`]).
    #[inline]
    pub fn script_name(&self) -> &str {
        self.script_name.as_deref().unwrap_or(DEFAULT_SCRIPT_NAME)
    }

    /// Registers native functions from a static slice.
    ///
    /// Every entry flagged `hot` is also assigned a slot in [`VM::hot_natives`]
    /// once the slice has been added, so the compiler can emit `OP_CALL_HOT` for
    /// its call sites.
    pub fn register_natives(&mut self, entries: &'static [NativeEntry]) {
        self.register_natives_filtered(entries, |_| true);
    }

    /// Registers only the native entries accepted by `keep`.
    ///
    /// Feature installation uses this to expose a subset of the compiled
    /// standard library without changing the static registries.
    pub fn register_natives_filtered(
        &mut self,
        entries: &'static [NativeEntry],
        keep: impl Fn(&NativeEntry) -> bool,
    ) {
        for entry in entries {
            if keep(entry) {
                self.register_native(entry);
            }
        }
        self.rebuild_hot_table();
    }

    fn register_native(&mut self, entry: &'static NativeEntry) {
        let impl_ = native_impl(entry);

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

    /// Registers plugin descriptors from a static slice.
    ///
    /// A plugin descriptor says which domains the plugin owns and under which
    /// path they are published (`std.util` → `util`). `register_natives` builds
    /// the dispatch slots from the flat keys, so either call order works.
    pub fn register_plugins(&mut self, entries: &'static [PluginDescriptor]) {
        self.register_plugins_filtered(entries, |_| true);
    }

    /// Registers only the plugin descriptors accepted by `keep`.
    pub fn register_plugins_filtered(
        &mut self,
        entries: &'static [PluginDescriptor],
        keep: impl Fn(&PluginDescriptor) -> bool,
    ) {
        for plugin in entries {
            if !keep(plugin) {
                continue;
            }
            if self
                .plugin_table
                .iter()
                .any(|existing| existing.name == plugin.name)
            {
                continue;
            }
            for alias in plugin.domains {
                self.domain_alias.insert(alias.path, alias.domain);
            }
            self.plugin_table.push(plugin);
        }
    }

    /// Registered plugins, in registration order.
    pub fn plugins(&self) -> &[&'static PluginDescriptor] {
        &self.plugin_table
    }

    /// Plugin that published `domain`, accepting its flat key or its path.
    pub fn plugin_of(&self, domain: &str) -> Option<&'static str> {
        let key = self.domain_alias.get(domain).copied().unwrap_or(domain);
        self.plugin_table
            .iter()
            .find(|plugin| plugin.domains.iter().any(|d| d.domain == key))
            .map(|plugin| plugin.name)
    }

    /// Registers type descriptors from a static slice.
    pub fn register_types(&mut self, entries: &'static [TypeDescriptor]) {
        self.register_types_filtered(entries, |_| true);
    }

    /// Registers only the type descriptors accepted by `keep`.
    pub fn register_types_filtered(
        &mut self,
        entries: &'static [TypeDescriptor],
        keep: impl Fn(&TypeDescriptor) -> bool,
    ) {
        for desc in entries {
            if !keep(desc) {
                continue;
            }
            self.type_index.insert(desc.name.into(), desc);
            self.type_entries.push(desc);
        }
    }

    /// Registers trait descriptors from a static slice.
    pub fn register_traits(&mut self, entries: &'static [TraitDescriptor]) {
        self.register_traits_filtered(entries, |_| true);
    }

    /// Registers only the trait descriptors accepted by `keep`.
    pub fn register_traits_filtered(
        &mut self,
        entries: &'static [TraitDescriptor],
        keep: impl Fn(&TraitDescriptor) -> bool,
    ) {
        for desc in entries {
            if !keep(desc) {
                continue;
            }
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
    /// Registers extension opcode handlers from a static slice.
    ///
    /// Only module-space codes (`128..=255`) are accepted: the core range is
    /// claimed by `dispatch_core` and `OP_EXT`, and a handler placed there would
    /// either be shadowed or silently take over a reserved slot.
    ///
    /// # Panics
    ///
    /// Panics when an entry uses a code below 128. Duplicate codes across crates
    /// are caught earlier, in `xcl_plugins::register_all`, which knows the std and
    /// plugin slices separately.
    pub fn register_opcodes(&mut self, entries: &'static [super::opcode::OpcodeEntry]) {
        let table = self
            .opcode_table
            .get_or_insert_with(|| Box::new([None; MODULE_COUNT]));
        for entry in entries.iter() {
            assert!(
                crate::runtime::opcode::layout::is_module_slot(entry.code),
                "opcode `{}` uses code {} — extension opcodes must be in 128..=255; \
                 codes below 128 belong to the core (and 127 is OP_EXT)",
                entry.name,
                entry.code
            );
            table[crate::runtime::opcode::layout::module_index(entry.code) as usize] =
                Some(entry.handler);
        }
    }

    /// Registers cold plugin modules reachable through `OP_EXT`.
    ///
    /// Kept sorted by id so the escape path can binary-search. Registration is a
    /// startup cost; the table never changes afterwards.
    ///
    /// # Panics
    ///
    /// Panics on a duplicate module id, or on two instructions sharing a local
    /// opcode inside one module.
    pub fn register_cold_modules(&mut self, entries: &'static [super::opcode::ColdModule]) {
        for module in entries.iter() {
            for (i, op) in module.ops.iter().enumerate() {
                assert!(
                    module.ops[..i].iter().all(|prev| prev.code != op.code),
                    "cold module `{}` (id {}) declares opcode {} twice: `{}` collides with an earlier entry",
                    module.name,
                    module.id,
                    op.code,
                    op.name
                );
            }
            self.cold_modules.push((module.id, module));
        }
        self.cold_modules.sort_unstable_by_key(|(id, _)| *id);
        for pair in self.cold_modules.windows(2) {
            assert!(
                pair[0].0 != pair[1].0,
                "cold module id {} is claimed by both `{}` and `{}`",
                pair[0].0,
                pair[0].1.name,
                pair[1].1.name
            );
        }
    }

    /// Runs the instruction `local` of cold module `module_id`.
    ///
    /// Called by [`OP_EXT`](crate::runtime::opcode::layout::OP_EXT) only, so the
    /// binary search is not on any hot path.
    #[inline]
    pub(crate) fn invoke_cold_opcode(
        &mut self,
        module_id: u32,
        local: u8,
        top: usize,
        instr: Instr,
    ) -> Action {
        let Ok(idx) = self
            .cold_modules
            .binary_search_by_key(&module_id, |(id, _)| *id)
        else {
            return Action::Next;
        };
        let module = self.cold_modules[idx].1;
        match module.ops.iter().find(|op| op.code == local) {
            Some(op) => (op.handler)(self, top, instr),
            None => Action::Next,
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
        // A plugin may publish its domain under a qualified path (`std.util`);
        // translate it to the flat key the natives were registered under.
        let domain = self.domain_alias.get(domain).copied().unwrap_or(domain);
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
        self.call_cache_dirty |= 1 << slot;
        self.jit_call_cache[slot] = func
            .as_func()
            .and_then(|f| self.jit_functions.get(&*f.name).copied());
    }

    /// Without the JIT feature only the interpreter cache exists.
    #[cfg(not(feature = "jit"))]
    #[inline(always)]
    pub(crate) fn cache_callee(&mut self, slot: usize, callee: Value, func: Value) {
        self.call_cache[slot] = (callee, func);
        self.call_cache_dirty |= 1 << slot;
    }

    /// Upper bound on recorded instructions, so a runaway program cannot grow
    /// the trace without limit. The playground is the only consumer.
    #[cfg(feature = "trace")]
    const TRACE_CAP: usize = 200_000;
}

/// Return the bump arena and the container buffers to the thread-local
/// pools so the next VM reuses the same pages and allocations.
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

            // Recycle the containers too. Oversized buffers are dropped so one
            // huge run cannot pin memory for the life of the thread.
            if self.stack.capacity() <= POOL_MAX_VALUES
                && self.regs.capacity() <= POOL_MAX_VALUES
                && self.globals.capacity() <= POOL_MAX_VALUES
                && self.frames.capacity() <= POOL_MAX_FRAMES
            {
                let mut stack = core::mem::take(&mut self.stack);
                let mut regs = core::mem::take(&mut self.regs);
                let mut globals = core::mem::take(&mut self.globals);
                let mut frames = core::mem::take(&mut self.frames);
                stack.clear();
                regs.clear();
                globals.clear();
                for frame in frames.iter_mut() {
                    // `iter` snapshots an array, so it can be as large as the
                    // array. Keep the small per-depth buffer that `for` reuses,
                    // drop anything that looks like a real payload.
                    if frame.iter.capacity() > 1024 {
                        frame.iter = Vec::new();
                    } else {
                        frame.iter.clear();
                    }
                    frame.defer_stack = Vec::new();
                    frame.argv = Vec::new();
                }
                BUFFER_POOL.with(|pool| {
                    pool.borrow_mut().replace(VmBuffers {
                        stack,
                        regs,
                        frames,
                        globals,
                    });
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

    /// Returns the executed instruction stream as `(ip, depth, opcode)`.
    ///
    /// Recording is opt-in through the `trace` feature. Depth 0 is the root
    /// frame; deeper entries come from function calls and refer to that
    /// function's own instruction list.
    #[cfg(feature = "trace")]
    pub fn trace(&self) -> &[(u32, u8, u8)] {
        &self.trace
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

#[cfg(feature = "jit")]
mod jit;
#[cfg(feature = "jit")]
use self::jit::{JitUnit, prepare_jit, prepare_jit_functions};
#[cfg(test)]
mod tests;

mod call;
mod hot;
mod registers;
mod run;

use self::hot::native_impl;
pub use self::hot::{HotNative, HotTable};
