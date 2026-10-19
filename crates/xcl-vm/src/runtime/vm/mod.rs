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
use std::sync::Arc;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

use crate::bytecode::*;
use crate::runtime::value::{
    BuiltinImpl, ImplDescriptor, NativeEntry, TraitDescriptor, TypeDescriptor, Value,
};
use bumpalo::Bump;
use smallvec::SmallVec;

/// Fast string hash for field name lookups (FNV-1a).
#[inline(always)]
pub fn fast_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
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
}

pub(crate) struct Frame {
    code: Arc<Bytecode>,
    locals: SmallVec<[Value; 16]>,
    ip: u32,
    iter: SmallVec<[Value; 8]>,
    iter_idx: u32,
    range_cur: i64,
    range_end: i64,
    stack_base: u32,
    /// Deferred closures to run when this frame returns (LIFO order).
    defer_stack: SmallVec<[Value; 4]>,
    /// Positional arguments passed into this frame (`$1`, `$#`, `$@`, `$*`).
    argv: SmallVec<[Value; 4]>,
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
    /// Pending frame to be pushed onto the frame stack (used by OP_CALL).
    pending: Option<Frame>,
    closures: Vec<crate::runtime::value::FuncValue>,
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
            pending: None,
            closures: Vec::new(),
            context_stack: HashMap::new(),
            opcode_table: [None; 256],
            #[cfg(feature = "opcode-stats")]
            opcode_counter: OpcodeCounter::new(),
            script_name: "<input>".into(),
            script_args: Vec::new(),
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

    /// Pops a value from the operand stack.
    pub fn pop_value(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::nil())
    }

    /// Pushes a value onto the operand stack.
    pub fn push_value(&mut self, value: Value) {
        self.stack.push(value);
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
        self.frames.clear();
        self.closures = bc.closures.clone();

        // Pre-allocate globals for enum variants and type names
        // Preserve existing globals (builtins registered by register_all)
        let needed = bc.global_names.len().max(self.globals.len());
        self.globals.resize(needed, Value::nil());

        // Register user-defined functions in globals so OP_CALL can find them
        // Use an offset so indices don't collide with global_table (builtins)
        let globals_offset = self.global_table.len() as u32;
        for (name, func) in &bc.user_functions {
            let idx = globals_offset + self.globals.len() as u32;
            self.globals
                .push(Value::from_func(func.clone(), &self.arena));
            self.global_index.insert(name.as_ref().into(), idx);
        }

        let n = bc.local_names.len().max(16);
        let mut locals = SmallVec::<[Value; 16]>::with_capacity(n);
        locals.resize(n, Value::nil());

        self.frames.push(Frame {
            code: Arc::new(bc.clone()),
            locals,
            ip: 0,
            iter: SmallVec::new(),
            iter_idx: 0,
            range_cur: 0,
            range_end: 0,
            stack_base: 0,
            defer_stack: SmallVec::new(),
            argv: SmallVec::from_slice(&self.script_args),
        });

        while let Some(top) = self.frames.len().checked_sub(1) {
            let frame = &self.frames[top];
            if frame.ip as usize >= frame.code.instructions.len() {
                // Run deferred closures before returning
                if let Some(frame) = self.frames.last_mut() {
                    let mut stack = core::mem::take(&mut frame.defer_stack);
                    stack.reverse();
                    for closure in stack {
                        let _ = self.call_value(closure, &[]);
                    }
                }
                let r = self.stack.pop().unwrap_or(Value::nil());
                self.frames.pop();
                if let Some(c) = self.frames.last_mut() {
                    c.ip += 1;
                    self.stack.truncate(c.stack_base as usize);
                    self.stack.push(r);
                } else {
                    return r;
                }
                continue;
            }

            let instr = unsafe { *frame.code.instructions.get_unchecked(frame.ip as usize) };
            let action = self.dispatch(top, instr);
            match action {
                Action::Next => self.frames[top].ip += 1,
                Action::Jump(to) => self.frames[top].ip = to,
                Action::Return => {
                    // Run deferred closures before returning
                    if let Some(frame) = self.frames.last_mut() {
                        let mut stack = core::mem::take(&mut frame.defer_stack);
                        stack.reverse();
                        for closure in stack {
                            let _ = self.call_value(closure, &[]);
                        }
                    }
                    let r = self.stack.pop().unwrap_or(Value::nil());
                    self.frames.pop();
                    if self.frames.is_empty() {
                        return r;
                    }
                    let c = self.frames.last_mut().unwrap();
                    c.ip += 1;
                    self.stack.truncate(c.stack_base as usize);
                    self.stack.push(r);
                }
                Action::PushFrame => {
                    let f = self.pending.take().expect("missing frame");
                    self.frames.push(f);
                }
            }
        }
        Value::nil()
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
            }
        } else {
            Action::Next
        }
    }
}

impl VM {
    fn enter_call(&mut self, func: crate::runtime::value::FuncValue, args: Vec<Value>) -> Action {
        let Some(bc) = &func.bytecode else {
            self.push(Value::nil());
            return Action::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let mut locals = SmallVec::<[Value; 16]>::with_capacity(n);
        for (i, _) in func.params.iter().enumerate() {
            locals.push(args.get(i).copied().unwrap_or(Value::nil()));
        }
        while locals.len() < n {
            locals.push(Value::nil());
        }

        let base = self.stack.len() as u32;
        let mut argv = SmallVec::<[Value; 4]>::with_capacity(args.len());
        for v in &args {
            argv.push(*v);
        }
        self.pending = Some(Frame {
            code: Arc::clone(bc),
            locals,
            ip: 0,
            iter: SmallVec::new(),
            iter_idx: 0,
            range_cur: 0,
            range_end: 0,
            stack_base: base,
            defer_stack: SmallVec::new(),
            argv,
        });
        Action::PushFrame
    }

    /// Same as `enter_call` but returns [`OpcodeResult`] for use from opcode handlers.
    fn enter_call_from_op(
        &mut self,
        func: crate::runtime::value::FuncValue,
        args: Vec<Value>,
    ) -> crate::runtime::opcode::OpcodeResult {
        match self.enter_call(func, args) {
            Action::PushFrame => crate::runtime::opcode::OpcodeResult::PushFrame,
            Action::Next => crate::runtime::opcode::OpcodeResult::Next,
            _ => crate::runtime::opcode::OpcodeResult::Next,
        }
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
        let saved_closures = core::mem::replace(&mut self.closures, bc.closures.clone());

        // Prepare locals: params first (matching how enter_call does it)
        let n = bc.local_names.len().max(func_ref.params.len());
        let mut locals = SmallVec::<[Value; 16]>::with_capacity(n);
        for (i, _) in func_ref.params.iter().enumerate() {
            locals.push(args.get(i).copied().unwrap_or(Value::nil()));
        }
        while locals.len() < n {
            locals.push(Value::nil());
        }

        let initial_depth = self.frames.len();
        let stack_base = self.stack.len() as u32;

        self.frames.push(Frame {
            code: Arc::clone(&bc),
            locals,
            ip: 0,
            iter: SmallVec::new(),
            iter_idx: 0,
            range_cur: 0,
            range_end: 0,
            stack_base,
            defer_stack: SmallVec::new(),
            argv: SmallVec::from_slice(args),
        });

        // Run the VM dispatch loop until the frame we just pushed returns
        loop {
            // Recalculate top every iteration because PushFrame/Return change frame count
            let top = match self.frames.len().checked_sub(1) {
                Some(i) => i,
                None => {
                    self.closures = saved_closures;
                    self.frames.truncate(initial_depth);
                    return Value::nil();
                }
            };

            // If we've returned to the caller depth, the closure is done
            if self.frames.len() <= initial_depth {
                break;
            }

            let frame = &self.frames[top];
            if frame.ip as usize >= frame.code.instructions.len() {
                // Run deferred closures before returning
                if let Some(frame) = self.frames.last_mut() {
                    let mut stack = core::mem::take(&mut frame.defer_stack);
                    stack.reverse();
                    for closure in stack {
                        let _ = self.call_value(closure, &[]);
                    }
                }
                // Frame ran off the end — treat top of stack as return value
                let result = self.stack.pop().unwrap_or(Value::nil());
                self.frames.pop();
                if self.frames.len() <= initial_depth {
                    self.stack.truncate(stack_base as usize);
                    self.closures = saved_closures;
                    return result;
                }
                // Nested call returned; hand the value to the outer frame
                if let Some(caller) = self.frames.last_mut() {
                    caller.ip += 1;
                    self.stack.truncate(caller.stack_base as usize);
                    self.stack.push(result);
                }
                continue;
            }

            let instr = unsafe { *frame.code.instructions.get_unchecked(frame.ip as usize) };
            let action = self.dispatch(top, instr);
            match action {
                Action::Next => {
                    let top = self.frames.len().checked_sub(1).unwrap_or(top);
                    self.frames[top].ip += 1;
                }
                Action::Jump(to) => {
                    let top = self.frames.len().checked_sub(1).unwrap_or(top);
                    self.frames[top].ip = to;
                }
                Action::Return => {
                    // Run deferred closures before returning
                    if let Some(frame) = self.frames.last_mut() {
                        let mut stack = core::mem::take(&mut frame.defer_stack);
                        stack.reverse();
                        for closure in stack {
                            let _ = self.call_value(closure, &[]);
                        }
                    }
                    let result = self.stack.pop().unwrap_or(Value::nil());
                    self.frames.pop();
                    if self.frames.len() <= initial_depth {
                        self.stack.truncate(stack_base as usize);
                        self.closures = saved_closures;
                        return result;
                    }
                    if let Some(caller) = self.frames.last_mut() {
                        caller.ip += 1;
                        self.stack.truncate(caller.stack_base as usize);
                        self.stack.push(result);
                    }
                }
                Action::PushFrame => {
                    let f = self.pending.take().expect("missing frame");
                    self.frames.push(f);
                    // Don't advance ip; the newly pushed frame will be dispatched next iteration
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
        if let Some(frame) = self.frames.last_mut() {
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
        vm.run(&bc)
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
}
