use std::sync::Arc;
use bumpalo::Bump;
use smallvec::SmallVec;
use crate::bytecode::*;
use crate::value::{Value, BuiltinImpl, TypeDescriptor};

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
        let mut pairs: Vec<(u8, u64)> = self.counts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| if c > 0 { Some((i as u8, c)) } else { None })
            .collect();
        pairs.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
        pairs.truncate(n);
        pairs
    }
}

enum Action { Next, Jump(u32), Return, PushFrame }

struct Frame {
    code: Arc<Bytecode>,
    locals: SmallVec<[Value; 16]>,
    ip: u32,
    iter: Vec<Value>,
    iter_idx: u32,
    range_cur: i64,
    range_end: i64,
    stack_base: u32,
    /// Deferred closures to run when this frame returns (LIFO order).
    defer_stack: Vec<Value>,
}

pub struct DomainSlot {
    pub name:    Box<str>,
    pub methods: Vec<Option<BuiltinImpl>>,
    pub index:   hashbrown::HashMap<Box<str>, u32>,
}

pub struct VM {
    pub stack: Vec<Value>,
    globals: Vec<Value>,
    global_index: hashbrown::HashMap<Box<str>, u32>,
    pub global_table: Vec<Option<BuiltinImpl>>,
    pub domain_index: hashbrown::HashMap<Box<str>, u32>,
    pub domain_table: Vec<DomainSlot>,
    pub type_index:   hashbrown::HashMap<Box<str>, &'static TypeDescriptor>,
    pub arena: Bump,
    pub well_known: crate::wellknown::WellKnown,
    pub(crate) frames: Vec<Frame>,
    pending: Option<Frame>,
    closures: Vec<crate::value::FuncValue>,
    #[cfg(feature = "opcode-stats")]
    opcode_counter: OpcodeCounter,
}

impl Default for VM { fn default() -> Self { Self::new() } }

impl VM {
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
            #[cfg(feature = "opcode-stats")]
            opcode_counter: OpcodeCounter::new(),
        }
    }

    /// Fill in the well-known type/constructor names.
    /// Called by `xcl-std` after its own registration.
    pub fn set_well_known(&mut self, wk: crate::wellknown::WellKnown) {
        self.well_known = wk;
    }

    pub fn register_all(&mut self) {
        self.register_all_natives();
        self.register_all_types();
    }

    pub fn register_all_natives(&mut self) {
        use crate::native::NATIVE_REGISTRY;
        use crate::value::{NativeKind, BuiltinImpl};

        for entry in NATIVE_REGISTRY.iter() {
            let impl_ = match entry.kind {
                NativeKind::Pure => {
                    let f: crate::value::BuiltinFn = unsafe { std::mem::transmute(entry.ptr.0) };
                    BuiltinImpl::Pure(f)
                }
                NativeKind::Ctx => {
                    let f: crate::value::BuiltinCtxFn = unsafe { std::mem::transmute(entry.ptr.0) };
                    BuiltinImpl::Ctx(f)
                }
            };

            if let Some(domain) = entry.domain {
                let didx = *self.domain_index.entry(domain.into()).or_insert_with(|| {
                    let idx = self.domain_table.len() as u32;
                    self.domain_table.push(DomainSlot {
                        name: domain.into(),
                        methods: Vec::new(),
                        index: hashbrown::HashMap::new(),
                    });
                    idx
                });
                let slot = &mut self.domain_table[didx as usize];
                let midx = slot.methods.len() as u32;
                slot.methods.push(Some(impl_));
                slot.index.insert(entry.method.into(), midx);
            } else {
                let idx = self.global_table.len() as u32;
                self.global_table.push(Some(impl_));
                self.global_index.insert(entry.method.into(), idx);
            }
        }
    }

    pub fn register_all_types(&mut self) {
        use crate::native::TYPE_REGISTRY;
        for desc in TYPE_REGISTRY.iter() {
            self.type_index.insert(desc.name.into(), desc);
        }
    }

    pub fn resolve_builtin(&self, name: &str) -> Option<&BuiltinImpl> {
        self.global_index.get(name).and_then(|&idx| {
            self.global_table.get(idx as usize).and_then(|e| e.as_ref())
        })
    }

    pub fn resolve_domain_method(&self, domain: &str, method: &str) -> Option<&BuiltinImpl> {
        let &didx = self.domain_index.get(domain)?;
        let slot = self.domain_table.get(didx as usize)?;
        let &midx = slot.index.get(method)?;
        slot.methods.get(midx as usize).and_then(|e| e.as_ref())
    }

    #[inline(always)] fn pop(&mut self) -> Value { unsafe { self.stack.pop().unwrap_unchecked() } }
    #[inline(always)] fn push(&mut self, v: Value) { self.stack.push(v); }

    pub fn run(&mut self, bc: &Bytecode) -> Value {
        self.stack.clear();
        self.frames.clear();
        self.closures = bc.closures.clone();

        // Register user-defined functions in globals so OP_CALL can find them
        for (name, func) in &bc.user_functions {
            let idx = self.globals.len() as u32;
            self.globals.push(Value::from_func(func.clone(), &self.arena));
            self.global_index.insert(name.as_ref().into(), idx);
        }

        let n = bc.local_names.len().max(16);
        let mut locals = SmallVec::<[Value; 16]>::with_capacity(n);
        locals.resize(n, Value::nil());

        self.frames.push(Frame {
            code: Arc::new(bc.clone()),
            locals,
            ip: 0,
            iter: Vec::new(),
            iter_idx: 0,
            range_cur: 0,
            range_end: 0,
            stack_base: 0,
            defer_stack: Vec::new(),
        });

        while let Some(top) = self.frames.len().checked_sub(1) {
            let frame = &self.frames[top];
            if frame.ip as usize >= frame.code.instructions.len() {
                // Run deferred closures before returning
                if let Some(frame) = self.frames.last_mut() {
                    let mut stack = std::mem::take(&mut frame.defer_stack);
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
                } else { return r; }
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
                        let mut stack = std::mem::take(&mut frame.defer_stack);
                        stack.reverse();
                        for closure in stack {
                            let _ = self.call_value(closure, &[]);
                        }
                    }
                    let r = self.stack.pop().unwrap_or(Value::nil());
                    self.frames.pop();
                    if self.frames.is_empty() { return r; }
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
            OP_PUSH_NIL => { self.push(Value::nil()); Action::Next }
            OP_PUSH_TRUE => { self.push(Value::bool(true)); Action::Next }
            OP_PUSH_FALSE => { self.push(Value::bool(false)); Action::Next }
            OP_PUSH_INT => { self.push(Value::from_i48(i.a() as i64)); Action::Next }
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
                unsafe { *self.frames[top].locals.get_unchecked_mut(i.a() as usize) = v; }
                Action::Next
            }
            OP_LOAD_GLOBAL => {
                let v = self.globals.get(i.a() as usize).copied().unwrap_or(Value::nil());
                self.push(v);
                Action::Next
            }
            OP_STORE_GLOBAL => {
                let v = self.pop();
                if (i.a() as usize) < self.globals.len() { self.globals[i.a() as usize] = v; }
                Action::Next
            }
            OP_ADD => { let b = self.pop(); let a = self.pop(); self.push(a.add(b, &self.arena)); Action::Next }
            OP_SUB => { let b = self.pop(); let a = self.pop(); self.push(a.sub_(b)); Action::Next }
            OP_MUL => { let b = self.pop(); let a = self.pop(); self.push(a.mul(b, &self.arena)); Action::Next }
            OP_DIV => { let b = self.pop(); let a = self.pop(); self.push(a.div_(b)); Action::Next }
            OP_MOD => { let b = self.pop(); let a = self.pop(); self.push(a.mod_(b)); Action::Next }
            OP_NEG => { let a = self.pop(); self.push(a.neg_()); Action::Next }
            OP_EQ  => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.eq_(b))); Action::Next }
            OP_NEQ => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(!a.eq_(b))); Action::Next }
            OP_LT  => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.lt(b))); Action::Next }
            OP_GT  => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.gt(b))); Action::Next }
            OP_LE  => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.lt(b) || a.eq_(b))); Action::Next }
            OP_GE  => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.gt(b) || a.eq_(b))); Action::Next }
            OP_AND => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.is_truthy() && b.is_truthy())); Action::Next }
            OP_OR  => { let b = self.pop(); let a = self.pop(); self.push(Value::bool(a.is_truthy() || b.is_truthy())); Action::Next }
            OP_NOT => { let a = self.pop(); self.push(a.not_()); Action::Next }
            OP_IN => {
                let c = self.pop(); let item = self.pop();
                let found = c.as_array().map(|a| a.iter().any(|v| v.eq_(item))).unwrap_or(false);
                self.push(Value::bool(found)); Action::Next
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
                            } else { false }
                        } else { false }
                    } else {
                        // Check type index for registered types
                        self.type_index.contains_key(tn)
                            && !value.is_nil()
                    }
                } else {
                    false
                };
                self.push(Value::bool(is_type)); Action::Next
            }
            OP_JUMP => Action::Jump(i.a()),
            OP_JUMP_IF_FALSE => {
                let c = self.pop();
                if c.is_truthy() { Action::Next } else { Action::Jump(i.a()) }
            }
            OP_JUMP_IF_NIL => {
                let v = self.pop();
                if v.is_nil() { Action::Jump(i.a()) } else { Action::Next }
            }
            OP_CALL => {
                let n = i.a() as usize;
                let mut args = Vec::with_capacity(n);
                for _ in 0..n { args.push(self.pop()); }
                args.reverse();
                let callee = self.pop();

                if let Some(name) = callee.as_str() {
                    // 1. Check global builtins first
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
                    // 2. Check domain methods (e.g. "io.read")
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
                    // 2b. Domain expansion: if callee is a domain name, check for block/dict args.
                    if self.domain_index.contains_key(name) {
                        // Case 1: one arg is a function — execute it with domain methods in scope.
                        if let Some((func_idx, func)) = args.iter().enumerate().find_map(|(i, a)| {
                            a.as_func().map(|f| (i, f.clone()))
                        }) {
                            let call_args: Vec<Value> = args.into_iter().enumerate()
                                .filter(|(i, _)| *i != func_idx)
                                .map(|(_, v)| v)
                                .collect();
                            let &didx = self.domain_index.get(name).unwrap();
                            let slot = &self.domain_table[didx as usize];
                            let mut saved: Vec<(Box<str>, Option<u32>)> = Vec::new();
                            for (method_name, &midx) in slot.index.iter() {
                                let existing = self.global_index.insert(method_name.clone(), self.global_table.len() as u32);
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
                        if let Some((_map_idx, map)) = args.iter().enumerate().find_map(|(i, a)| {
                            a.as_map().map(|m| (i, m))
                        }) {
                            let &didx = self.domain_index.get(name).unwrap();
                            // Collect method calls to avoid borrow conflict
                            let calls: Vec<(u32, Value)> = self.domain_table[didx as usize].index.iter()
                                .filter_map(|(method_name, &midx)| {
                                    map.get(&**method_name).map(|v| (midx, *v))
                                })
                                .collect();
                            let mut last_result = Value::nil();
                            for (midx, value) in calls {
                                let impl_ = self.domain_table[didx as usize].methods[midx as usize]
                                    .as_ref().copied().unwrap();
                                last_result = impl_.call(self, &[value]);
                            }
                            self.push(last_result);
                            return Action::Next;
                        }
                    }
                    // 3. Fallback: search all globals by name
                    let found = self.globals.iter().find_map(|g| {
                        g.as_func().filter(|f| &*f.name == name).cloned()
                    });
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
                for _ in 0..nargs { args.push(self.pop()); }
                args.reverse();
                let receiver = self.pop();

                // Look up the domain from the receiver's type.
                // For typed maps (Result, Error, Handle), use the subtype to find the domain.
                let domain_name = if let Some(subtype) = receiver.map_subtype() {
                    match subtype {
                        1 => Some("result"),   // SUBTYPE_RESULT
                        2 => Some("error"),    // SUBTYPE_ERROR
                        3 => Some("handle"),   // SUBTYPE_HANDLE
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
            OP_LEN => { let v = self.pop(); self.push(Value::from_i48(v.len() as i64)); Action::Next }
            OP_GET_INDEX => {
                let idx = self.pop();
                let target = self.pop();
                let result = if let Some(arr) = target.as_array() {
                    let i = idx.as_int();
                    let i = if i < 0 { arr.len() as i64 + i } else { i };
                    arr.get(i as usize).copied().unwrap_or(Value::nil())
                } else if let Some(m) = target.as_map() {
                    idx.as_str().and_then(|k| m.get(k).copied()).unwrap_or(Value::nil())
                } else { Value::nil() };
                self.push(result); Action::Next
            }
            OP_GET_FIELD => {
                let field_idx = i.a() as usize;
                let string_table = self.frames[top].code.string_table.clone();
                let field = &string_table[field_idx];
                let target = self.pop();
                let v = target.as_map().and_then(|m| m.get(&**field).copied()).unwrap_or(Value::nil());
                self.push(v); Action::Next
            }
            OP_MAKE_ARRAY => {
                let n = i.a() as usize;
                let mut elements = Vec::with_capacity(n);
                for _ in 0..n { elements.push(self.pop()); }
                elements.reverse();
                self.push(Value::from_array(elements, &self.arena)); Action::Next
            }
            OP_MAKE_DICT => {
                let n = i.a() as usize;
                let mut m = std::collections::HashMap::with_capacity(n);
                for _ in 0..n {
                    let v = self.pop();
                    let k = self.pop();
                    m.insert(k.as_string(), v);
                }
                self.push(Value::from_map(m, &self.arena)); Action::Next
            }
            OP_ARRAY_PUSH => {
                let elem = self.pop();
                let target = self.pop();
                if let Some(arr) = target.as_array_mut() { arr.push(elem); }
                self.push(target); Action::Next
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
                } else { Action::Jump(i.a()) }
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
                } else { Action::Jump(i.a()) }
            }
            OP_LOAD_LAMBDA => {
                let f = self.closures[i.a() as usize].clone();
                self.push(Value::from_func(f, &self.arena));
                Action::Next
            }
            OP_COALESCE => {
                let r = self.pop(); let l = self.pop();
                self.push(if l.is_nil() { r } else { l }); Action::Next
            }
            OP_LOAD_LOCAL_LOAD_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(a); self.push(b);
                Action::Next
            }
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
            OP_LT_LOCAL_LOCAL => {
                let l = &self.frames[top].locals;
                let a = unsafe { *l.get_unchecked(i.a() as usize) };
                let b = unsafe { *l.get_unchecked(i.b() as usize) };
                self.push(Value::bool(a.lt(b)));
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
                unsafe { *self.frames[top].locals.get_unchecked_mut(i.b() as usize) = v; }
                Action::Next
            }

            // ── Integer-specialized arithmetic ──
            OP_ADD_INT => {
                let b = self.pop(); let a = self.pop();
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_add(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 + b.as_i48() as f64) as i64
                    });
                    self.push(Value::from_i48(r));
                } else {
                    self.push(a.add(b, &self.arena));
                }
                Action::Next
            }
            OP_SUB_INT => {
                let b = self.pop(); let a = self.pop();
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_sub(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 - b.as_i48() as f64) as i64
                    });
                    self.push(Value::from_i48(r));
                } else {
                    self.push(a.sub_(b));
                }
                Action::Next
            }
            OP_MUL_INT => {
                let b = self.pop(); let a = self.pop();
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_mul(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 * b.as_i48() as f64) as i64
                    });
                    self.push(Value::from_i48(r));
                } else {
                    self.push(a.mul(b, &self.arena));
                }
                Action::Next
            }
            OP_LT_INT => {
                let b = self.pop(); let a = self.pop();
                let r = if a.is_int() && b.is_int() {
                    a.as_i48() < b.as_i48()
                } else {
                    a.lt(b)
                };
                self.push(Value::bool(r));
                Action::Next
            }
            OP_GT_INT => {
                let b = self.pop(); let a = self.pop();
                let r = if a.is_int() && b.is_int() {
                    a.as_i48() > b.as_i48()
                } else {
                    a.gt(b)
                };
                self.push(Value::bool(r));
                Action::Next
            }
            OP_EQ_INT => {
                let b = self.pop(); let a = self.pop();
                let r = if a.is_int() && b.is_int() {
                    a.as_i48() == b.as_i48()
                } else {
                    a.eq_(b)
                };
                self.push(Value::bool(r));
                Action::Next
            }

            // ── Superinstructions ──
            OP_INC_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_add(1).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(idx) = Value::from_i48(r); }
                } else {
                    let r = v.add(Value::from_i48(1), &self.arena);
                    unsafe { *l.get_unchecked_mut(idx) = r; }
                }
                Action::Next
            }
            OP_DEC_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_sub(1).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(idx) = Value::from_i48(r); }
                } else {
                    let r = v.sub_(Value::from_i48(1));
                    unsafe { *l.get_unchecked_mut(idx) = r; }
                }
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
            OP_ADD_INT_STORE_LOCAL => {
                // a = dest slot, b = source slot, c = int constant (as u16)
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let c = i.c() as i64;
                let v = unsafe { *l.get_unchecked(src) };
                if v.is_int() {
                    let r = v.as_i48().checked_add(c).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(dest) = Value::from_i48(r); }
                } else {
                    let r = v.add(Value::from_i48(c), &self.arena);
                    unsafe { *l.get_unchecked_mut(dest) = r; }
                }
                Action::Next
            }
            OP_INC_LOCAL_INT => {
                // a = local slot, b = int constant
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_add(c).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(idx) = Value::from_i48(r); }
                } else {
                    let r = v.add(Value::from_i48(c), &self.arena);
                    unsafe { *l.get_unchecked_mut(idx) = r; }
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
                    unsafe { *f.locals.get_unchecked_mut(var_slot) = Value::from_i48(next_val); }
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

            // ── ADD_INT_LOCAL: $x = $x + c ──
            OP_ADD_INT_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_add(c).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(idx) = Value::from_i48(r); }
                } else {
                    let r = v.add(Value::from_i48(c), &self.arena);
                    unsafe { *l.get_unchecked_mut(idx) = r; }
                }
                Action::Next
            }

            // ── ADD_LOCAL_STORE: $a = $a + $b (direct local-to-local add) ──
            OP_ADD_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                // Fast integer path
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_add(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 + b.as_i48() as f64) as i64
                    });
                    unsafe { *l.get_unchecked_mut(dest) = Value::from_i48(r); }
                } else {
                    unsafe { *l.get_unchecked_mut(dest) = a.add(b, &self.arena); }
                }
                Action::Next
            }

            // ── SUB_INT_LOCAL: $x = $x - c ──
            OP_SUB_INT_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_sub(c).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(idx) = Value::from_i48(r); }
                } else {
                    let r = v.sub_(Value::from_i48(c));
                    unsafe { *l.get_unchecked_mut(idx) = r; }
                }
                Action::Next
            }

            // ── MUL_INT_LOCAL: $x = $x * c ──
            OP_MUL_INT_LOCAL => {
                let l = &mut self.frames[top].locals;
                let idx = i.a() as usize;
                let c = i.b() as i64;
                let v = unsafe { *l.get_unchecked(idx) };
                if v.is_int() {
                    let r = v.as_i48().checked_mul(c).unwrap_or(v.as_i48());
                    unsafe { *l.get_unchecked_mut(idx) = Value::from_i48(r); }
                } else {
                    let r = v.mul(Value::from_i48(c), &self.arena);
                    unsafe { *l.get_unchecked_mut(idx) = r; }
                }
                Action::Next
            }

            // ── WHILE_LT_INT: while $a < b { body } (single-opcode while-loop) ──
            // Operand a = local slot, b = int constant
            // If locals[a] < b, continue to body. Otherwise, skip past body.
            OP_WHILE_LT_INT => {
                let f = &mut self.frames[top];
                let slot = i.a() as usize;
                let limit = i.b() as i32 as i64;
                let v = unsafe { *f.locals.get_unchecked(slot) };
                let val = if v.is_int() { v.as_i48() } else { v.as_int() };
                if val < limit {
                    // Continue to loop body
                    Action::Next
                } else {
                    // Skip past body — find the closing OP_JUMP
                    let loop_ip = f.ip;
                    f.ip += 1;
                    let code = &f.code.instructions;
                    let mut scan = f.ip as usize;
                    while scan < code.len() {
                        let instr = unsafe { *code.get_unchecked(scan) };
                        if instr.op() == OP_JUMP && instr.a() == loop_ip {
                            f.ip = (scan + 1) as u32;
                            return Action::Jump(f.ip);
                        }
                        scan += 1;
                    }
                    Action::Next
                }
            }

            // ── SUB_LOCAL_STORE: $a = $a - $b ──
            OP_SUB_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_sub(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 - b.as_i48() as f64) as i64
                    });
                    unsafe { *l.get_unchecked_mut(dest) = Value::from_i48(r); }
                } else {
                    unsafe { *l.get_unchecked_mut(dest) = a.sub_(b); }
                }
                Action::Next
            }

            // ── ADD_LOCAL_LOCAL_STORE: $c = $a + $b ──
            OP_ADD_LOCAL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let b_val = i.b() as u32;
                let src1 = (b_val >> 16) as usize;
                let src2 = (b_val & 0xFFFF) as usize;
                let a = unsafe { *l.get_unchecked(src1) };
                let b = unsafe { *l.get_unchecked(src2) };
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_add(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 + b.as_i48() as f64) as i64
                    });
                    unsafe { *l.get_unchecked_mut(dest) = Value::from_i48(r); }
                } else {
                    unsafe { *l.get_unchecked_mut(dest) = a.add(b, &self.arena); }
                }
                Action::Next
            }

            // ── MUL_LOCAL_LOCAL_STORE: $c = $a * $b ──
            OP_MUL_LOCAL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let b_val = i.b() as u32;
                let src1 = (b_val >> 16) as usize;
                let src2 = (b_val & 0xFFFF) as usize;
                let a = unsafe { *l.get_unchecked(src1) };
                let b = unsafe { *l.get_unchecked(src2) };
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_mul(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 * b.as_i48() as f64) as i64
                    });
                    unsafe { *l.get_unchecked_mut(dest) = Value::from_i48(r); }
                } else {
                    unsafe { *l.get_unchecked_mut(dest) = a.mul(b, &self.arena); }
                }
                Action::Next
            }

            // ── MUL_LOCAL_STORE: $a = $a * $b (direct local-to-local mul) ──
            OP_MUL_LOCAL_STORE => {
                let l = &mut self.frames[top].locals;
                let dest = i.a() as usize;
                let src = i.b() as usize;
                let a = unsafe { *l.get_unchecked(dest) };
                let b = unsafe { *l.get_unchecked(src) };
                if a.is_int() && b.is_int() {
                    let r = a.as_i48().checked_mul(b.as_i48()).unwrap_or_else(|| {
                        (a.as_i48() as f64 * b.as_i48() as f64) as i64
                    });
                    unsafe { *l.get_unchecked_mut(dest) = Value::from_i48(r); }
                } else {
                    unsafe { *l.get_unchecked_mut(dest) = a.mul(b, &self.arena); }
                }
                Action::Next
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
        while locals.len() < n { locals.push(Value::nil()); }

        let base = self.stack.len() as u32;
        self.pending = Some(Frame {
            code: Arc::clone(bc),
            locals,
            ip: 0,
            iter: Vec::new(),
            iter_idx: 0,
            range_cur: 0,
            range_end: 0,
            stack_base: base,
            defer_stack: Vec::new(),
        });
        Action::PushFrame
    }

    /// Call an XCL closure value with the given args. Reentrant.
    /// Runs the VM dispatch loop until the closure frame returns, then returns its result.
    pub fn call_value(&mut self, func: Value, args: &[Value]) -> Value {
        let func_ref = match func.as_func() {
            Some(f) => f,
            None    => return Value::nil(),
        };

        let bc = match &func_ref.bytecode {
            Some(b) => Arc::clone(b),
            None    => return Value::nil(),
        };

        // Save and replace closures table so OP_LOAD_LAMBDA works for nested closures
        let saved_closures = std::mem::replace(&mut self.closures, bc.closures.clone());

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
            iter: Vec::new(),
            iter_idx: 0,
            range_cur: 0,
            range_end: 0,
            stack_base,
            defer_stack: Vec::new(),
        });

        // Run the VM dispatch loop until the frame we just pushed returns
        loop {
            let top = match self.frames.len().checked_sub(1) {
                Some(i) => i,
                None    => {
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
                    let mut stack = std::mem::take(&mut frame.defer_stack);
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
                Action::Next => self.frames[top].ip += 1,
                Action::Jump(to) => self.frames[top].ip = to,
                Action::Return => {
                    // Run deferred closures before returning
                    if let Some(frame) = self.frames.last_mut() {
                        let mut stack = std::mem::take(&mut frame.defer_stack);
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
                }
            }
        }

        self.closures = saved_closures;
        Value::nil()
    }

    /// Returns the opcode dispatch counts as a HashMap<opcode, count>.
    /// Only available when the `opcode-stats` feature is enabled.
    #[cfg(feature = "opcode-stats")]
    pub fn opcode_counts(&self) -> std::collections::HashMap<u8, u64> {
        let mut map = std::collections::HashMap::new();
        for (op, &count) in self.opcode_counter.counts.iter().enumerate() {
            if count > 0 {
                map.insert(op as u8, count);
            }
        }
        map
    }

    /// Push a closure onto the current frame's defer stack.
    pub fn push_defer(&mut self, closure: Value) {
        if let Some(frame) = self.frames.last_mut() {
            frame.defer_stack.push(closure);
        }
    }

    /// Prints a histogram of the top N opcodes by dispatch count.
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
    match op {
        OP_NOP => "OP_NOP".to_string(),
        OP_PUSH_NIL => "OP_PUSH_NIL".to_string(),
        OP_PUSH_TRUE => "OP_PUSH_TRUE".to_string(),
        OP_PUSH_FALSE => "OP_PUSH_FALSE".to_string(),
        OP_PUSH_INT => "OP_PUSH_INT".to_string(),
        OP_PUSH_INT_WIDE => "OP_PUSH_INT_WIDE".to_string(),
        OP_PUSH_FLOAT => "OP_PUSH_FLOAT".to_string(),
        OP_PUSH_STR => "OP_PUSH_STR".to_string(),
        OP_LOAD_LOCAL => "OP_LOAD_LOCAL".to_string(),
        OP_STORE_LOCAL => "OP_STORE_LOCAL".to_string(),
        OP_LOAD_GLOBAL => "OP_LOAD_GLOBAL".to_string(),
        OP_STORE_GLOBAL => "OP_STORE_GLOBAL".to_string(),
        OP_ADD => "OP_ADD".to_string(),
        OP_SUB => "OP_SUB".to_string(),
        OP_MUL => "OP_MUL".to_string(),
        OP_DIV => "OP_DIV".to_string(),
        OP_MOD => "OP_MOD".to_string(),
        OP_NEG => "OP_NEG".to_string(),
        OP_EQ => "OP_EQ".to_string(),
        OP_NEQ => "OP_NEQ".to_string(),
        OP_LT => "OP_LT".to_string(),
        OP_GT => "OP_GT".to_string(),
        OP_LE => "OP_LE".to_string(),
        OP_GE => "OP_GE".to_string(),
        OP_AND => "OP_AND".to_string(),
        OP_OR => "OP_OR".to_string(),
        OP_NOT => "OP_NOT".to_string(),
        OP_IN => "OP_IN".to_string(),
        OP_TYPE_TEST => "OP_TYPE_TEST".to_string(),
        OP_JUMP => "OP_JUMP".to_string(),
        OP_JUMP_IF_FALSE => "OP_JUMP_IF_FALSE".to_string(),
        OP_JUMP_IF_NIL => "OP_JUMP_IF_NIL".to_string(),
        OP_CALL => "OP_CALL".to_string(),
        OP_CALL_METHOD => "OP_CALL_METHOD".to_string(),
        OP_RETURN => "OP_RETURN".to_string(),
        OP_HALT => "OP_HALT".to_string(),
        OP_LEN => "OP_LEN".to_string(),
        OP_GET_INDEX => "OP_GET_INDEX".to_string(),
        OP_GET_FIELD => "OP_GET_FIELD".to_string(),
        OP_MAKE_ARRAY => "OP_MAKE_ARRAY".to_string(),
        OP_MAKE_DICT => "OP_MAKE_DICT".to_string(),
        OP_ARRAY_PUSH => "OP_ARRAY_PUSH".to_string(),
        OP_FOR_ITER_START => "OP_FOR_ITER_START".to_string(),
        OP_FOR_ITER_NEXT => "OP_FOR_ITER_NEXT".to_string(),
        OP_FOR_RANGE_START => "OP_FOR_RANGE_START".to_string(),
        OP_FOR_RANGE_NEXT => "OP_FOR_RANGE_NEXT".to_string(),
        OP_LOAD_LAMBDA => "OP_LOAD_LAMBDA".to_string(),
        OP_COALESCE => "OP_COALESCE".to_string(),
        OP_LOAD_LOCAL_LOAD_LOCAL => "OP_LL_LL".to_string(),
        OP_ADD_LOCAL_LOCAL => "OP_ADD_LL".to_string(),
        OP_SUB_LOCAL_LOCAL => "OP_SUB_LL".to_string(),
        OP_LT_LOCAL_LOCAL => "OP_LT_LL".to_string(),
        OP_LOAD_LOCAL_PUSH_INT => "OP_LL_PI".to_string(),
        OP_LOAD_LOCAL_STORE_LOCAL => "OP_LL_SL".to_string(),
        OP_FOR_NUMBER_LOOP => "OP_FOR_NUM_LP".to_string(),
        OP_ADD_INT_LOCAL => "OP_ADD_INT_L".to_string(),
        OP_ADD_LOCAL_STORE => "OP_ADD_L_ST".to_string(),
        OP_SUB_INT_LOCAL => "OP_SUB_INT_L".to_string(),
        OP_MUL_INT_LOCAL => "OP_MUL_INT_L".to_string(),
        OP_WHILE_LT_INT => "OP_WHILE_LT".to_string(),
        OP_MUL_LOCAL_STORE => "OP_MUL_L_ST".to_string(),
        OP_SUB_LOCAL_STORE => "OP_SUB_L_ST".to_string(),
        OP_ADD_LOCAL_LOCAL_STORE => "OP_ADD_LL_ST".to_string(),
        OP_MUL_LOCAL_LOCAL_STORE => "OP_MUL_LL_ST".to_string(),
        _ => format!("0x{:02X}", op),
    }
}