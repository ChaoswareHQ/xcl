use std::sync::Arc;
use bumpalo::Bump;
use smallvec::SmallVec;
use crate::bytecode::*;
use crate::value::{Value, BuiltinImpl, TypeDescriptor};

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
    catch_stack: Vec<(u32, u32)>,
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
    frames: Vec<Frame>,
    pending: Option<Frame>,
    closures: Vec<crate::value::FuncValue>,
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
            catch_stack: Vec::new(),
        });

        while let Some(top) = self.frames.len().checked_sub(1) {
            let frame = &self.frames[top];
            if frame.ip as usize >= frame.code.instructions.len() {
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
            OP_TRY_START => {
                let depth = self.stack.len() as u32;
                self.frames[top].catch_stack.push((i.a(), depth));
                Action::Next
            }
            OP_TRY_END => { self.frames[top].catch_stack.pop(); Action::Next }
            OP_THROW => {
                let f = &mut self.frames[top];
                if let Some((target, _)) = f.catch_stack.last().copied() {
                    f.ip = target;
                    Action::Next
                } else { Action::Return }
            }
            OP_CATCH => Action::Next,

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
            catch_stack: Vec::new(),
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
            catch_stack: Vec::new(),
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
}