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
//! use xcl_vm::VM;
//! use xcl_vm::lexer::lex;
//! use xcl_vm::parser::parse;
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
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

use crate::bytecode::*;
use crate::value::{BuiltinImpl, TAG_INT, TypeDescriptor, Value};
use bumpalo::Bump;
use smallvec::SmallVec;

// ── Helper functions for integer arithmetic with overflow fallback ──

#[inline(always)]
fn checked_add_overflow(a: i64, b: i64) -> i64 {
    a.checked_add(b)
        .unwrap_or_else(|| (a as f64 + b as f64) as i64)
}

#[inline(always)]
fn checked_sub_overflow(a: i64, b: i64) -> i64 {
    a.checked_sub(b)
        .unwrap_or_else(|| (a as f64 - b as f64) as i64)
}

#[inline(always)]
fn checked_mul_overflow(a: i64, b: i64) -> i64 {
    a.checked_mul(b)
        .unwrap_or_else(|| (a as f64 * b as f64) as i64)
}

#[inline(always)]
fn checked_div_overflow(a: i64, b: i64) -> i64 {
    a.checked_div(b)
        .unwrap_or_else(|| (a as f64 / b as f64) as i64)
}

// ── Macros to reduce dispatch boilerplate ──
// Since Rust macros cannot expand to match arms directly, we use nested match
// patterns with guard clauses in the dispatch method.

#[cfg(feature = "opcode-stats")]
/// Generate opcode_name match from a list of opcode => display_name pairs.
macro_rules! opcode_names {
    ( $op:expr, $( $opcode:ident => $name:expr ),+ $(,)? ) => {
        match $op {
            $( $opcode => $name.to_string(), )+
            _ => format!("0x{:02X}", $op),
        }
    };
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

enum Action {
    Next,
    Jump(u32),
    Return,
    PushFrame,
}

pub(crate) struct Frame {
    code: Arc<Bytecode>,
    locals: SmallVec<[Value; 16]>,
    ip: u32,
    /// SmallVec avoids heap allocation for small iterators (≤8 elements inline).
    iter: SmallVec<[Value; 8]>,
    iter_idx: u32,
    range_cur: i64,
    range_end: i64,
    stack_base: u32,
    /// Deferred closures to run when this frame returns (LIFO order).
    defer_stack: SmallVec<[Value; 4]>,
}

/// A dispatch table slot containing all methods registered for a specific domain namespace.
pub struct DomainSlot {
    /// Domain identifier name (e.g., `"fs"`, `"net"`, `"math"`).
    pub name: Box<str>,
    /// Array of method implementations indexed by method slot id.
    pub methods: Vec<Option<BuiltinImpl>>,
    /// Fast string-to-slot-id index mapping method names to offsets in `methods`.
    pub index: HashMap<Box<str>, u32>,
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
    /// Fast bump allocator arena for temporary heap objects created during execution.
    pub arena: Bump,
    /// Registry of well-known constructor and type names (`Ok`, `Err`, `Result`).
    pub well_known: crate::wellknown::WellKnown,
    pub(crate) frames: Vec<Frame>,
    pending: Option<Frame>,
    closures: Vec<crate::value::FuncValue>,
    /// Stack-based context for domain blocks (e.g., `browser "brave" { ... }`).
    /// Each key maps to a stack of values, enabling nested domain contexts.
    pub(crate) context_stack: HashMap<String, Vec<Value>>,
    #[cfg(feature = "opcode-stats")]
    opcode_counter: OpcodeCounter,
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
            arena: Bump::new(),
            well_known: Default::default(),
            frames: Vec::new(),
            pending: None,
            closures: Vec::new(),
            context_stack: HashMap::new(),
            #[cfg(feature = "opcode-stats")]
            opcode_counter: OpcodeCounter::new(),
        }
    }

    /// Configures the names of well-known types and constructors (e.g. `Ok`, `Err`, `Result`).
    ///
    /// Typically called by `xcl-plugins` during plugin registration.
    ///
    /// # Arguments
    ///
    /// * `wk` - Configured [`crate::wellknown::WellKnown`] metadata.
    pub fn set_well_known(&mut self, wk: crate::wellknown::WellKnown) {
        self.well_known = wk;
    }

    /// Registers all compile-time native functions and types from the distributed slices.
    ///
    /// Combines [`VM::register_all_natives`] and [`VM::register_all_types`].
    pub fn register_all(&mut self) {
        self.register_all_natives();
        self.register_all_types();
    }

    /// Scans the distributed [`crate::native::NATIVE_REGISTRY`] slice
    /// and populates the VM's domain and global dispatch tables.
    pub fn register_all_natives(&mut self) {
        use crate::native::NATIVE_REGISTRY;
        use crate::value::{BuiltinImpl, NativeKind};

        for entry in NATIVE_REGISTRY.iter() {
            let impl_ = match entry.kind {
                NativeKind::Pure => {
                    let f: crate::value::BuiltinFn = unsafe { core::mem::transmute(entry.ptr.0) };
                    BuiltinImpl::Pure(f)
                }
                NativeKind::Ctx => {
                    let f: crate::value::BuiltinCtxFn =
                        unsafe { core::mem::transmute(entry.ptr.0) };
                    BuiltinImpl::Ctx(f)
                }
            };

            if let Some(domain) = entry.domain {
                // Build the full dotted name: domain + "." + method
                let full_name = format!("{}.{}", domain, entry.method);

                // Register in domain table for domain-based lookup
                let didx = *self.domain_index.entry(domain.into()).or_insert_with(|| {
                    let idx = self.domain_table.len() as u32;
                    self.domain_table.push(DomainSlot {
                        name: domain.into(),
                        methods: Vec::new(),
                        index: HashMap::new(),
                    });
                    idx
                });
                let slot = &mut self.domain_table[didx as usize];
                let midx = slot.methods.len() as u32;
                slot.methods.push(Some(impl_));
                slot.index.insert(entry.method.into(), midx);

                // Also register the full dotted name globally so "net.tcp.connect" works
                let idx = self.global_table.len() as u32;
                self.global_table.push(Some(impl_));
                self.global_index.insert(full_name.into(), idx);
            } else {
                let idx = self.global_table.len() as u32;
                self.global_table.push(Some(impl_));
                self.global_index.insert(entry.method.into(), idx);
            }
        }
    }

    /// Scans the distributed [`crate::native::TYPE_REGISTRY`] slice
    /// and indexes all registered custom types by name.
    pub fn register_all_types(&mut self) {
        use crate::native::TYPE_REGISTRY;
        for desc in TYPE_REGISTRY.iter() {
            self.type_index.insert(desc.name.into(), desc);
        }
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
    /// use xcl_vm::VM;
    /// use xcl_vm::lexer::lex;
    /// use xcl_vm::parser::parse;
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
        for (name, func) in &bc.user_functions {
            let idx = self.globals.len() as u32;
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
        match i.op() {
            OP_NOP => Action::Next,
            OP_PUSH_NIL => {
                self.push(Value::nil());
                Action::Next
            }
            OP_PUSH_TRUE => {
                self.push(Value::bool(true));
                Action::Next
            }
            OP_PUSH_FALSE => {
                self.push(Value::bool(false));
                Action::Next
            }
            OP_PUSH_INT => {
                self.push(Value::from_i48(i.a() as i64));
                Action::Next
            }
            OP_PUSH_INT_WIDE => {
                let v = self.frames[top].code.wide_operands[i.a() as usize];
                self.push(Value::from_i48(v));
                Action::Next
            }
            OP_PUSH_FLOAT => {
                let bits = self.frames[top].code.wide_operands[i.a() as usize];
                self.push(Value::from_f64(f64::from_bits(bits as u64)));
                Action::Next
            }
            OP_PUSH_STR => {
                let s = self.frames[top].code.string_table[i.a() as usize].to_string();
                self.push(Value::from_str(s, &self.arena));
                Action::Next
            }
            OP_LOAD_LOCAL => {
                let v = unsafe { *self.frames[top].locals.get_unchecked(i.a() as usize) };
                self.push(v);
                Action::Next
            }
            OP_STORE_LOCAL => {
                let v = self.pop();
                unsafe {
                    *self.frames[top].locals.get_unchecked_mut(i.a() as usize) = v;
                }
                Action::Next
            }
            OP_LOAD_GLOBAL => {
                let v = self
                    .globals
                    .get(i.a() as usize)
                    .copied()
                    .unwrap_or(Value::nil());
                self.push(v);
                Action::Next
            }
            OP_STORE_GLOBAL => {
                let v = self.pop();
                if (i.a() as usize) < self.globals.len() {
                    self.globals[i.a() as usize] = v;
                }
                Action::Next
            }
            OP_NEG => {
                let a = self.pop();
                self.push(a.neg_());
                Action::Next
            }
            OP_NOT => {
                let a = self.pop();
                self.push(a.not_());
                Action::Next
            }
            OP_ADD => {
                let b = self.pop();
                let a = self.pop();
                self.push(a.add(b, &self.arena));
                Action::Next
            }
            OP_SUB => {
                let b = self.pop();
                let a = self.pop();
                self.push(a.sub_(b));
                Action::Next
            }
            OP_MUL => {
                let b = self.pop();
                let a = self.pop();
                self.push(a.mul(b, &self.arena));
                Action::Next
            }
            OP_DIV => {
                let b = self.pop();
                let a = self.pop();
                self.push(a.div_(b));
                Action::Next
            }
            OP_MOD => {
                let b = self.pop();
                let a = self.pop();
                self.push(a.mod_(b));
                Action::Next
            }
            OP_EQ => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.eq_(b)));
                Action::Next
            }
            OP_NEQ => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(!a.eq_(b)));
                Action::Next
            }
            OP_LT => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.lt(b)));
                Action::Next
            }
            OP_GT => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.gt(b)));
                Action::Next
            }
            OP_LE => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.lt(b) || a.eq_(b)));
                Action::Next
            }
            OP_GE => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.gt(b) || a.eq_(b)));
                Action::Next
            }
            OP_AND => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.is_truthy() && b.is_truthy()));
                Action::Next
            }
            OP_OR => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::bool(a.is_truthy() || b.is_truthy()));
                Action::Next
            }
            OP_IN => {
                let c = self.pop();
                let item = self.pop();
                let found = c
                    .as_array()
                    .map(|a| a.iter().any(|v| v.eq_(item)))
                    .unwrap_or(false);
                self.push(Value::bool(found));
                Action::Next
            }
            OP_TYPE_TEST => {
                // Stack: value, type_name
                let type_name_s = self.pop().as_str();
                let value = self.pop();
                let is_type = if let Some(tn) = type_name_s {
                    // Check if the value is a map with a "tag" field matching the type name
                    if let Some(map) = value.as_map() {
                        if let Some(tag_val) = map.get("tag") {
                            if let Some(tag) = tag_val.as_str() {
                                tag == tn
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        // Check type index for registered types
                        self.type_index.contains_key(tn) && !value.is_nil()
                    }
                } else {
                    false
                };
                self.push(Value::bool(is_type));
                Action::Next
            }
            OP_JUMP => Action::Jump(i.a()),
            OP_JUMP_IF_FALSE => {
                let c = self.pop();
                if c.is_truthy() {
                    Action::Next
                } else {
                    Action::Jump(i.a())
                }
            }
            OP_JUMP_IF_NIL => {
                let v = self.pop();
                if v.is_nil() {
                    Action::Jump(i.a())
                } else {
                    Action::Next
                }
            }
            OP_CALL => {
                let n = i.a() as usize;
                let mut args = Vec::with_capacity(n);
                for _ in 0..n {
                    args.push(self.pop());
                }
                args.reverse();
                let callee = self.pop();

                if let Some(name) = callee.as_str() {
                    // 1. Check global builtins first (handles "net.tcp.connect", "io.read", etc.)
                    if let Some(&idx) = self.global_index.get(name) {
                        if let Some(Some(impl_)) = self.global_table.get(idx as usize) {
                            let impl_copy = *impl_;
                            let result = impl_copy.call(self, &args);
                            self.push(result);
                            return Action::Next;
                        }
                        // Check user-defined globals
                        if (idx as usize) < self.globals.len() {
                            let func = self.globals[idx as usize];
                            if let Some(f) = func.as_func() {
                                return self.enter_call(f.clone(), args);
                            }
                        }
                    }
                    // 2. Check domain methods (e.g. "io.read") — only for two-part names
                    // For three+ part names like "net.tcp.connect", step 1 already handled it
                    if name.split('.').count() == 2 {
                        if let Some(dot_pos) = name.find('.') {
                            let domain = &name[..dot_pos];
                            let method = &name[dot_pos + 1..];
                            if let Some(impl_) = self.resolve_domain_method(domain, method) {
                                let impl_copy = *impl_;
                                let result = impl_copy.call(self, &args);
                                self.push(result);
                                return Action::Next;
                            }
                        }
                    }
                    // 2b. Domain expansion: if callee is a domain name, check for block/dict args.
                    if self.domain_index.contains_key(name) {
                        // Case 1: one arg is a function — execute it with domain methods in scope.
                        if let Some((func_idx, func)) = args
                            .iter()
                            .enumerate()
                            .find_map(|(i, a)| a.as_func().map(|f| (i, f.clone())))
                        {
                            let call_args: Vec<Value> = args
                                .into_iter()
                                .enumerate()
                                .filter(|(i, _)| *i != func_idx)
                                .map(|(_, v)| v)
                                .collect();
                            let &didx = self.domain_index.get(name).unwrap();
                            let slot = &self.domain_table[didx as usize];
                            let mut saved: Vec<(Box<str>, Option<u32>)> = Vec::new();
                            for (method_name, &midx) in slot.index.iter() {
                                let existing = self
                                    .global_index
                                    .insert(method_name.clone(), self.global_table.len() as u32);
                                saved.push((method_name.clone(), existing));
                                let impl_ = slot.methods[midx as usize];
                                self.global_table.push(impl_);
                            }
                            let func_value = Value::from_func(func.clone(), &self.arena);
                            let result = self.call_value(func_value, &call_args);
                            for (method_name, existing) in saved.into_iter().rev() {
                                self.global_table.pop();
                                if let Some(idx) = existing {
                                    self.global_index.insert(method_name.clone(), idx);
                                } else {
                                    self.global_index.remove(&method_name);
                                }
                            }
                            self.push(result);
                            return Action::Next;
                        }
                        // Case 2: one arg is a map — each key is a method name, each value is the arg.
                        if let Some((_map_idx, map)) = args
                            .iter()
                            .enumerate()
                            .find_map(|(i, a)| a.as_map().map(|m| (i, m)))
                        {
                            let &didx = self.domain_index.get(name).unwrap();
                            // Collect method calls to avoid borrow conflict
                            let calls: Vec<(u32, Value)> = self.domain_table[didx as usize]
                                .index
                                .iter()
                                .filter_map(|(method_name, &midx)| {
                                    map.get(&**method_name).map(|v| (midx, *v))
                                })
                                .collect();
                            let mut last_result = Value::nil();
                            for (midx, value) in calls {
                                let impl_ = self.domain_table[didx as usize].methods[midx as usize]
                                    .as_ref()
                                    .copied()
                                    .unwrap();
                                last_result = impl_.call(self, &[value]);
                            }
                            self.push(last_result);
                            return Action::Next;
                        }
                    }
                    // 3. Fallback: search all globals by name
                    let found = self
                        .globals
                        .iter()
                        .find_map(|g| g.as_func().filter(|f| &*f.name == name).cloned());
                    if let Some(func) = found {
                        return self.enter_call(func, args);
                    }
                } else if let Some(func) = callee.as_func() {
                    return self.enter_call(func.clone(), args);
                }
                self.push(Value::nil());
                Action::Next
            }
            OP_CALL_METHOD => {
                // Dispatch a method call on a value by looking up its type's domain.
                // Stack: receiver, arg1, arg2, ... -> result
                let field_idx = i.a() as usize;
                let nargs = i.b() as usize;
                let string_table = self.frames[top].code.string_table.clone();
                let method_name = &string_table[field_idx];

                // Collect args (excluding receiver)
                let mut args = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    args.push(self.pop());
                }
                args.reverse();
                let receiver = self.pop();

                // Look up the domain from the receiver's type.
                // For typed maps (Result, Error, Handle), use the subtype to find the domain.
                let domain_name = if let Some(subtype) = receiver.map_subtype() {
                    match subtype {
                        1 => Some("result"), // SUBTYPE_RESULT
                        2 => Some("error"),  // SUBTYPE_ERROR
                        3 => Some("handle"), // SUBTYPE_HANDLE
                        _ => None,
                    }
                } else {
                    None
                };

                if let Some(domain) = domain_name {
                    if let Some(impl_) = self.resolve_domain_method(domain, method_name) {
                        let impl_copy = *impl_;
                        // Prepend receiver as the first argument
                        let mut all_args = Vec::with_capacity(nargs + 1);
                        all_args.push(receiver);
                        all_args.extend(args);
                        let result = impl_copy.call(self, &all_args);
                        self.push(result);
                        return Action::Next;
                    }
                }

                // Not found
                self.push(Value::nil());
                Action::Next
            }
            OP_RETURN | OP_HALT => Action::Return,
            OP_LEN => {
                let v = self.pop();
                self.push(Value::from_i48(v.len() as i64));
                Action::Next
            }
            OP_GET_INDEX => {
                let idx = self.pop();
                let target = self.pop();
                let result = if let Some(arr) = target.as_array() {
                    let i = idx.as_int();
                    let i = if i < 0 { arr.len() as i64 + i } else { i };
                    arr.get(i as usize).copied().unwrap_or(Value::nil())
                } else if let Some(m) = target.as_map() {
                    idx.as_str()
                        .and_then(|k| m.get(k).copied())
                        .unwrap_or(Value::nil())
                } else {
                    Value::nil()
                };
                self.push(result);
                Action::Next
            }
            OP_GET_FIELD => {
                let field_idx = i.a() as usize;
                let string_table = self.frames[top].code.string_table.clone();
                let field = &string_table[field_idx];
                let target = self.pop();
                let v = target
                    .as_map()
                    .and_then(|m| m.get(&**field).copied())
                    .unwrap_or(Value::nil());
                self.push(v);
                Action::Next
            }
            OP_MAKE_ARRAY => {
                let n = i.a() as usize;
                let mut elements = Vec::with_capacity(n);
                for _ in 0..n {
                    elements.push(self.pop());
                }
                elements.reverse();
                self.push(Value::from_array(elements, &self.arena));
                Action::Next
            }
            OP_MAKE_DICT => {
                let n = i.a() as usize;
                let mut m = HashMap::with_capacity(n);
                for _ in 0..n {
                    let v = self.pop();
                    let k = self.pop();
                    m.insert(k.as_string(), v);
                }
                self.push(Value::from_map(m, &self.arena));
                Action::Next
            }
            OP_ARRAY_PUSH => {
                let elem = self.pop();
                let target = self.pop();
                if let Some(arr) = target.as_array_mut() {
                    arr.push(elem);
                }
                self.push(target);
                Action::Next
            }
            OP_FOR_ITER_START => {
                let iterable = self.pop();
                let f = &mut self.frames[top];
                f.iter.clear();
                if let Some(arr) = iterable.as_array() {
                    f.iter.extend_from_slice(arr);
                }
                f.iter_idx = 0;
                Action::Next
            }
            OP_FOR_ITER_NEXT => {
                let f = &mut self.frames[top];
                if (f.iter_idx as usize) < f.iter.len() {
                    let v = f.iter[f.iter_idx as usize];
                    f.iter_idx += 1;
                    self.push(v);
                    Action::Next
                } else {
                    Action::Jump(i.a())
                }
            }
            OP_FOR_RANGE_START => {
                let end = self.pop().as_int();
                let start = self.pop().as_int();
                let f = &mut self.frames[top];
                f.range_cur = start;
                f.range_end = end;
                Action::Next
            }
            OP_FOR_RANGE_NEXT => {
                let f = &mut self.frames[top];
                if f.range_cur < f.range_end {
                    let v = f.range_cur;
                    f.range_cur += 1;
                    self.push(Value::from_i48(v));
                    Action::Next
                } else {
                    Action::Jump(i.a())
                }
            }
            OP_LOAD_LAMBDA => {
                let f = self.closures[i.a() as usize].clone();
                self.push(Value::from_func(f, &self.arena));
                Action::Next
            }
            OP_COALESCE => {
                let r = self.pop();
                let l = self.pop();
                self.push(if l.is_nil() { r } else { l });
                Action::Next
            }
            OP_LOAD_LOCAL_LOAD_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(a);
                self.push(b);
                Action::Next
            }
            OP_LOAD_LOCAL_PUSH_INT => {
                let v = unsafe { *self.frames[top].locals.get_unchecked(i.a() as usize) };
                self.push(v);
                self.push(Value::from_i48(i.b() as i64));
                Action::Next
            }
            OP_LOAD_LOCAL_STORE_LOCAL => {
                let v = unsafe { *self.frames[top].locals.get_unchecked(i.a() as usize) };
                unsafe {
                    *self.frames[top].locals.get_unchecked_mut(i.b() as usize) = v;
                }
                Action::Next
            }

            // ── Integer-specialized arithmetic (fast path, no type checks) ──
            OP_ADD_INT => {
                let b = self.pop();
                let a = self.pop();
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    self.push(a.add_int(b));
                } else {
                    self.push(a.add(b, &self.arena));
                }
                Action::Next
            }
            OP_SUB_INT => {
                let b = self.pop();
                let a = self.pop();
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    self.push(a.sub_int(b));
                } else {
                    self.push(a.sub_(b));
                }
                Action::Next
            }
            OP_MUL_INT => {
                let b = self.pop();
                let a = self.pop();
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    self.push(a.mul_int(b));
                } else {
                    self.push(a.mul(b, &self.arena));
                }
                Action::Next
            }
            // ── Integer-specialized comparisons (fast path, no type checks) ──
            OP_LT_INT => {
                let b = self.pop();
                let a = self.pop();
                let r = if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    a.lt_int(b)
                } else {
                    a.lt(b)
                };
                self.push(Value::bool(r));
                Action::Next
            }
            OP_GT_INT => {
                let b = self.pop();
                let a = self.pop();
                let r = if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    a.gt_int(b)
                } else {
                    a.gt(b)
                };
                self.push(Value::bool(r));
                Action::Next
            }
            OP_EQ_INT => {
                let b = self.pop();
                let a = self.pop();
                let r = if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    a.eq_int(b)
                } else {
                    a.eq_(b)
                };
                self.push(Value::bool(r));
                Action::Next
            }

            // ── INC/DEC_LOCAL: $x = $x + 1 / $x - 1 ──
            OP_INC_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.tag() == TAG_INT {
                    let r = v.as_i48().checked_add(1).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(idx) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(idx) = v.add(Value::from_i48(1), &self.arena);
                    }
                }
                Action::Next
            }
            OP_DEC_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.tag() == TAG_INT {
                    let r = v.as_i48().checked_sub(1).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(idx) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(idx) = v.sub_(Value::from_i48(1));
                    }
                }
                Action::Next
            }
            // ── Local-local binary ops: push(a op b) where a and b are locals ──
            OP_ADD_LOCAL_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(a.add(b, &self.arena));
                Action::Next
            }
            OP_SUB_LOCAL_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(a.sub_(b));
                Action::Next
            }
            OP_MUL_LOCAL_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(a.mul(b, &self.arena));
                Action::Next
            }
            OP_DIV_LOCAL_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(a.div_(b));
                Action::Next
            }
            OP_LT_LOCAL_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(Value::bool(a.lt(b)));
                Action::Next
            }
            // ── ADD_INT_STORE_LOCAL: $dest = $src + c (three operands) ──
            OP_ADD_INT_STORE_LOCAL => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let c = i.c() as i64;
                let v = unsafe { *l.get_unchecked(src) };
                if v.is_int() {
                    let r = v.as_i48().checked_add(c).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    let r = v.add(Value::from_i48(c), &self.arena);
                    unsafe {
                        *l.get_unchecked_mut(dest) = r;
                    }
                }
                Action::Next
            }
            // ── INC_LOCAL_INT: $x = $x + c ──
            OP_INC_LOCAL_INT => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_add(c).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(idx) = Value::from_i48(r);
                    }
                } else {
                    let r = v.add(Value::from_i48(c), &self.arena);
                    unsafe {
                        *l.get_unchecked_mut(idx) = r;
                    }
                }
                Action::Next
            }

            // ── FOR_NUMBER_LOOP: single-opcode numeric for-loop (Luau FORNLOOP style) ──
            // a = (var_slot << 16) | start_wide_idx, b = end_wide_idx
            // The loop body follows this instruction, ending with OP_JUMP back here
            OP_FOR_NUMBER_LOOP => {
                let f = &mut self.frames[top];
                let var_slot = (i.a() >> 16) as usize;
                let start_wide_idx = (i.a() & 0xFFFF) as usize;
                let end_wide_idx = i.b() as usize;

                // Get start and end from wide operands (only on first iteration)
                let start = f.code.wide_operands[start_wide_idx];
                let end = f.code.wide_operands[end_wide_idx];

                // Read current value from local slot
                let cur = unsafe { *f.locals.get_unchecked(var_slot) };
                let cur_val = if cur.is_int() { cur.as_i48() } else { start };

                // Increment
                let next_val = cur_val + 1;

                // Compare and jump
                if next_val < end {
                    // Store incremented value back to local
                    unsafe {
                        *f.locals.get_unchecked_mut(var_slot) = Value::from_i48(next_val);
                    }
                    // Continue to loop body (next instruction)
                    f.ip += 1;
                    Action::Next
                } else {
                    // Loop done — jump past the body and the OP_JUMP at the end
                    // We need to find the OP_JUMP at the end of the body and skip it
                    // The OP_JUMP's target tells us where the loop started, so we skip to ip+1
                    // Actually, we need to skip past the body. The body ends with OP_JUMP.
                    // We need to find where the body ends. Since the body always ends with
                    // OP_JUMP back to this instruction, we can scan forward to find it.
                    let loop_ip = f.ip; // current instruction pointer (FOR_NUMBER_LOOP)
                    f.ip += 1; // skip past FOR_NUMBER_LOOP to start of body
                    // Scan forward to find the OP_JUMP that closes the loop
                    let code = &f.code.instructions;
                    let mut scan = f.ip as usize;
                    while scan < code.len() {
                        let instr = unsafe { *code.get_unchecked(scan) };
                        if instr.op() == OP_JUMP && instr.a() == loop_ip {
                            // Found the closing jump — skip past it
                            f.ip = (scan + 1) as u32;
                            return Action::Jump(f.ip);
                        }
                        scan += 1;
                    }
                    // Fallback: should not happen with valid bytecode
                    Action::Next
                }
            }

            // ── Int-constant local arithmetic: $x = $x op c ──
            OP_ADD_INT_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.tag() == TAG_INT {
                    let r = v.as_i48().checked_add(c).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(idx) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(idx) = v.add(Value::from_i48(c), &self.arena);
                    }
                }
                Action::Next
            }
            OP_SUB_INT_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.tag() == TAG_INT {
                    let r = v.as_i48().checked_sub(c).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(idx) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(idx) = v.sub_(Value::from_i48(c));
                    }
                }
                Action::Next
            }
            OP_MUL_INT_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.tag() == TAG_INT {
                    let r = v.as_i48().checked_mul(c).unwrap_or(v.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(idx) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(idx) = v.mul(Value::from_i48(c), &self.arena);
                    }
                }
                Action::Next
            }

            // ── Two-operand local store: dest = dest op src ──
            OP_ADD_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_add_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.add(b, &self.arena);
                    }
                }
                Action::Next
            }
            OP_SUB_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_sub_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.sub_(b);
                    }
                }
                Action::Next
            }
            OP_MUL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_mul_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.mul(b, &self.arena);
                    }
                }
                Action::Next
            }
            OP_DIV_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_div_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.div_(b);
                    }
                }
                Action::Next
            }

            // ── WHILE_LT_INT: while $a < b { body } (single-opcode while-loop) ──
            // Operand a = local slot, b = int constant, c = exit_wide_idx
            // If locals[a] < b, continue to body. Otherwise, jump to exit_ip from wide[c].
            OP_WHILE_LT_INT => {
                let f = &mut self.frames[top];
                let slot = i.a() as usize;
                let limit = i.b() as i32 as i64;
                let exit_wide_idx = i.c() as usize;
                let v = unsafe { *f.locals.get_unchecked(slot) };
                let val = if v.tag() == TAG_INT {
                    v.as_i48()
                } else {
                    v.as_int()
                };
                if val < limit {
                    Action::Next
                } else {
                    let exit_ip = f.code.wide_operands[exit_wide_idx] as u32;
                    f.ip = exit_ip;
                    Action::Jump(exit_ip)
                }
            }

            // ── Three-operand local store: dest = src1 op src2 ──
            OP_ADD_LOCAL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src1 = i.c() as usize;
                let src2 = i.b() as usize;
                let a = unsafe { *l.get_unchecked(src1) };
                let b = unsafe { *l.get_unchecked(src2) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_add_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.add(b, &self.arena);
                    }
                }
                Action::Next
            }
            OP_SUB_LOCAL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src1 = i.c() as usize;
                let src2 = i.b() as usize;
                let a = unsafe { *l.get_unchecked(src1) };
                let b = unsafe { *l.get_unchecked(src2) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_sub_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.sub_(b);
                    }
                }
                Action::Next
            }
            OP_MUL_LOCAL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src1 = i.c() as usize;
                let src2 = i.b() as usize;
                let a = unsafe { *l.get_unchecked(src1) };
                let b = unsafe { *l.get_unchecked(src2) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_mul_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.mul(b, &self.arena);
                    }
                }
                Action::Next
            }
            OP_DIV_LOCAL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src1 = i.c() as usize;
                let src2 = i.b() as usize;
                let a = unsafe { *l.get_unchecked(src1) };
                let b = unsafe { *l.get_unchecked(src2) };
                if a.tag() == TAG_INT && b.tag() == TAG_INT {
                    let r = checked_div_overflow(a.as_i48(), b.as_i48());
                    unsafe {
                        *l.get_unchecked_mut(dest) = Value::from_i48(r);
                    }
                } else {
                    unsafe {
                        *l.get_unchecked_mut(dest) = a.div_(b);
                    }
                }
                Action::Next
            }

            // ── WHILE_LT_LOCAL: while $a < $b { ... } (both locals) ──
            // Operand a = left slot, b = right slot, c = exit_wide_idx
            OP_WHILE_LT_LOCAL => {
                let f = &mut self.frames[top];
                let l = &f.locals;
                let left = i.a() as usize;
                let right = i.b() as usize;
                let exit_wide_idx = i.c() as usize;
                let a = unsafe { *l.get_unchecked(left) };
                let b = unsafe { *l.get_unchecked(right) };
                let cond = if a.is_int() && b.is_int() {
                    a.as_i48() < b.as_i48()
                } else if a.is_float() && b.is_float() {
                    a.as_f64() < b.as_f64()
                } else {
                    false
                };
                if cond {
                    Action::Next
                } else {
                    let exit_ip = f.code.wide_operands[exit_wide_idx] as u32;
                    f.ip = exit_ip;
                    Action::Jump(exit_ip)
                }
            }

            _ => Action::Next,
        }
    }

    fn enter_call(&mut self, func: crate::value::FuncValue, args: Vec<Value>) -> Action {
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
        });
        Action::PushFrame
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
    opcode_names!(op,
        OP_NOP => "OP_NOP",
        OP_PUSH_NIL => "OP_PUSH_NIL",
        OP_PUSH_TRUE => "OP_PUSH_TRUE",
        OP_PUSH_FALSE => "OP_PUSH_FALSE",
        OP_PUSH_INT => "OP_PUSH_INT",
        OP_PUSH_INT_WIDE => "OP_PUSH_INT_WIDE",
        OP_PUSH_FLOAT => "OP_PUSH_FLOAT",
        OP_PUSH_STR => "OP_PUSH_STR",
        OP_LOAD_LOCAL => "OP_LOAD_LOCAL",
        OP_STORE_LOCAL => "OP_STORE_LOCAL",
        OP_LOAD_GLOBAL => "OP_LOAD_GLOBAL",
        OP_STORE_GLOBAL => "OP_STORE_GLOBAL",
        OP_ADD => "OP_ADD",
        OP_SUB => "OP_SUB",
        OP_MUL => "OP_MUL",
        OP_DIV => "OP_DIV",
        OP_MOD => "OP_MOD",
        OP_NEG => "OP_NEG",
        OP_EQ => "OP_EQ",
        OP_NEQ => "OP_NEQ",
        OP_LT => "OP_LT",
        OP_GT => "OP_GT",
        OP_LE => "OP_LE",
        OP_GE => "OP_GE",
        OP_AND => "OP_AND",
        OP_OR => "OP_OR",
        OP_NOT => "OP_NOT",
        OP_IN => "OP_IN",
        OP_TYPE_TEST => "OP_TYPE_TEST",
        OP_JUMP => "OP_JUMP",
        OP_JUMP_IF_FALSE => "OP_JUMP_IF_FALSE",
        OP_JUMP_IF_NIL => "OP_JUMP_IF_NIL",
        OP_CALL => "OP_CALL",
        OP_CALL_METHOD => "OP_CALL_METHOD",
        OP_RETURN => "OP_RETURN",
        OP_HALT => "OP_HALT",
        OP_LEN => "OP_LEN",
        OP_GET_INDEX => "OP_GET_INDEX",
        OP_GET_FIELD => "OP_GET_FIELD",
        OP_MAKE_ARRAY => "OP_MAKE_ARRAY",
        OP_MAKE_DICT => "OP_MAKE_DICT",
        OP_ARRAY_PUSH => "OP_ARRAY_PUSH",
        OP_FOR_ITER_START => "OP_FOR_ITER_START",
        OP_FOR_ITER_NEXT => "OP_FOR_ITER_NEXT",
        OP_FOR_RANGE_START => "OP_FOR_RANGE_START",
        OP_FOR_RANGE_NEXT => "OP_FOR_RANGE_NEXT",
        OP_LOAD_LAMBDA => "OP_LOAD_LAMBDA",
        OP_COALESCE => "OP_COALESCE",
        OP_LOAD_LOCAL_LOAD_LOCAL => "OP_LL_LL",
        OP_ADD_LOCAL_LOCAL => "OP_ADD_LL",
        OP_SUB_LOCAL_LOCAL => "OP_SUB_LL",
        OP_LT_LOCAL_LOCAL => "OP_LT_LL",
        OP_LOAD_LOCAL_PUSH_INT => "OP_LL_PI",
        OP_LOAD_LOCAL_STORE_LOCAL => "OP_LL_SL",
        OP_FOR_NUMBER_LOOP => "OP_FOR_NUM_LP",
        OP_ADD_INT_LOCAL => "OP_ADD_INT_L",
        OP_ADD_LOCAL_STORE => "OP_ADD_L_ST",
        OP_SUB_INT_LOCAL => "OP_SUB_INT_L",
        OP_MUL_INT_LOCAL => "OP_MUL_INT_L",
        OP_WHILE_LT_INT => "OP_WHILE_LT",
        OP_MUL_LOCAL_STORE => "OP_MUL_L_ST",
        OP_DIV_LOCAL_STORE => "OP_DIV_L_ST",
        OP_SUB_LOCAL_STORE => "OP_SUB_L_ST",
        OP_ADD_LOCAL_LOCAL_STORE => "OP_ADD_LL_ST",
        OP_MUL_LOCAL_LOCAL_STORE => "OP_MUL_LL_ST",
        OP_SUB_LOCAL_LOCAL_STORE => "OP_SUB_LL_ST",
        OP_DIV_LOCAL_LOCAL_STORE => "OP_DIV_LL_ST",
        OP_WHILE_LT_LOCAL => "OP_WHILE_LT_L",
    )
}
