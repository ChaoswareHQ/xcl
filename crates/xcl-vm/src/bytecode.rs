use std::sync::Arc;
use crate::ast::{AstNode, BinaryOperator, UnaryOperator, TypeKind};
use crate::value::FuncValue;

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Instr(pub u64);

impl Instr {
    #[inline(always)] pub const fn op(self) -> u8 { (self.0 & 0xFF) as u8 }
    #[inline(always)] pub const fn a(self)  -> u32 { ((self.0 >> 8)  & 0xFFFF_FFFF) as u32 }
    #[inline(always)] pub const fn b(self)  -> u16 { ((self.0 >> 40) & 0xFFFF) as u16 }
    #[inline(always)] pub const fn c(self)  -> u8  { ((self.0 >> 56) & 0xFF) as u8 }

    #[inline(always)]
    pub const fn make(op: u8, a: u32, b: u16, c: u8) -> Self {
        Instr((op as u64) | ((a as u64) << 8) | ((b as u64) << 40) | ((c as u64) << 56))
    }

    #[inline(always)] pub const fn none(op: u8)               -> Self { Self::make(op, 0, 0, 0) }
    #[inline(always)] pub const fn a1(op: u8, a: u32)         -> Self { Self::make(op, a, 0, 0) }
    #[inline(always)] pub const fn a2(op: u8, a: u32, b: u16) -> Self { Self::make(op, a, b, 0) }
}

pub const OP_NOP:             u8 = 0;
pub const OP_PUSH_NIL:        u8 = 1;
pub const OP_PUSH_TRUE:       u8 = 2;
pub const OP_PUSH_FALSE:      u8 = 3;
pub const OP_PUSH_INT:        u8 = 4;
pub const OP_PUSH_INT_WIDE:   u8 = 5;
pub const OP_PUSH_FLOAT:      u8 = 6;
pub const OP_PUSH_STR:        u8 = 7;
pub const OP_LOAD_LOCAL:      u8 = 8;
pub const OP_STORE_LOCAL:     u8 = 9;
pub const OP_LOAD_GLOBAL:     u8 = 10;
pub const OP_STORE_GLOBAL:    u8 = 11;

pub const OP_ADD:             u8 = 12;
pub const OP_SUB:             u8 = 13;
pub const OP_MUL:             u8 = 14;
pub const OP_DIV:             u8 = 15;
pub const OP_MOD:             u8 = 16;
pub const OP_NEG:             u8 = 17;

pub const OP_EQ:              u8 = 18;
pub const OP_NEQ:             u8 = 19;
pub const OP_LT:              u8 = 20;
pub const OP_GT:              u8 = 21;
pub const OP_LE:              u8 = 22;
pub const OP_GE:              u8 = 23;
pub const OP_AND:             u8 = 24;
pub const OP_OR:              u8 = 25;
pub const OP_NOT:             u8 = 26;
pub const OP_IN:              u8 = 27;

pub const OP_JUMP:            u8 = 28;
pub const OP_JUMP_IF_FALSE:   u8 = 29;
pub const OP_JUMP_IF_NIL:     u8 = 30;

pub const OP_CALL:            u8 = 32;
pub const OP_RETURN:          u8 = 33;
pub const OP_HALT:            u8 = 34;

pub const OP_MAKE_ARRAY:      u8 = 35;
pub const OP_MAKE_DICT:       u8 = 36;
pub const OP_GET_INDEX:       u8 = 37;
pub const OP_GET_FIELD:       u8 = 38;
pub const OP_ARRAY_PUSH:      u8 = 39;
pub const OP_LEN:             u8 = 41;

pub const OP_FOR_ITER_START:  u8 = 42;
pub const OP_FOR_ITER_NEXT:   u8 = 43;
pub const OP_FOR_RANGE_START: u8 = 44;
pub const OP_FOR_RANGE_NEXT:  u8 = 45;

pub const OP_LOAD_LAMBDA:     u8 = 46;

pub const OP_TRY_START:       u8 = 47;
pub const OP_TRY_END:         u8 = 48;
pub const OP_THROW:           u8 = 49;
pub const OP_CATCH:           u8 = 50;
pub const OP_COALESCE:        u8 = 51;

/// Type test: check if a value is of a given type (by tag field or type index)
pub const OP_TYPE_TEST:         u8 = 52;

pub const OP_LOAD_LOCAL_LOAD_LOCAL: u8 = 60;
pub const OP_ADD_LOCAL_LOCAL:       u8 = 61;
pub const OP_SUB_LOCAL_LOCAL:       u8 = 62;
pub const OP_LT_LOCAL_LOCAL:        u8 = 63;
pub const OP_LOAD_LOCAL_PUSH_INT:   u8 = 64;
pub const OP_LOAD_LOCAL_STORE_LOCAL:u8 = 65;

/// Call a method on a value by looking up its type's domain.
/// Stack: receiver, arg1, arg2, ... -> result
/// Operand a = field name string index, b = arg count (excluding receiver)
pub const OP_CALL_METHOD: u8 = 66;

#[derive(Debug, Clone)]
pub struct Bytecode {
    pub instructions:  Arc<[Instr]>,
    pub wide_operands: Arc<[i64]>,
    pub string_table:  Arc<[Box<str>]>,
    pub local_names:   Arc<[Box<str>]>,
    pub user_functions: Vec<(Box<str>, FuncValue)>,
    pub closures:      Vec<FuncValue>,
}

struct Compiler {
    code:      Vec<Instr>,
    wide:      Vec<i64>,
    strings:   Vec<Box<str>>,
    locals:    Vec<Box<str>>,
    functions: Vec<(Box<str>, FuncValue)>,
    closures:  Vec<FuncValue>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            code: Vec::with_capacity(64),
            wide: Vec::new(),
            strings: Vec::new(),
            locals: Vec::new(),
            functions: Vec::new(),
            closures: Vec::new(),
        }
    }

    #[inline(always)] fn emit(&mut self, i: Instr) { self.code.push(i); }
    #[inline(always)] fn ip(&self) -> u32 { self.code.len() as u32 }

    fn patch_jump(&mut self, at: u32, target: u32) {
        let i = &mut self.code[at as usize];
        *i = Instr::make(i.op(), target, i.b(), i.c());
    }

    #[inline]
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| &**x == s) { return i as u32; }
        let i = self.strings.len() as u32;
        self.strings.push(s.into());
        i
    }

    #[inline]
    fn local(&mut self, name: &str) -> u32 {
        if let Some(i) = self.locals.iter().position(|x| &**x == name) { return i as u32; }
        let i = self.locals.len() as u32;
        self.locals.push(name.into());
        i
    }

    #[inline]
    fn wide(&mut self, v: i64) -> u32 {
        let i = self.wide.len() as u32;
        self.wide.push(v);
        i
    }

    fn try_fuse_load_load(&mut self, second: u32) -> bool {
        if let Some(prev) = self.code.last().copied()
            && prev.op() == OP_LOAD_LOCAL
        {
            let a = prev.a();
            self.code.pop();
            self.emit(Instr::make(OP_LOAD_LOCAL_LOAD_LOCAL, a, second as u16, 0));
            return true;
        }
        false
    }

    fn try_fuse_add_local_local(&mut self) -> bool {
        if self.code.len() < 2 { return false; }
        let l2 = self.code[self.code.len() - 1].op();
        let l1 = self.code[self.code.len() - 2].op();
        if l1 == OP_LOAD_LOCAL && l2 == OP_LOAD_LOCAL {
            let a = self.code[self.code.len() - 2].a();
            let b_slot = self.code[self.code.len() - 1].a();
            self.code.truncate(self.code.len() - 2);
            self.emit(Instr::make(OP_ADD_LOCAL_LOCAL, a, b_slot as u16, 0));
            return true;
        }
        false
    }

    fn compile(&mut self, ast: &[AstNode]) -> Bytecode {
        for n in ast { self.node(n); }
        self.emit(Instr::none(OP_HALT));
        Bytecode {
            instructions: std::mem::take(&mut self.code).into(),
            wide_operands: std::mem::take(&mut self.wide).into(),
            string_table: std::mem::take(&mut self.strings).into(),
            local_names: std::mem::take(&mut self.locals).into(),
            user_functions: std::mem::take(&mut self.functions),
            closures: std::mem::take(&mut self.closures),
        }
    }

    fn node(&mut self, node: &AstNode) {
        match node {
            AstNode::Assign { name, value, .. } => {
                self.node(value);
                let slot = self.local(name);
                self.emit(Instr::a1(OP_STORE_LOCAL, slot));
            }
            AstNode::Return { value, .. } => {
                if let Some(v) = value { self.node(v); }
                else { self.emit(Instr::none(OP_PUSH_NIL)); }
                self.emit(Instr::none(OP_RETURN));
            }
            AstNode::Break { .. } | AstNode::Continue { .. } => {}
            AstNode::Throw { value, .. } => { self.node(value); self.emit(Instr::none(OP_THROW)); }

            AstNode::If { condition, then_branch, else_branch, .. } => {
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in then_branch { self.node(s); }
                if let Some(e) = else_branch {
                    let j2 = self.ip();
                    self.emit(Instr::a1(OP_JUMP, 0));
                    let t = self.ip();
                    self.patch_jump(j, t);
                    for s in e { self.node(s); }
                    let end = self.ip();
                    self.patch_jump(j2, end);
                } else {
                    let end = self.ip();
                    self.patch_jump(j, end);
                }
            }

            AstNode::For { variable, iterable, body, .. } => {
                if let AstNode::Range { start, end, .. } = iterable.as_ref() {
                    if let Some(s) = start { self.node(s); }
                    else { self.emit(Instr::a1(OP_PUSH_INT, 0)); }
                    if let Some(e) = end { self.node(e); }
                    else { self.emit(Instr::a1(OP_PUSH_INT, 0)); }
                    self.emit(Instr::none(OP_FOR_RANGE_START));
                    let start_ip = self.ip();
                    let next = self.ip();
                    self.emit(Instr::a1(OP_FOR_RANGE_NEXT, 0));
                    let var = self.local(variable);
                    self.emit(Instr::a1(OP_STORE_LOCAL, var));
                    for s in body { self.node(s); }
                    self.emit(Instr::a1(OP_JUMP, start_ip));
                    let end = self.ip();
                    self.patch_jump(next, end);
                } else {
                    self.node(iterable);
                    self.emit(Instr::none(OP_FOR_ITER_START));
                    let start_ip = self.ip();
                    let next = self.ip();
                    self.emit(Instr::a1(OP_FOR_ITER_NEXT, 0));
                    let var = self.local(variable);
                    self.emit(Instr::a1(OP_STORE_LOCAL, var));
                    for s in body { self.node(s); }
                    self.emit(Instr::a1(OP_JUMP, start_ip));
                    let end = self.ip();
                    self.patch_jump(next, end);
                }
            }

            AstNode::While { condition, body, .. } => {
                let start = self.ip();
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in body { self.node(s); }
                self.emit(Instr::a1(OP_JUMP, start));
                let end = self.ip();
                self.patch_jump(j, end);
            }

            AstNode::Try { body, catch_body, catch_var, .. } => {
                let cj = self.ip();
                self.emit(Instr::a1(OP_TRY_START, 0));
                for s in body { self.node(s); }
                self.emit(Instr::none(OP_TRY_END));
                let skip = self.ip();
                self.emit(Instr::a1(OP_JUMP, 0));
                let target = self.ip();
                self.patch_jump(cj, target);
                self.emit(Instr::none(OP_CATCH));
                if let Some(c) = catch_body {
                    if let Some(name) = catch_var {
                        let slot = self.local(name);
                        self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                    }
                    for s in c { self.node(s); }
                }
                let end = self.ip();
                self.patch_jump(skip, end);
            }

            AstNode::FunctionDef { body, name, params, .. } => {
                let mut sub = Compiler::new();
                for p in params { sub.local(&p.name); }
                for s in body { sub.node(s); }
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                let f = FuncValue {
                    name: name.clone(),
                    bytecode: Some(Arc::new(bc)),
                    params: params.iter().map(|p| p.name.clone()).collect(),
                    closure_captures: Vec::new(),
                    closure_env: Vec::new(),
                };
                self.local(name);
                self.functions.push((name.as_str().into(), f));
            }

            AstNode::Call { callee, args, .. } => {
                // Check if callee is a domain method like "io.read"
                if let AstNode::FieldAccess { target, field, .. } = callee.as_ref() {
                    if let AstNode::Ident { name, .. } = target.as_ref() {
                        // If the target is a known local variable, it's a method call on a value.
                        // Otherwise, treat it as a domain call like "io.read".
                        let is_local = self.locals.iter().any(|n| **n == *name);
                        if is_local {
                            self.node(target);
                            for a in args { self.node(&a.value); }
                            let field_idx = self.intern(field);
                            self.emit(Instr::a2(OP_CALL_METHOD, field_idx, args.len() as u16));
                            return;
                        } else {
                            // If the target is a simple identifier, treat it as a domain call
                            let full_name = format!("{}.{}", name, field);
                            let i = self.intern(&full_name);
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                        }
                    } else {
                        self.node(callee);
                    }
                } else if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // If the name was originally a $-prefixed variable, load from local
                    // Otherwise push the name string for global/builtin lookup
                    if let Some(slot) = self.locals.iter().position(|n| **n == *name).map(|i| i as u32) {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    } else {
                        let i = self.intern(name);
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                    }
                } else {
                    self.node(callee);
                }
                for a in args { self.node(&a.value); }
                self.emit(Instr::a2(OP_CALL, args.len() as u32, 0));
            }

            AstNode::Int { value, .. } => {
                if (0..=u32::MAX as i64).contains(value) {
                    self.emit(Instr::a1(OP_PUSH_INT, *value as u32));
                } else {
                    let idx = self.wide(*value);
                    self.emit(Instr::a1(OP_PUSH_INT_WIDE, idx));
                }
            }
            AstNode::Float { value, .. } => {
                let idx = self.wide(value.to_bits() as i64);
                self.emit(Instr::a1(OP_PUSH_FLOAT, idx));
            }
            AstNode::Str { value, .. } => {
                let i = self.intern(value);
                self.emit(Instr::a1(OP_PUSH_STR, i));
            }
            AstNode::Bool { value, .. } => {
                self.emit(Instr::none(if *value { OP_PUSH_TRUE } else { OP_PUSH_FALSE }));
            }
            AstNode::Nil { .. } => self.emit(Instr::none(OP_PUSH_NIL)),

            AstNode::Ident { name, .. } => {
                let slot = self.local(name);
                if !self.try_fuse_load_load(slot) {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                }
            }

            AstNode::BinaryOp { left, op, right, .. } => {
                if matches!(op, BinaryOperator::Add)
                    && self.try_fuse_add_local_local()
                {
                    return;
                }
                self.node(left);
                self.node(right);
                let opcode = match op {
                    BinaryOperator::Add => OP_ADD,
                    BinaryOperator::Sub => OP_SUB,
                    BinaryOperator::Mul => OP_MUL,
                    BinaryOperator::Div => OP_DIV,
                    BinaryOperator::Mod => OP_MOD,
                    BinaryOperator::Eq => OP_EQ,
                    BinaryOperator::NotEq => OP_NEQ,
                    BinaryOperator::Lt => OP_LT,
                    BinaryOperator::Gt => OP_GT,
                    BinaryOperator::Le => OP_LE,
                    BinaryOperator::Ge => OP_GE,
                    BinaryOperator::And => OP_AND,
                    BinaryOperator::Or => OP_OR,
                    BinaryOperator::Coalesce => OP_COALESCE,
                    BinaryOperator::In => OP_IN,
                };
                self.emit(Instr::none(opcode));
            }

            AstNode::UnaryOp { op, operand, .. } => {
                self.node(operand);
                let opcode = match op {
                    UnaryOperator::Neg => OP_NEG,
                    UnaryOperator::Not => OP_NOT,
                };
                self.emit(Instr::none(opcode));
            }

            AstNode::ArrayLit { elements, .. } => {
                for e in elements { self.node(e); }
                self.emit(Instr::a1(OP_MAKE_ARRAY, elements.len() as u32));
            }

            AstNode::DictLit { entries, .. } => {
                for (k, v) in entries {
                    let i = self.intern(k);
                    self.emit(Instr::a1(OP_PUSH_STR, i));
                    self.node(v);
                }
                self.emit(Instr::a1(OP_MAKE_DICT, entries.len() as u32));
            }

            AstNode::Block { statements, .. } => for s in statements { self.node(s); },

            AstNode::Index { target, index, .. } => {
                self.node(target);
                self.node(index);
                self.emit(Instr::none(OP_GET_INDEX));
            }

            AstNode::FieldAccess { target, field, .. } => {
                self.node(target);
                let i = self.intern(field);
                self.emit(Instr::a1(OP_GET_FIELD, i));
            }

            AstNode::Range { start, end, .. } => {
                let i = self.intern("range");
                self.emit(Instr::a1(OP_PUSH_STR, i));
                if let Some(s) = start { self.node(s); } else { self.emit(Instr::none(OP_PUSH_NIL)); }
                if let Some(e) = end { self.node(e); } else { self.emit(Instr::none(OP_PUSH_NIL)); }
                self.emit(Instr::a2(OP_CALL, 2, 0));
            }

            AstNode::Lambda { params, body, .. } => {
                let mut sub = Compiler::new();
                for p in params { sub.local(p); }
                sub.node(body);
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                let f = FuncValue {
                    name: "lambda".into(),
                    bytecode: Some(Arc::new(bc)),
                    params: params.clone(),
                    closure_captures: Vec::new(),
                    closure_env: Vec::new(),
                };
                let idx = self.closures.len() as u32;
                self.closures.push(f);
                self.emit(Instr::a1(OP_LOAD_LAMBDA, idx));
            }

            AstNode::Pipeline { input, stages, .. } => {
                self.node(input);
                for stage in stages {
                    let i = self.intern(&stage.name);
                    self.emit(Instr::a1(OP_PUSH_STR, i));
                    if let Some(a) = &stage.arg { self.node(a); }
                    else { self.emit(Instr::none(OP_PUSH_NIL)); }
                    self.emit(Instr::a2(OP_CALL, 2, 0));
                }
            }

            // Type definition: compile body (fields + methods), register type name as a global
            AstNode::TypeDef { name, kind, body, .. } => {
                // Compile any methods in the body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Register the type name as a global variable (nil for now, VM will resolve it)
                let type_idx = self.intern(name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                let slot = self.local(name);
                self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                // For sum types, compile variant constructors
                if let TypeKind::Sum { variants } = kind {
                    for v in variants {
                        // Each variant becomes a function call: VariantName(args) -> dict with tag
                        let v_idx = self.intern(&v.name);
                        self.emit(Instr::a1(OP_PUSH_STR, v_idx));
                        let v_slot = self.local(&v.name);
                        self.emit(Instr::a1(OP_STORE_LOCAL, v_slot));
                    }
                }
            }

            // Impl block: compile methods, associate with type
            AstNode::ImplDef { type_name, trait_name, body, .. } => {
                // Compile all methods in the impl body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Store the impl relationship as a global marker
                let impl_key = format!("__impl_{}_{}", type_name, trait_name);
                let idx = self.intern(&impl_key);
                self.emit(Instr::a1(OP_PUSH_STR, idx));
                self.emit(Instr::none(OP_PUSH_TRUE));
            }

            // Type test: $x is TypeName
            AstNode::TypeTest { value, type_name, .. } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_TEST));
            }
        }
    }
}

pub fn compile(ast: &[AstNode]) -> Bytecode {
    Compiler::new().compile(ast)
}