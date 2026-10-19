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
use alloc::{
    vec::Vec,
};

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

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use crate::HashMap;

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
enum Action {
    /// Advance to the next instruction.
    Next,
    /// Jump to the given instruction pointer.
    Jump(u32),
    /// Return from the current frame.
    Return,
    /// Push the pending frame onto the frame stack.
    PushFrame,
    /// The current frame switched to different bytecode (tail call).
    TailCall,
}

pub(crate) struct Frame {
    code: Arc<Bytecode>,
    /// Local slots.
    ///
    /// A plain `Vec` (not `SmallVec`) on purpose: every local read does
    /// `get_unchecked`, and `SmallVec` must branch on inline-vs-heap for each
    /// access. It also shrinks `Frame` by ~112 bytes, which is memcpy'd three
    /// times per call (pool → frames → pool).
    locals: Vec<Value>,
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

/// Returns the raw instruction bounds of a frame's bytecode.
///
/// The dispatch loop keeps these in locals; a raw pointer avoids borrowing
/// `VM` for the duration of the loop (handlers need `&mut VM`).
#[inline(always)]
fn frame_code_bounds(frame: &Frame) -> (*const Instr, usize) {
    let code = &frame.code.instructions;
    (code.as_ptr(), code.len())
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
    /// Flat list of registered type descriptors (for axiom type/super lookups).
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
    /// Direct-mapped 4-entry callee cache for fast recursive call dispatch.
    call_cache: [(Value, Value); 4],
    /// Number of live call frames.
    ///
    /// `frames[..frame_depth]` are live; the rest are recycled slots kept
    /// around for reuse. Frames are never moved out of the vector: entering a
    /// call reinitializes the slot at `frame_depth` in place, returning
    /// decrements it. The old design pushed/popped `Frame` by value through a
    /// separate pool, which moved ~200 bytes three times per call (~20% of all
    /// retired instructions on recursive workloads).
    frame_depth: usize,
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
        Self {
            stack: Vec::with_capacity(64),
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
            arena: Bump::new(),
            frames: Vec::with_capacity(16),
            closures: Arc::new([]),
            context_stack: HashMap::new(),
            opcode_table: [None; 256],
            #[cfg(feature = "opcode-stats")]
            opcode_counter: OpcodeCounter::new(),
            script_name: "<input>".into(),
            script_args: Vec::new(),
            call_cache: [(Value::nil(), Value::nil()); 4],
            frame_depth: 0,
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
    #[inline]
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
    fn acquire_frame(&mut self, bc: &Arc<Bytecode>) -> usize {
        let idx = self.frame_depth;
        if idx == self.frames.len() {
            self.frames.push(Frame {
                code: Arc::clone(bc),
                locals: Vec::new(),
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
        if !Arc::ptr_eq(&frame.code, bc) {
            frame.code = Arc::clone(bc);
        }
        frame.ip = 0;
        frame.iter.clear();
        frame.iter_idx = 0;
        frame.range_cur = 0;
        frame.range_end = 0;
        frame.defer_stack.clear();
        frame.argv.clear();
        frame.locals.clear();
        idx
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
        &self.frames[top].locals
    }

    /// Returns a mutable slice of local variables in the current frame.
    #[allow(clippy::mut_from_ref)]
    pub fn frame_locals_mut(&mut self, top: usize) -> &mut [Value] {
        &mut self.frames[top].locals
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
    /// Universal methods like `type` and `super` are available on all values
    /// regardless of their type.
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
    pub fn run(&mut self, bc: &Bytecode) -> Value {
        self.stack.clear();
        self.frame_depth = 0;
        if self.frames.len() > 4096 {
            self.frames.truncate(4096);
        }
        self.closures = Arc::clone(&bc.closures);
        self.call_cache = [(Value::nil(), Value::nil()); 4];

        // Pre-allocate globals for enum variants and type names
        // Preserve existing globals (builtins registered by register_all)
        let needed = bc.global_names.len().max(self.globals.len());
        self.globals.resize(needed, Value::nil());

        // Register user-defined functions in globals so OP_CALL can find them
        // Use an offset so indices don't collide with global_table (builtins)
        let globals_offset = self.global_table.len() as u32;
        for (name, func) in bc.user_functions.iter() {
            let idx = globals_offset + self.globals.len() as u32;
            self.globals
                .push(Value::from_func_shared(Arc::clone(func), &self.arena));
            self.global_index.insert(name.as_ref().into(), idx);
        }

        let n = bc.local_names.len().max(16);
        let root_code = Arc::new(bc.clone());
        let root = self.acquire_frame(&root_code);
        {
            let frame = &mut self.frames[root];
            frame.stack_base = 0;
            frame.locals.resize(n, Value::nil());
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

        loop {
            if ip >= code_len {
                // Run deferred closures before returning
                self.run_defers();
                let r = self.stack.pop().unwrap_or(Value::nil());
                let returning_base = self.top_frame().map(|f| f.stack_base);
                self.frame_depth -= 1;
                debug_assert_eq!(self.frame_depth, top);
                if self.frame_depth == 0 {
                    return Value::escape(r);
                }
                top -= 1;
                let caller = unsafe { self.frames.get_unchecked(top) };
                let base = returning_base.unwrap_or(caller.stack_base);
                ip = caller.ip as usize;
                (code_ptr, code_len) = frame_code_bounds(caller);
                self.stack.truncate(base as usize);
                self.stack.push(r);
                continue;
            }

            let instr = unsafe { *code_ptr.add(ip) };
            let action = self.dispatch(top, instr);
            match action {
                Action::Next => ip += 1,
                Action::Jump(to) => ip = to as usize,
                Action::TailCall => {
                    // The frame rebound to another function's bytecode.
                    ip = 0;
                    (code_ptr, code_len) =
                        frame_code_bounds(unsafe { self.frames.get_unchecked(top) });
                }
                Action::Return => {
                    // Run deferred closures before returning
                    self.run_defers();
                    let r = self.stack.pop().unwrap_or(Value::nil());
                    let returning_base = self.top_frame().map(|f| f.stack_base);
                    self.frame_depth -= 1;
                    if self.frame_depth == 0 {
                        return Value::escape(r);
                    }
                    top -= 1;
                    let caller = unsafe { self.frames.get_unchecked(top) };
                    let base = returning_base.unwrap_or(caller.stack_base);
                    ip = caller.ip as usize;
                    (code_ptr, code_len) = frame_code_bounds(caller);
                    self.stack.truncate(base as usize);
                    self.stack.push(r);
                }
                Action::PushFrame => {
                    // The callee frame is already on the stack. Record where the
                    // caller resumes; nothing else needs the caller's ip.
                    unsafe { self.frames.get_unchecked_mut(top) }.ip = (ip + 1) as u32;
                    top += 1;
                    ip = 0;
                    (code_ptr, code_len) =
                        frame_code_bounds(unsafe { self.frames.get_unchecked(top) });
                }
            }
        }
    }

    #[inline(always)]
    fn dispatch(&mut self, top: usize, i: Instr) -> Action {
        #[cfg(feature = "opcode-stats")]
        self.opcode_counter.record(i.op());

        // Core opcode dispatch
        if let Some(result) = ops::dispatch_core(self, top, i) {
            return match result {
                crate::runtime::opcode::OpcodeResult::Next => Action::Next,
                crate::runtime::opcode::OpcodeResult::Jump(addr) => Action::Jump(addr),
                crate::runtime::opcode::OpcodeResult::Return => Action::Return,
                crate::runtime::opcode::OpcodeResult::PushFrame => Action::PushFrame,
                crate::runtime::opcode::OpcodeResult::TailCall => Action::TailCall,
            };
        }

        // Extension opcode lookup
        let op_code = i.op();
        if let Some(handler) = self.opcode_table[op_code as usize] {
            match handler(self, top, i) {
                super::opcode::OpcodeResult::Next => Action::Next,
                super::opcode::OpcodeResult::Jump(addr) => Action::Jump(addr),
                super::opcode::OpcodeResult::Return => Action::Return,
                super::opcode::OpcodeResult::PushFrame => Action::PushFrame,
                super::opcode::OpcodeResult::TailCall => Action::TailCall,
            }
        } else {
            Action::Next
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
        let frame = &mut self.frames[idx];
        frame.ip = 0;
        frame.stack_base = base;

        // Prepare locals (bulk copy + resize)
        let copy = func.params.len().min(args.len());
        frame.locals.extend_from_slice(&args[..copy]);
        frame.locals.resize(n, Value::nil());

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
        let frame = &mut self.frames[idx];
        frame.ip = 0;
        frame.stack_base = arg_base as u32;

        let copy = func.params.len().min(nargs);
        frame
            .locals
            .extend_from_slice(&self.stack[arg_start..arg_start + copy]);
        frame.locals.resize(n, Value::nil());

        if func.uses_special_vars {
            frame
                .argv
                .extend_from_slice(&self.stack[arg_start..arg_start + nargs]);
        }

        self.stack.truncate(arg_base);
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

        if !Arc::ptr_eq(&frame.code, bc) {
            frame.code = Arc::clone(bc);
        }
        frame.ip = 0;
        frame.iter.clear();
        frame.iter_idx = 0;
        frame.range_cur = 0;
        frame.range_end = 0;
        frame.defer_stack.clear();

        // Prepare locals (bulk copy + resize)
        frame.locals.clear();
        frame.locals.reserve(n);
        let copy = func.params.len().min(args.len());
        frame.locals.extend_from_slice(&args[..copy]);
        frame.locals.resize(n, Value::nil());

        // Prepare argv (only if function uses special vars)
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
        let stack_base = self.stack.len() as u32;

        let idx = self.acquire_frame(&bc);
        {
            let frame = &mut self.frames[idx];
            frame.stack_base = stack_base;
            for (i, _) in func_ref.params.iter().enumerate() {
                frame
                    .locals
                    .push(args.get(i).copied().unwrap_or(Value::nil()));
            }
            frame.locals.resize(n, Value::nil());
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
                    return Value::nil();
                }
            };

            // If we've returned to the caller depth, the closure is done
            if self.frame_depth <= initial_depth {
                break;
            }

            let frame = &self.frames[top];
            if frame.ip as usize >= frame.code.instructions.len() {
                // Run deferred closures before returning
                self.run_defers();
                // Frame ran off the end — treat top of stack as return value
                let result = self.stack.pop().unwrap_or(Value::nil());
                let returning_base = self.top_frame().map(|f| f.stack_base);
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

            let instr = unsafe { *frame.code.instructions.get_unchecked(frame.ip as usize) };
            let action = self.dispatch(top, instr);
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
}
