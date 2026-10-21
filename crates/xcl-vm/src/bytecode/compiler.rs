//! AST-to-bytecode compiler: translates parsed XCL AST nodes into 64-bit packed instructions.
//!
//! The [`Compiler`] struct accumulates instructions, wide operands, string interning,
//! and local/global slot allocation. After compilation, the peephole optimizer and
//! data flow analysis passes are applied before returning the final [`Bytecode`].

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::string::String;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::collections::{BTreeMap, BTreeSet};
#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "std")]
#[cfg(feature = "std")]
use std::sync::Arc;

use super::Bytecode;
use super::instr::Instr;
use super::opcodes::*;
use super::peephole::peephole_optimize_once;
use crate::HashMap;
use crate::compiler::ast::{
    AstNode, BinaryOperator, CallArg, SpecialVarKind, TypeKind, UnaryOperator,
};
use crate::compiler::span::Span;
use crate::runtime::value::{FuncValue, Value};

/// Temp-slot namespace for pipeline accumulators. Ordinary expression temps use
/// small depths, so this range cannot collide with them.
const PIPE_TEMP_BASE: u32 = 1 << 20;

/// Opcode for a general binary expression.
#[inline(always)]
fn binop_opcode(op: BinaryOperator) -> u8 {
    match op {
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
        BinaryOperator::Shl => OP_SHL,
        BinaryOperator::Shr => OP_SHR,
        BinaryOperator::BitAnd => OP_BITAND,
        BinaryOperator::BitOr => OP_BITOR,
        BinaryOperator::BitXor => OP_BITXOR,
        BinaryOperator::Pow => OP_POW,
    }
}

/// Opcode for `LOAD_LOCAL_LOAD_LOCAL -> ADD/SUB/MUL/DIV_LOCAL_LOCAL` fusion.
#[inline(always)]
fn local_local_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_LOCAL_LOCAL),
        BinaryOperator::Sub => Some(OP_SUB_LOCAL_LOCAL),
        BinaryOperator::Mul => Some(OP_MUL_LOCAL_LOCAL),
        BinaryOperator::Div => Some(OP_DIV_LOCAL_LOCAL),
        _ => None,
    }
}

/// Opcode for `$dest = $dest op $rhs` (local-store form).
#[inline(always)]
fn local_store_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_LOCAL_STORE),
        BinaryOperator::Sub => Some(OP_SUB_LOCAL_STORE),
        BinaryOperator::Mul => Some(OP_MUL_LOCAL_STORE),
        BinaryOperator::Div => Some(OP_DIV_LOCAL_STORE),
        _ => None,
    }
}

/// Opcode for `$dest = $lhs op $rhs` (local-local-store form).
#[inline(always)]
fn local_local_store_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_LOCAL_LOCAL_STORE),
        BinaryOperator::Sub => Some(OP_SUB_LOCAL_LOCAL_STORE),
        BinaryOperator::Mul => Some(OP_MUL_LOCAL_LOCAL_STORE),
        BinaryOperator::Div => Some(OP_DIV_LOCAL_LOCAL_STORE),
        BinaryOperator::Mod => Some(OP_MOD_LOCAL_LOCAL_STORE),
        _ => None,
    }
}

/// Opcode for `$dest = $dest op <integer constant>`.
#[inline(always)]
fn int_store_opcode(op: BinaryOperator) -> Option<u8> {
    match op {
        BinaryOperator::Add => Some(OP_ADD_INT_LOCAL),
        BinaryOperator::Sub => Some(OP_SUB_INT_LOCAL),
        BinaryOperator::Mul => Some(OP_MUL_INT_LOCAL),
        _ => None,
    }
}

/// Body of an inlineable function.
#[derive(Clone)]
enum InlineBody {
    /// Single `Return(expr)` — the original form.
    Expr(AstNode),
    /// Zero or more `Assign` statements followed by exactly one `Return { value: Some(result) }`.
    Block {
        /// `(name, value_expr)` pairs for body-local assignments.
        assigns: Vec<(String, AstNode)>,
        /// The final return expression.
        result: AstNode,
    },
}

/// An inlineable user function: pure body with no side effects.
struct InlineFn {
    /// Formal parameter names.
    params: Vec<String>,
    /// The pure body (single expression or multi-statement block).
    body: InlineBody,
}

struct Compiler {
    code: Vec<Instr>,
    wide: Vec<i64>,
    strings: Vec<Box<str>>,
    locals: Vec<Box<str>>,
    globals: Vec<Box<str>>,
    functions: Vec<(Box<str>, Arc<FuncValue>)>,
    closures: Vec<FuncValue>,
    /// Whether the current function body references special variables ($1, $#, $@, $*).
    uses_special_vars: bool,
    /// Map of function name -> inlineable function info (pre-pass result).
    inline_map: BTreeMap<String, InlineFn>,
    /// Counter for generating fresh inline-local names.
    inline_counter: u64,
    /// Stack of function names currently being inlined (recursion guard).
    inline_stack: Vec<String>,
    /// Nesting depth of pipelines being compiled (temp slots for long
    /// string tables must not collide across nested pipelines).
    pipeline_depth: u32,
    /// Top-level variable names mapped to their root-frame slot. Function and
    /// lambda bodies have their own locals, so a read of one of these names is
    /// compiled to `OP_LOAD_ROOT` instead of creating a fresh local.
    root_slots: HashMap<Box<str>, u32>,
    /// Number of leading local slots (the root variables) whose stores must
    /// survive dead-store elimination because functions read them by slot.
    pinned_slots: u32,
    // Item 4: side maps for O(1) lookup instead of O(n) Vec::position scans.
    string_index: HashMap<Box<str>, u32>,
    local_index: HashMap<Box<str>, u32>,
    global_index: HashMap<Box<str>, u32>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            code: Vec::with_capacity(64),
            wide: Vec::new(),
            strings: Vec::new(),
            locals: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            closures: Vec::new(),
            uses_special_vars: false,
            inline_map: BTreeMap::new(),
            inline_counter: 0,
            inline_stack: Vec::new(),
            pipeline_depth: 0,
            root_slots: HashMap::new(),
            pinned_slots: 0,
            string_index: HashMap::new(),
            local_index: HashMap::new(),
            global_index: HashMap::new(),
        }
    }

    fn with_inline_map(mut self, map: BTreeMap<String, InlineFn>) -> Self {
        self.inline_map = map;
        self
    }

    /// Seeds the compiler with the top-level variable slots. The main compiler
    /// treats them as its own locals (the root frame); bodies read them through
    /// [`OP_LOAD_ROOT`].
    fn with_root_vars(mut self, names: &[String]) -> Self {
        for name in names {
            let i = self.locals.len() as u32;
            self.local_index.insert(name.as_str().into(), i);
            self.locals.push(name.as_str().into());
            self.root_slots.insert(name.as_str().into(), i);
        }
        self.pinned_slots = self.locals.len() as u32;
        self
    }

    /// Creates a compiler for a function or lambda body. The body has its own
    /// locals but can read the top-level frame's variables.
    fn body_compiler(&self) -> Self {
        let mut c = Compiler::new();
        c.root_slots = self.root_slots.clone();
        c
    }

    /// Slot of a top-level variable read from inside a function body.
    ///
    /// Returns `None` when the name is a local of the current body (parameters
    /// and assigned names shadow the root), or when it is not a top-level name.
    #[inline]
    fn root_read_slot(&self, name: &str) -> Option<u32> {
        if self.local_index.contains_key(name) {
            return None;
        }
        self.root_slots.get(name).copied()
    }

    #[inline(always)]
    fn emit(&mut self, i: Instr) {
        self.code.push(i);
    }
    #[inline(always)]
    fn ip(&self) -> u32 {
        self.code.len() as u32
    }

    fn patch_jump(&mut self, at: u32, target: u32) {
        let i = &mut self.code[at as usize];
        *i = Instr::make(i.op(), target, i.b(), i.c());
    }

    #[inline]
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.string_index.get(s) {
            return i;
        }
        let i = self.strings.len() as u32;
        self.string_index.insert(s.into(), i);
        self.strings.push(s.into());
        i
    }

    #[inline]
    fn local(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.local_index.get(name) {
            return i;
        }
        let i = self.locals.len() as u32;
        self.local_index.insert(name.into(), i);
        self.locals.push(name.into());
        i
    }

    #[inline]
    fn global(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.global_index.get(name) {
            return i;
        }
        let i = self.globals.len() as u32;
        self.global_index.insert(name.into(), i);
        self.globals.push(name.into());
        i
    }

    /// Check if a name is a registered global (enum variant, type name, etc.)
    #[inline]
    fn is_global(&self, name: &str) -> bool {
        self.global_index.contains_key(name)
    }

    /// Returns the full dotted name when `node` is a field-access chain rooted
    /// at a plain identifier that is not a local, global, or type name. Such a
    /// chain is a zero-argument domain call (`metadata.all`, `fs.cwd`), not a
    /// field read.
    fn zero_arg_domain_call(&self, node: &AstNode) -> Option<String> {
        let name = collect_dotted_name(node)?;
        if !name.contains('.') {
            return None;
        }
        let base = name.split('.').next().unwrap_or(&name);
        let bare = base.strip_prefix('$').unwrap_or(base);
        let is_value = base.starts_with('$')
            || self.is_global(bare)
            || self.locals.iter().any(|n| n.as_ref() == bare);
        if is_value { None } else { Some(name) }
    }

    #[inline]
    fn wide(&mut self, v: i64) -> u32 {
        let i = self.wide.len() as u32;
        self.wide.push(v);
        i
    }

    #[inline]
    fn temp_slot(&mut self, depth: u32) -> u32 {
        let name = format!("__tmp_{depth}");
        self.local(&name)
    }

    fn can_emit_to_slot(&self, node: &AstNode) -> bool {
        match node {
            AstNode::Ident { name, .. } => {
                !self.is_global(name) && self.root_read_slot(name).is_none()
            }
            AstNode::Int { .. } | AstNode::Float { .. } => true,
            AstNode::UnaryOp {
                op: UnaryOperator::Neg,
                operand,
                ..
            } => self.can_emit_to_slot(operand),
            AstNode::BinaryOp {
                left, op, right, ..
            } if local_local_store_opcode(*op).is_some() => {
                self.can_emit_to_slot(left) && self.can_emit_to_slot(right)
            }
            AstNode::Index { target, index, .. } => {
                // A string-literal index is compiled as an interned field
                // read, so it does not need to be stack-emittable.
                self.can_emit_to_slot(target)
                    && (matches!(index.as_ref(), AstNode::Str { .. })
                        || self.can_emit_to_slot(index))
            }
            AstNode::FieldAccess { target, .. } => {
                self.zero_arg_domain_call(node).is_none() && self.can_emit_to_slot(target)
            }
            _ => false,
        }
    }

    fn emit_expr_to_slot(&mut self, node: &AstNode, temp_depth: u32) -> u32 {
        match node {
            AstNode::Ident { name, .. } => {
                if let Some(slot) = self.root_read_slot(name) {
                    // Top-level variable read from a function body: load it and
                    // park the value in this expression's temp slot.
                    let dst = self.temp_slot(temp_depth);
                    self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                    return dst;
                }
                self.local(name)
            }
            AstNode::Int { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                // Inline small values directly in operand b (kind=0)
                // For larger values, store in wide_operands (kind=6)
                if let Ok(v) = u16::try_from(*value) {
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, v, 0));
                } else {
                    let w = self.wide(*value);
                    // Item 5: fall back to unfused push + store when wide index overflows u16.
                    if let Ok(wu) = u16::try_from(w) {
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 6));
                    } else {
                        self.emit(Instr::a1(OP_PUSH_INT_WIDE, w));
                        self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                    }
                }
                dst
            }
            AstNode::Float { value, .. } => {
                let dst = self.temp_slot(temp_depth);
                let w = self.wide(value.to_bits() as i64);
                // Item 5: fall back to unfused push + store when wide index overflows u16.
                if let Ok(wu) = u16::try_from(w) {
                    self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 1));
                } else {
                    self.emit(Instr::a1(OP_PUSH_FLOAT, w));
                    self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                }
                dst
            }
            AstNode::UnaryOp {
                op: UnaryOperator::Neg,
                operand,
                ..
            } => {
                if let AstNode::Int { value, .. } = operand.as_ref() {
                    let dst = self.temp_slot(temp_depth);
                    let neg = -*value;
                    if u16::try_from(neg).is_ok() {
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, neg as u16, 0));
                    } else {
                        let w = self.wide(neg);
                        // Item 5: fall back to unfused push + store when wide index overflows u16.
                        if let Ok(wu) = u16::try_from(w) {
                            self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 6));
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT_WIDE, w));
                            self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                        }
                    }
                    return dst;
                }
                if let AstNode::Float { value, .. } = operand.as_ref() {
                    let dst = self.temp_slot(temp_depth);
                    let w = self.wide((-*value).to_bits() as i64);
                    // Item 5: fall back to unfused push + store when wide index overflows u16.
                    if let Ok(wu) = u16::try_from(w) {
                        self.emit(Instr::make(OP_LOAD_CONST_LOCAL, dst, wu, 1));
                    } else {
                        self.emit(Instr::a1(OP_PUSH_FLOAT, w));
                        self.emit(Instr::make(OP_STORE_LOCAL, dst, 0, 0));
                    }
                    return dst;
                }
                let zero_slot = self.temp_slot(temp_depth);
                self.emit(Instr::make(OP_LOAD_CONST_LOCAL, zero_slot, 0, 0));
                let src = self.emit_expr_to_slot(operand, temp_depth + 1);
                // Item 5: fall back to unfused sequence when operands overflow.
                if let (Ok(src16), Ok(z8)) = (u16::try_from(src), u8::try_from(zero_slot)) {
                    self.emit(Instr::make(OP_SUB_LOCAL_LOCAL_STORE, zero_slot, src16, z8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, zero_slot));
                    self.emit(Instr::a1(OP_LOAD_LOCAL, src));
                    self.emit(Instr::none(OP_SUB));
                    self.emit(Instr::a1(OP_STORE_LOCAL, zero_slot));
                }
                zero_slot
            }
            AstNode::Index { target, index, .. } => {
                // `$obj["literal"]` is exactly a field read: intern the literal
                // and use the field opcode instead of materializing the key on
                // the operand stack first.
                if let AstNode::Str { value, .. } = index.as_ref() {
                    let t_slot = self.emit_expr_to_slot(target, temp_depth);
                    let field_idx = self.intern(value);
                    let dst = self.temp_slot(temp_depth);
                    // Item 5: fall back to unfused sequence when operands overflow.
                    if let (Ok(fi16), Ok(t8)) = (u16::try_from(field_idx), u8::try_from(t_slot)) {
                        self.emit(Instr::make(OP_GET_FIELD_LOCAL_STORE, dst, fi16, t8));
                    } else {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                        self.emit(Instr::a1(OP_GET_FIELD, field_idx));
                        self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                    }
                    return dst;
                }
                let t_slot = self.emit_expr_to_slot(target, temp_depth);
                let i_slot = self.emit_expr_to_slot(index, temp_depth + 1);
                let dst = self.temp_slot(temp_depth);
                // Item 5: fall back to unfused sequence when operands overflow.
                if let (Ok(is16), Ok(t8)) = (u16::try_from(i_slot), u8::try_from(t_slot)) {
                    self.emit(Instr::make(OP_GET_INDEX_LOCAL_LOCAL_STORE, dst, is16, t8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                    self.emit(Instr::a1(OP_LOAD_LOCAL, i_slot));
                    self.emit(Instr::none(OP_GET_INDEX));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                }
                dst
            }
            AstNode::FieldAccess { target, field, .. } => {
                if let Some(name) = self.zero_arg_domain_call(node) {
                    let i = self.intern(&name);
                    if let Ok(name16) = u16::try_from(i) {
                        self.emit(Instr::make(OP_CALL_NAME, 0, name16, 0));
                    } else {
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        self.emit(Instr::a2(OP_CALL, 0, 0));
                    }
                    let dst = self.temp_slot(temp_depth);
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                    return dst;
                }
                let t_slot = self.emit_expr_to_slot(target, temp_depth);
                let field_idx = self.intern(field);
                let dst = self.temp_slot(temp_depth);
                // Item 5: fall back to unfused sequence when operands overflow.
                if let (Ok(fi16), Ok(t8)) = (u16::try_from(field_idx), u8::try_from(t_slot)) {
                    self.emit(Instr::make(OP_GET_FIELD_LOCAL_STORE, dst, fi16, t8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, t_slot));
                    self.emit(Instr::a1(OP_GET_FIELD, field_idx));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                }
                dst
            }
            AstNode::BinaryOp {
                left, op, right, ..
            } => {
                let l_slot = self.emit_expr_to_slot(left, temp_depth);
                let r_slot = self.emit_expr_to_slot(right, temp_depth + 1);
                let dst = self.temp_slot(temp_depth);
                let Some(opc) = local_local_store_opcode(*op) else {
                    unreachable!()
                };
                // Item 5: fall back to unfused sequence when operands overflow.
                if let (Ok(rs16), Ok(ls8)) = (u16::try_from(r_slot), u8::try_from(l_slot)) {
                    self.emit(Instr::make(opc, dst, rs16, ls8));
                } else {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, l_slot));
                    self.emit(Instr::a1(OP_LOAD_LOCAL, r_slot));
                    self.emit(Instr::none(binop_opcode(*op)));
                    self.emit(Instr::a1(OP_STORE_LOCAL, dst));
                }
                dst
            }
            _ => unreachable!(),
        }
    }

    /// Emits `local[a] ± int` as a single push when a call argument has that
    /// shape. Returns `true` if the fused opcode was emitted.
    /// Emits call arguments, fusing `local ± int` into a single push.
    /// Emits call arguments and returns the argument count the callee receives.
    ///
    /// Positional arguments go first, in source order. Named arguments are
    /// packed into one trailing map (`name: value` pairs), which natives split
    /// with `split_named_args`. The map counts as one argument.
    fn emit_call_args(&mut self, args: &[CallArg]) -> usize {
        let mut positional = 0;
        for a in args {
            if a.name.is_none() {
                positional += 1;
                if !self.try_emit_arg_fused(&a.value) {
                    self.node(&a.value);
                }
            }
        }
        let named = args.iter().filter(|a| a.name.is_some()).count();
        if named == 0 {
            return positional;
        }
        for a in args {
            if let Some(name) = &a.name {
                let key = self.intern(name);
                self.emit(Instr::a1(OP_PUSH_STR, key));
                self.node(&a.value);
            }
        }
        self.emit(Instr::a1(OP_MAKE_DICT, named as u32));
        positional + 1
    }

    fn try_emit_arg_fused(&mut self, node: &AstNode) -> bool {
        let AstNode::BinaryOp {
            left, op, right, ..
        } = node
        else {
            return false;
        };
        let AstNode::Ident { name, .. } = left.as_ref() else {
            return false;
        };
        let AstNode::Int { value, .. } = right.as_ref() else {
            return false;
        };
        if self.is_global(name) || !(0..=u16::MAX as i64).contains(value) {
            return false;
        }
        let Some(slot) = self.locals.iter().position(|n| &**n == name) else {
            return false;
        };
        let kind = match op {
            BinaryOperator::Add => 0u8,
            BinaryOperator::Sub => 1u8,
            BinaryOperator::Mul => 2u8,
            _ => return false,
        };
        self.emit(Instr::make(
            OP_PUSH_LOCAL_INT_OP,
            slot as u32,
            *value as u16,
            kind,
        ));
        true
    }

    /// Try to fuse a nested binary expression into superinstructions.
    /// For example, `$zr = $zr*$zr - $zi*$zi + $cr` becomes:
    ///   MUL_LL_ST(temp1, zr, zr)
    ///   MUL_LL_ST(temp2, zi, zi)
    ///   SUB_LL_ST(temp1, temp1, temp2)
    ///   ADD_LL_ST(zr, temp1, cr)
    fn try_emit_binary_fused(&mut self, dest_slot: u32, node: &AstNode) -> bool {
        let AstNode::BinaryOp {
            left, op, right, ..
        } = node
        else {
            return false;
        };

        // Case 1: $dest = $dest op $b (simple self-assignment)
        if let AstNode::Ident { name, .. } = left.as_ref()
            && !self.is_global(name)
            && self.local(name) == dest_slot
        {
            if let AstNode::Ident { name: rn, .. } = right.as_ref()
                && !self.is_global(rn)
            {
                let rslot = self.local(rn);
                if let Some(opc) = local_store_opcode(*op) {
                    // Item 5: only emit fused opcode when rslot fits in u16.
                    if let Ok(r16) = u16::try_from(rslot) {
                        self.emit(Instr::a2(opc, dest_slot, r16));
                        return true;
                    }
                    // Fall through to unfused path.
                }
            }
            if let AstNode::Int { value, .. } = right.as_ref() {
                if *value == 1 && matches!(op, BinaryOperator::Add) {
                    self.emit(Instr::a1(OP_INC_LOCAL, dest_slot));
                    return true;
                }
                if *value == 1 && matches!(op, BinaryOperator::Sub) {
                    self.emit(Instr::a1(OP_DEC_LOCAL, dest_slot));
                    return true;
                }
                if (0..=u16::MAX as i64).contains(value) {
                    if let Some(opc) = int_store_opcode(*op) {
                        self.emit(Instr::a2(opc, dest_slot, *value as u16));
                        return true;
                    }
                }
            }
        }

        // Case 2: $dest = $a op $b (both distinct locals)
        if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. }) =
            (left.as_ref(), right.as_ref())
            && !self.is_global(ln)
            && !self.is_global(rn)
        {
            let lslot = self.local(ln);
            let rslot = self.local(rn);
            // Item 5: only emit fused opcode when both operands fit.
            if let (Ok(l8), Ok(r16)) = (u8::try_from(lslot), u16::try_from(rslot)) {
                if let Some(opc) = local_local_store_opcode(*op) {
                    self.emit(Instr::make(opc, dest_slot, r16, l8));
                    return true;
                }
            }
        }

        // Case 3: Nested 3-address register compilation
        if local_local_store_opcode(*op).is_some()
            && self.locals.len() < 200
            && self.can_emit_to_slot(node)
        {
            if let AstNode::Ident { name: ln, .. } = left.as_ref()
                && !self.is_global(ln)
                && self.local(ln) == dest_slot
                && local_store_opcode(*op).is_some()
            {
                // $dest = $dest + $obj.field  (or $obj["field"]) in one opcode.
                if matches!(op, BinaryOperator::Add) {
                    let field = match right.as_ref() {
                        AstNode::FieldAccess { field, .. } => Some(field),
                        AstNode::Index { index, .. } => match index.as_ref() {
                            AstNode::Str { value, .. } => Some(value),
                            _ => None,
                        },
                        _ => None,
                    };
                    let obj_name = match right.as_ref() {
                        AstNode::FieldAccess { target, .. } => match target.as_ref() {
                            AstNode::Ident { name, .. } => Some(name),
                            _ => None,
                        },
                        AstNode::Index { target, .. } => match target.as_ref() {
                            AstNode::Ident { name, .. } => Some(name),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let (Some(field), Some(obj_name)) = (field, obj_name)
                        && !self.is_global(obj_name)
                    {
                        let obj_slot = self.local(obj_name);
                        // Item 5: only emit fused opcode when operands fit.
                        if let (Ok(o8), Ok(fi16)) =
                            (u8::try_from(obj_slot), u16::try_from(self.intern(field)))
                        {
                            self.emit(Instr::make(OP_ADD_FIELD_LOCAL_STORE, dest_slot, fi16, o8));
                            return true;
                        }
                    }
                }

                let r_slot = self.emit_expr_to_slot(right, 0);
                let Some(opc) = local_store_opcode(*op) else {
                    unreachable!()
                };
                // Item 5: only emit fused opcode when r_slot fits in u16.
                if let Ok(r16) = u16::try_from(r_slot) {
                    self.emit(Instr::a2(opc, dest_slot, r16));
                    return true;
                }
                // Fall through to unfused path below.
            }

            let l_slot = self.emit_expr_to_slot(left, 0);
            let r_slot = self.emit_expr_to_slot(right, 1);
            // Item 5: only emit fused opcode when both operands fit.
            if let (Ok(l8), Ok(r16)) = (u8::try_from(l_slot), u16::try_from(r_slot)) {
                let Some(opc) = local_local_store_opcode(*op) else {
                    unreachable!()
                };
                self.emit(Instr::make(opc, dest_slot, r16, l8));
                return true;
            }
        }

        false
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

    /// Check if the last instruction is LL_LL and upgrade it, or two LOAD_LOCALs
    fn try_fuse_local_local(&mut self, op: u8) -> bool {
        // If the last instruction is already LL_LL, upgrade it to the arithmetic opcode
        if let Some(prev) = self.code.last().copied() {
            if prev.op() == OP_LOAD_LOCAL_LOAD_LOCAL {
                self.code.pop();
                self.emit(Instr::make(op, prev.a(), prev.b(), 0));
                return true;
            }
        }
        // Fallback: two consecutive LOAD_LOCAL instructions
        if self.code.len() < 2 {
            return false;
        }
        let l2 = self.code[self.code.len() - 1].op();
        let l1 = self.code[self.code.len() - 2].op();
        if l1 == OP_LOAD_LOCAL && l2 == OP_LOAD_LOCAL {
            let a = self.code[self.code.len() - 2].a();
            let b_slot = self.code[self.code.len() - 1].a();
            self.code.truncate(self.code.len() - 2);
            self.emit(Instr::make(op, a, b_slot as u16, 0));
            return true;
        }
        false
    }

    fn compile(&mut self, ast: &[AstNode]) -> Bytecode {
        self.compile_with(ast, true, &mut Vec::new())
    }

    /// Compiles `ast`, optionally without the optimizing passes.
    ///
    /// `ranges` receives `(first_ip, line)` for every top-level statement that
    /// emitted at least one instruction. The offsets are only meaningful when
    /// `optimize` is `false`: both optimizing passes rewrite the instruction
    /// stream, which would invalidate them.
    fn compile_with(
        &mut self,
        ast: &[AstNode],
        optimize: bool,
        ranges: &mut Vec<(u32, u32)>,
    ) -> Bytecode {
        for n in ast {
            let start = self.ip();
            self.node(n);
            let line = node_line(n);
            if line != 0 && self.ip() > start {
                ranges.push((start, line));
            }
        }
        self.emit(Instr::none(OP_HALT));

        let mut code: Vec<Instr> = core::mem::take(&mut self.code);
        if optimize {
            // Run peephole optimizer to fuse common patterns
            peephole_optimize_once(&mut code, &mut self.wide);

            // Run data flow analysis to eliminate dead stores
            crate::optimizer::optimize(&mut code, &mut self.wide, self.pinned_slots);
        }

        let strings = core::mem::take(&mut self.strings);
        let string_values: Vec<Value> = strings
            .iter()
            .map(|s| Value::from_str_boxed(s.to_string()))
            .collect();

        Bytecode {
            instructions: code.into(),
            wide_operands: core::mem::take(&mut self.wide).into(),
            string_table: strings.into(),
            string_values: string_values.into(),
            local_names: core::mem::take(&mut self.locals).into(),
            global_names: core::mem::take(&mut self.globals).into(),
            user_functions: core::mem::take(&mut self.functions).into(),
            closures: core::mem::take(&mut self.closures).into(),
        }
    }

    fn node(&mut self, node: &AstNode) {
        match node {
            AstNode::Assign { name, value, .. } => {
                let slot = self.local(name);
                // Fast in-place array push: $arr = $arr + [elem]
                if let AstNode::BinaryOp {
                    left,
                    op: BinaryOperator::Add,
                    right,
                    ..
                } = value.as_ref()
                    && let AstNode::Ident { name: ln, .. } = left.as_ref()
                    && ln == name
                    && let AstNode::ArrayLit { elements, .. } = right.as_ref()
                    && elements.len() == 1
                {
                    if self.locals.len() < 200 && self.can_emit_to_slot(&elements[0]) {
                        let elem_slot = self.emit_expr_to_slot(&elements[0], 0);
                        self.emit(Instr::a2(OP_ARRAY_PUSH_LOCAL, slot, elem_slot as u16));
                        return;
                    }
                    self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    self.node(&elements[0]);
                    self.emit(Instr::none(OP_ARRAY_PUSH));
                    self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                    return;
                }
                // Try recursive RHS fusion for nested binary expressions
                // e.g., $zr = $zr*$zr - $zi*$zi + $cr
                if let AstNode::BinaryOp { .. } = value.as_ref() {
                    if self.try_emit_binary_fused(slot, value.as_ref()) {
                        return;
                    }
                }
                // Direct RHS fusion for array index and field access
                if self.locals.len() < 200 && self.can_emit_to_slot(value.as_ref()) {
                    match value.as_ref() {
                        AstNode::Index { target, index, .. } => {
                            let t_slot = self.emit_expr_to_slot(target, 0);
                            // `$obj["literal"]` is a field read with an
                            // interned key: no need to materialize the key.
                            if let AstNode::Str { value, .. } = index.as_ref() {
                                let field_idx = self.intern(value);
                                // Item 5: only emit fused opcode when operands fit.
                                if let (Ok(fi16), Ok(t8)) =
                                    (u16::try_from(field_idx), u8::try_from(t_slot))
                                {
                                    self.emit(Instr::make(
                                        OP_GET_FIELD_LOCAL_STORE,
                                        slot,
                                        fi16,
                                        t8,
                                    ));
                                    return;
                                }
                            }
                            let i_slot = self.emit_expr_to_slot(index, 1);
                            // Item 5: only emit fused opcode when operands fit.
                            if let (Ok(is16), Ok(t8)) =
                                (u16::try_from(i_slot), u8::try_from(t_slot))
                            {
                                self.emit(Instr::make(
                                    OP_GET_INDEX_LOCAL_LOCAL_STORE,
                                    slot,
                                    is16,
                                    t8,
                                ));
                                return;
                            }
                        }
                        AstNode::FieldAccess { target, field, .. } => {
                            let t_slot = self.emit_expr_to_slot(target, 0);
                            let field_idx = self.intern(field);
                            // Item 5: only emit fused opcode when operands fit.
                            if let (Ok(fi16), Ok(t8)) =
                                (u16::try_from(field_idx), u8::try_from(t_slot))
                            {
                                self.emit(Instr::make(OP_GET_FIELD_LOCAL_STORE, slot, fi16, t8));
                                return;
                            }
                        }
                        _ => {}
                    }
                }
                // Try to fuse hot patterns: $x = $x + 1, $x = $x - 1, $x = $x + c, $a = $a + $b
                if let AstNode::BinaryOp {
                    left, op, right, ..
                } = value.as_ref()
                {
                    if let AstNode::Ident {
                        name: left_name, ..
                    } = left.as_ref()
                    {
                        if left_name == name {
                            // $x = $x op something
                            match op {
                                BinaryOperator::Add => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x + 1 â†’ OP_INC_LOCAL
                                        self.emit(Instr::a1(OP_INC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x + c â†’ OP_INC_LOCAL_INT
                                        self.emit(Instr::make(
                                            OP_INC_LOCAL_INT,
                                            slot,
                                            *c as u16,
                                            0,
                                        ));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x + $y â†’ OP_ADD_LOCAL_STORE (direct local-to-local)
                                        let right_slot = self.local(right_name);
                                        // Item 5: only emit fused opcode when slot fits.
                                        if let Ok(rs16) = u16::try_from(right_slot) {
                                            self.emit(Instr::a2(OP_ADD_LOCAL_STORE, slot, rs16));
                                            return;
                                        }
                                    }
                                }
                                BinaryOperator::Sub => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x - 1 â†’ OP_DEC_LOCAL
                                        self.emit(Instr::a1(OP_DEC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x - $y â†’ OP_SUB_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        // Item 5: only emit fused opcode when slot fits.
                                        if let Ok(rs16) = u16::try_from(right_slot) {
                                            self.emit(Instr::a2(OP_SUB_LOCAL_STORE, slot, rs16));
                                            return;
                                        }
                                    }
                                }
                                BinaryOperator::Mul => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x * c â†’ OP_MUL_INT_LOCAL
                                        // Item 5: only emit when constant fits in u16.
                                        if let Ok(c16) = u16::try_from(*c) {
                                            self.emit(Instr::a2(OP_MUL_INT_LOCAL, slot, c16));
                                            return;
                                        }
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x * $y â†’ OP_MUL_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        // Item 5: only emit fused opcode when slot fits.
                                        if let Ok(rs16) = u16::try_from(right_slot) {
                                            self.emit(Instr::a2(OP_MUL_LOCAL_STORE, slot, rs16));
                                            return;
                                        }
                                    }
                                }
                                BinaryOperator::Div => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x / c â†’ handled by general path for now
                                        let _ = c;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // NEW: $z = $a op $b (both operands are distinct locals)
                    if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. }) =
                        (left.as_ref(), right.as_ref())
                    {
                        let lslot = self.local(ln);
                        let rslot = self.local(rn);
                        // Item 5: only emit fused opcode when both operands fit.
                        if let (Ok(l8), Ok(r16)) = (u8::try_from(lslot), u16::try_from(rslot)) {
                            if let Some(opc) = local_local_store_opcode(*op) {
                                self.emit(Instr::make(opc, slot, r16, l8));
                                return;
                            }
                        }
                    }
                }
                self.node(value);
                self.emit(Instr::a1(OP_STORE_LOCAL, slot));
            }
            AstNode::Return { value, .. } => {
                // Tail-call optimization: `return foo(args)`.
                if let Some(AstNode::Call { callee, args, .. }) = value.as_deref() {
                    if let AstNode::Ident { name, .. } = callee.as_ref() {
                        // A `$`-variable callee is a value, not a global name:
                        // load it and use the value tail call.
                        if let Some(slot) =
                            self.locals.iter().position(|n| n.as_ref() == name.as_str())
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot as u32));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_TAIL_CALL, n as u32, 0));
                            return;
                        }
                        // Emit OP_TAIL_CALL_NAME: fused tail call with embedded name
                        let i = self.intern(name);
                        if let Ok(name16) = u16::try_from(i) {
                            let n = self.emit_call_args(args);
                            self.emit(Instr::make(OP_TAIL_CALL_NAME, n as u32, name16, 0));
                        } else {
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_TAIL_CALL, n as u32, 0));
                        }
                        return;
                    }
                }
                if let Some(v) = value {
                    self.node(v);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                self.emit(Instr::none(OP_RETURN));
            }
            AstNode::Break { .. } | AstNode::Continue { .. } => {}

            AstNode::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                // `if $x < N` / `if $x == N` fuse the compare and the branch
                // into one dispatch. The branch target goes in the wide table.
                let fused = match condition.as_ref() {
                    AstNode::BinaryOp {
                        left, op, right, ..
                    } => match (left.as_ref(), right.as_ref()) {
                        (AstNode::Ident { name, .. }, AstNode::Int { value, .. })
                            if !self.is_global(name)
                                && *value >= 0
                                && *value <= u16::MAX as i64 =>
                        {
                            let slot = self
                                .locals
                                .iter()
                                .position(|n| &**n == name)
                                .map(|i| i as u32);
                            slot.and_then(|slot| match op {
                                BinaryOperator::Lt => Some((OP_IF_LT_INT, slot, *value as u16)),
                                BinaryOperator::Eq => Some((OP_IF_EQ_INT, slot, *value as u16)),
                                _ => None,
                            })
                        }
                        _ => None,
                    },
                    _ => None,
                };

                if let Some((opc, slot, limit)) = fused
                    && self.wide.len() < 256
                {
                    let widx = self.wide(0) as u8;
                    self.emit(Instr::make(opc, slot, limit, widx));
                    for s in then_branch {
                        self.node(s);
                    }
                    if let Some(e) = else_branch {
                        let j2 = self.ip();
                        self.emit(Instr::a1(OP_JUMP, 0));
                        let t = self.ip();
                        self.wide[widx as usize] = t as i64;
                        for s in e {
                            self.node(s);
                        }
                        let end = self.ip();
                        self.patch_jump(j2, end);
                    } else {
                        let end = self.ip();
                        self.wide[widx as usize] = end as i64;
                    }
                    return;
                }

                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in then_branch {
                    self.node(s);
                }
                if let Some(e) = else_branch {
                    let j2 = self.ip();
                    self.emit(Instr::a1(OP_JUMP, 0));
                    let t = self.ip();
                    self.patch_jump(j, t);
                    for s in e {
                        self.node(s);
                    }
                    let end = self.ip();
                    self.patch_jump(j2, end);
                } else {
                    let end = self.ip();
                    self.patch_jump(j, end);
                }
            }

            AstNode::For {
                variable,
                iterable,
                body,
                ..
            } => {
                if let AstNode::Range { start, end, .. } = iterable.as_ref() {
                    // Check if start and end are integer constants â€” if so, use OP_FOR_NUMBER_LOOP
                    let start_val = match start.as_ref().map(|n| n.as_ref()) {
                        Some(AstNode::Int { value, .. }) => Some(*value),
                        Some(_) => None,
                        None => Some(0),
                    };
                    let end_val = match end.as_ref().map(|n| n.as_ref()) {
                        Some(AstNode::Int { value, .. }) => Some(*value),
                        Some(_) => None,
                        None => Some(0),
                    };

                    if let (Some(start_i), Some(end_i)) = (start_val, end_val) {
                        // Use single-opcode FOR_NUMBER_LOOP (like Luau's FORNLOOP)
                        // Runtime expects: a = (var_slot << 16) | start_wide_idx, b = end_wide_idx
                        let var = self.local(variable);
                        let start_minus_1 = self.wide(start_i.wrapping_sub(1));
                        let end_wide = self.wide(end_i);
                        let exit_wide = self.wide(0);
                        // Check that var_slot and start_wide_idx fit in packed operand a
                        if var <= 0xFFFF && start_minus_1 <= 0xFFFF {
                            self.emit(Instr::a1(OP_PUSH_INT_WIDE, start_minus_1));
                            self.emit(Instr::a1(OP_STORE_LOCAL, var));
                            let loop_start = self.ip();
                            let packed_a = (var << 16) | start_minus_1;
                            self.emit(Instr::make(
                                OP_FOR_NUMBER_LOOP,
                                packed_a,
                                end_wide as u16,
                                exit_wide as u8,
                            ));
                            for s in body {
                                self.node(s);
                            }
                            // Jump back to FOR_NUMBER_LOOP
                            self.emit(Instr::a1(OP_JUMP, loop_start));
                            let exit_ip = self.ip();
                            self.wide[exit_wide as usize] = exit_ip as i64;
                        } else {
                            // Overflow: fall through to FOR_RANGE_START/NEXT path below
                        }
                    } else {
                        // Fallback: use FOR_RANGE_START/NEXT for non-constant ranges
                        if let Some(s) = start {
                            self.node(s);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT, 0));
                        }
                        if let Some(e) = end {
                            self.node(e);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT, 0));
                        }
                        self.emit(Instr::none(OP_FOR_RANGE_START));
                        let start_ip = self.ip();
                        let next = self.ip();
                        self.emit(Instr::a1(OP_FOR_RANGE_NEXT, 0));
                        let var = self.local(variable);
                        self.emit(Instr::a1(OP_STORE_LOCAL, var));
                        for s in body {
                            self.node(s);
                        }
                        self.emit(Instr::a1(OP_JUMP, start_ip));
                        let end = self.ip();
                        self.patch_jump(next, end);
                    }
                } else {
                    self.node(iterable);
                    self.emit(Instr::none(OP_FOR_ITER_START));
                    let start_ip = self.ip();
                    let next = self.ip();
                    self.emit(Instr::a1(OP_FOR_ITER_NEXT, 0));
                    let var = self.local(variable);
                    self.emit(Instr::a1(OP_STORE_LOCAL, var));
                    for s in body {
                        self.node(s);
                    }
                    self.emit(Instr::a1(OP_JUMP, start_ip));
                    let end = self.ip();
                    self.patch_jump(next, end);
                }
            }

            AstNode::While {
                condition, body, ..
            } => {
                // Try to fuse: while $i < N { ... } â†’ OP_WHILE_LT_INT
                // Or: while $a < $b { ... } â†’ OP_WHILE_LT_LOCAL
                match condition.as_ref() {
                    AstNode::BinaryOp {
                        left,
                        op: BinaryOperator::Lt,
                        right,
                        ..
                    } => {
                        if let AstNode::Ident {
                            name: left_name, ..
                        } = left.as_ref()
                        {
                            if let AstNode::Int { value: limit, .. } = right.as_ref()
                                && (0..=i64::from(u16::MAX)).contains(limit)
                            {
                                let slot = self.local(left_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(
                                    OP_WHILE_LT_INT,
                                    slot,
                                    *limit as u16,
                                    exit_wide as u8,
                                ));
                                for s in body {
                                    self.node(s);
                                }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                return;
                            } else if let AstNode::Int { value: limit, .. } = right.as_ref() {
                                // Limits above `u16::MAX` still fuse: the limit
                                // moves into `wide_operands` (operand `b`).
                                let slot = self.local(left_name);
                                let limit_wide = self.wide(*limit);
                                let exit_wide = self.wide(0);
                                if let (Ok(limit_idx), Ok(exit_idx)) =
                                    (u16::try_from(limit_wide), u8::try_from(exit_wide))
                                {
                                    let loop_start = self.ip();
                                    self.emit(Instr::make(
                                        OP_WHILE_LT_INT_WIDE,
                                        slot,
                                        limit_idx,
                                        exit_idx,
                                    ));
                                    for s in body {
                                        self.node(s);
                                    }
                                    self.emit(Instr::a1(OP_JUMP, loop_start));
                                    let exit_ip = self.ip();
                                    self.wide[exit_wide as usize] = exit_ip as i64;
                                    return;
                                }
                            } else if let AstNode::Ident {
                                name: right_name, ..
                            } = right.as_ref()
                            {
                                let left_slot = self.local(left_name);
                                let right_slot = self.local(right_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(
                                    OP_WHILE_LT_LOCAL,
                                    left_slot,
                                    right_slot as u16,
                                    exit_wide as u8,
                                ));
                                for s in body {
                                    self.node(s);
                                }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                return;
                            }
                        }
                    }
                    _ => {}
                };

                // Fallback: generic while loop
                let start = self.ip();
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in body {
                    self.node(s);
                }
                self.emit(Instr::a1(OP_JUMP, start));
                let end = self.ip();
                self.patch_jump(j, end);
            }

            AstNode::FunctionDef {
                body, name, params, ..
            } => {
                let mut sub = self
                    .body_compiler()
                    .with_inline_map(core::mem::take(&mut self.inline_map));
                for p in params {
                    sub.local(&p.name);
                }
                // Names assigned anywhere in the body are locals for the whole
                // body, so they shadow same-named top-level variables.
                let mut reads = Vec::new();
                let mut writes = Vec::new();
                for s in body {
                    scan_vars(s, &mut reads, &mut writes);
                }
                for w in &writes {
                    sub.local(w);
                }
                for s in body {
                    sub.node(s);
                }
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                // Restore inline_map for sibling nodes
                self.inline_map = core::mem::take(&mut sub.inline_map);
                let f = FuncValue {
                    name: name.clone(),
                    bytecode: Some(Arc::new(bc)),
                    params: params.iter().map(|p| p.name.clone()).collect(),
                    closure_captures: Vec::new(),
                    closure_env: Vec::new(),
                    uses_special_vars: sub.uses_special_vars,
                };
                self.functions.push((name.as_str().into(), Arc::new(f)));
            }

            AstNode::Call { callee, args, .. } => {
                // A bare `$f` statement: call the value when it holds a function.
                // (`$x` holding anything else is left on the stack as before.)
                if args.is_empty() {
                    if let AstNode::Ident { name, .. } = callee.as_ref() {
                        let local = self
                            .locals
                            .iter()
                            .position(|n| n.as_ref() == name.as_str())
                            .map(|i| i as u32);
                        if let Some(slot) = local {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                            self.emit(Instr::none(OP_CALL_VALUE_ZERO));
                            return;
                        }
                        if let Some(slot) = self.root_read_slot(name) {
                            self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                            self.emit(Instr::none(OP_CALL_VALUE_ZERO));
                            return;
                        }
                    }
                }
                // Try to inline: if callee is a plain identifier matching an inlineable function
                if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // A `$`-variable with the same name shadows the global function,
                    // so the call must stay dynamic.
                    let shadowed = self.locals.iter().any(|n| n.as_ref() == name.as_str());
                    // Clone the inline body out to avoid borrow conflicts with self
                    let inline_body = if shadowed {
                        None
                    } else {
                        self.inline_map
                            .get(name)
                            .map(|f| (f.params.clone(), f.body.clone()))
                    };

                    if let Some((params, body)) = &inline_body {
                        // Criterion: arg count must match
                        if args.len() == params.len()
                            // Named arguments are not positional; inlining would
                            // substitute them into the wrong parameters.
                            && args.iter().all(|a| a.name.is_none())
                            // Criterion 4: every arg must be simple
                            && args.iter().all(|a| is_simple_arg(&a.value))
                            // Recursion guard: do not inline if already on the stack
                            && !self.inline_stack.contains(name)
                            // Depth limit
                            && self.inline_stack.len() < 8
                        {
                            // Build substitution map: param -> arg expression
                            let subs: Vec<(String, AstNode)> = params
                                .iter()
                                .cloned()
                                .zip(args.iter().map(|a| a.value.clone()))
                                .collect();

                            match body {
                                InlineBody::Expr(body_expr) => {
                                    // Original single-expression inlining
                                    let body = substitute(body_expr, &subs);
                                    self.node(&body);
                                    return;
                                }
                                InlineBody::Block { assigns, result } => {
                                    // Multi-statement block inlining:
                                    // 1. Create fresh names for body-locals
                                    // 2. Substitute params and body-locals
                                    // 3. Compile each assignment
                                    // 4. Compile the result expression

                                    // Build fresh name mapping for body-locals
                                    let mut fresh_subs = subs.clone();
                                    let mut local_map: Vec<(String, String)> = Vec::new();
                                    for (local_name, _) in assigns {
                                        let fresh = format!(
                                            "__xcl_inl_{}_{}",
                                            self.inline_counter, local_name
                                        );
                                        self.inline_counter += 1;
                                        let old_name = local_name.clone();
                                        local_map.push((old_name.clone(), fresh.clone()));
                                        fresh_subs.push((
                                            old_name,
                                            AstNode::Ident {
                                                name: fresh.clone(),
                                                span: Span::empty(),
                                            },
                                        ));
                                    }

                                    // Push this function onto the inline stack
                                    self.inline_stack.push(name.clone());

                                    // Compile each assignment with substituted values
                                    for (local_name, value_expr) in assigns {
                                        let substituted_value = substitute(value_expr, &fresh_subs);
                                        // Find the fresh name for this local
                                        let fresh_name = local_map
                                            .iter()
                                            .find(|(old, _)| *old == *local_name)
                                            .map(|(_, fresh)| fresh.as_str())
                                            .unwrap_or(local_name.as_str());
                                        let slot = self.local(fresh_name);
                                        // Compile: $fresh = substituted_value
                                        self.node(&substituted_value);
                                        self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                                    }

                                    // Compile the result expression
                                    let substituted_result = substitute(result, &fresh_subs);
                                    self.node(&substituted_result);

                                    // Pop the inline stack
                                    self.inline_stack.pop();

                                    return;
                                }
                            }
                        }
                    }
                }

                // Build full dotted name from nested FieldAccess chains.
                // e.g., net.tcp.connect -> "net.tcp.connect"
                //       io.read -> "io.read"
                //       $r.map_err -> "$r.map_err"
                let full_name = collect_dotted_name(callee.as_ref());
                if let Some(name) = full_name {
                    // Check if the base identifier is a local variable AND there's a dot (method call)
                    let parts: Vec<&str> = name.split('.').collect();
                    let base = parts.first().copied().unwrap_or(&name);
                    // Strip $ prefix from base for local variable lookup
                    let base_unprefixed = base.strip_prefix('$').unwrap_or(base);
                    let is_local = self.locals.iter().any(|n| n.as_ref() == base_unprefixed);
                    let root_slot = if is_local {
                        None
                    } else {
                        self.root_read_slot(base_unprefixed)
                    };
                    if is_local && parts.len() > 1 {
                        // It's a method call on a local value: $x.method()
                        // Compile as OP_CALL_METHOD: load receiver, push args, CALL_METHOD
                        let method_name = parts.last().copied().unwrap_or(&name);
                        let field_idx = self.intern(method_name);
                        // Load the receiver (the local variable)
                        if let Some(slot) = self
                            .locals
                            .iter()
                            .position(|n| n.as_ref() == base_unprefixed)
                            .map(|i| i as u32)
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        // Push args
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL_METHOD, field_idx, n as u16));
                    } else if let Some(slot) = root_slot.filter(|_| parts.len() > 1) {
                        // Method call on a top-level variable read from a
                        // function body: `$r.is_ok`.
                        let method_name = parts.last().copied().unwrap_or(&name);
                        let field_idx = self.intern(method_name);
                        self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL_METHOD, field_idx, n as u16));
                    } else if is_local && parts.len() == 1 {
                        // Single-part local variable call: $f() or $f arg
                        if let Some(slot) = self
                            .locals
                            .iter()
                            .position(|n| **n == *base)
                            .map(|i| i as u32)
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL, n as u32, 0));
                    } else {
                        // Bare or dotted name call. Embed the callee name in the
                        // opcode so the name push and the call are one dispatch.
                        // The handler resolves builtins, user functions, and
                        // domain methods exactly like `OP_CALL`.
                        let i = self.intern(&name);
                        if let Ok(name16) = u16::try_from(i) {
                            let n = self.emit_call_args(args);
                            self.emit(Instr::make(OP_CALL_NAME, n as u32, name16, 0));
                        } else {
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_CALL, n as u32, 0));
                        }
                    }
                } else if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // If the name was originally a $-prefixed variable, load from local
                    // Otherwise push the name string for global/builtin lookup
                    if let Some(slot) = self
                        .locals
                        .iter()
                        .position(|n| **n == *name)
                        .map(|i| i as u32)
                    {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL, n as u32, 0));
                    } else {
                        // Fused named call: embed the callee name in the opcode
                        let i = self.intern(name);
                        if let Ok(name16) = u16::try_from(i) {
                            let n = self.emit_call_args(args);
                            self.emit(Instr::make(OP_CALL_NAME, n as u32, name16, 0));
                        } else {
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_CALL, n as u32, 0));
                        }
                    }
                } else {
                    self.node(callee);
                    let n = self.emit_call_args(args);
                    self.emit(Instr::a2(OP_CALL, n as u32, 0));
                }
            }

            AstNode::DomainCall {
                callee,
                args,
                items,
                span,
            } => {
                if items.is_empty() {
                    self.emit(Instr::none(OP_PUSH_NIL));
                    return;
                }
                for (i, item) in items.iter().enumerate() {
                    // Positional args first, then the item value, then the named
                    // map. Item named args replace shared ones with the same key.
                    let mut call_args: Vec<CallArg> =
                        args.iter().filter(|a| a.name.is_none()).cloned().collect();
                    call_args.push(CallArg {
                        value: item.value.clone(),
                        name: None,
                    });
                    for a in args.iter().filter(|a| a.name.is_some()) {
                        let key = a.name.as_ref().expect("named arg");
                        if !item.named.iter().any(|(k, _)| k == key) {
                            call_args.push(a.clone());
                        }
                    }
                    for (key, value) in &item.named {
                        call_args.push(CallArg {
                            value: value.clone(),
                            name: Some(key.clone()),
                        });
                    }
                    let call = AstNode::Call {
                        span: span.clone(),
                        callee: callee.clone(),
                        args: call_args,
                    };
                    self.node(&call);
                    if i + 1 != items.len() {
                        self.emit(Instr::none(OP_POP));
                    }
                }
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
                self.emit(Instr::none(if *value {
                    OP_PUSH_TRUE
                } else {
                    OP_PUSH_FALSE
                }));
            }
            AstNode::Nil { .. } => self.emit(Instr::none(OP_PUSH_NIL)),

            AstNode::Ident { name, .. } => {
                // Check if this is a registered global (enum variant, type name)
                if self.is_global(name) {
                    let g_slot = self.global(name);
                    self.emit(Instr::a1(OP_LOAD_GLOBAL, g_slot));
                } else if let Some(slot) = self.root_read_slot(name) {
                    self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                } else {
                    let slot = self.local(name);
                    if !self.try_fuse_load_load(slot) {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    }
                }
            }

            AstNode::BinaryOp {
                left, op, right, ..
            } => {
                self.node(left);
                self.node(right);
                // Try superinstructions AFTER emitting operands
                if let Some(opc) = local_local_opcode(*op)
                    && self.try_fuse_local_local(opc)
                {
                    return;
                }
                let opcode = binop_opcode(*op);
                self.emit(Instr::none(opcode));
            }

            AstNode::UnaryOp { op, operand, .. } => {
                self.node(operand);
                let opcode = match op {
                    UnaryOperator::Neg => OP_NEG,
                    UnaryOperator::Not => OP_NOT,
                    UnaryOperator::BitNot => OP_BITNOT,
                };
                self.emit(Instr::none(opcode));
            }

            AstNode::ArrayLit { elements, .. } => {
                for e in elements {
                    self.node(e);
                }
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

            AstNode::Block { statements, .. } => {
                for s in statements {
                    self.node(s);
                }
            }

            AstNode::Index { target, index, .. } => {
                self.node(target);
                self.node(index);
                self.emit(Instr::none(OP_GET_INDEX));
            }

            AstNode::FieldAccess { target, field, .. } => {
                // A dotted chain rooted at a plain identifier is a domain
                // method: `metadata.all` in expression position means the
                // same as the statement form. Without this it compiled to a
                // field read on an uninitialized local named `metadata`.
                // `$var.field`, globals, and type/variant names stay field
                // reads.
                if let Some(name) = self.zero_arg_domain_call(node) {
                    let i = self.intern(&name);
                    if let Ok(name16) = u16::try_from(i) {
                        self.emit(Instr::make(OP_CALL_NAME, 0, name16, 0));
                    } else {
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        self.emit(Instr::a2(OP_CALL, 0, 0));
                    }
                    return;
                }
                self.node(target);
                let i = self.intern(field);
                self.emit(Instr::a1(OP_GET_FIELD, i));
            }

            AstNode::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.node(s);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                if let Some(e) = end {
                    self.node(e);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                self.emit(Instr::none(OP_MAKE_RANGE));
            }

            AstNode::Lambda { params, body, .. } => {
                // Resolve the body's free names against the enclosing scope:
                // values that live in this frame are captured by value.
                let mut reads = Vec::new();
                let mut writes = Vec::new();
                scan_vars(body, &mut reads, &mut writes);
                let mut capture_names: Vec<String> = Vec::new();
                let mut capture_slots: Vec<u32> = Vec::new();
                for name in &reads {
                    if params.contains(name) || writes.iter().any(|w| w == name) {
                        continue;
                    }
                    if let Some(&slot) = self.local_index.get(name.as_str()) {
                        capture_names.push(name.clone());
                        capture_slots.push(slot);
                    }
                    // Top-level variables are read through `OP_LOAD_ROOT` and
                    // do not need to be captured.
                }

                let mut sub = self.body_compiler();
                for p in params {
                    sub.local(p);
                }
                // Captures occupy the slots right after the parameters; the VM
                // writes their values there when the closure is entered.
                for c in &capture_names {
                    sub.local(c);
                }
                for w in &writes {
                    sub.local(w);
                }
                sub.node(body);
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                let f = FuncValue {
                    name: "lambda".into(),
                    bytecode: Some(Arc::new(bc)),
                    params: params.clone(),
                    closure_captures: capture_names,
                    closure_env: Vec::new(),
                    uses_special_vars: sub.uses_special_vars,
                };
                let idx = self.closures.len() as u32;
                self.closures.push(f);
                // Push the captured values, then build the closure from them.
                for slot in capture_slots {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                }
                self.emit(Instr::a1(OP_LOAD_LAMBDA, idx));
            }

            AstNode::Pipeline { input, stages, .. } => {
                // A pipeline passes the accumulator as the first argument. The
                // stage is a named callee, so `OP_CALL_NAME` is the direct
                // encoding: `[acc, arg]` plus the name in the opcode.
                self.node(input);
                let depth = self.pipeline_depth;
                self.pipeline_depth += 1;
                for stage in stages {
                    if let Some(a) = &stage.arg {
                        self.node(a);
                    } else {
                        self.emit(Instr::none(OP_PUSH_NIL));
                    }
                    let i = self.intern(&stage.name);
                    if let Ok(name16) = u16::try_from(i) {
                        self.emit(Instr::make(OP_CALL_NAME, 2, name16, 0));
                    } else {
                        // String-table overflow: rebuild `[callee, acc, arg]`
                        // explicitly (more than 65535 distinct strings).
                        let base = PIPE_TEMP_BASE + depth * 2;
                        let arg = self.temp_slot(base);
                        self.emit(Instr::a1(OP_STORE_LOCAL, arg));
                        let acc = self.temp_slot(base + 1);
                        self.emit(Instr::a1(OP_STORE_LOCAL, acc));
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        self.emit(Instr::a1(OP_LOAD_LOCAL, acc));
                        self.emit(Instr::a1(OP_LOAD_LOCAL, arg));
                        self.emit(Instr::a2(OP_CALL, 2, 0));
                    }
                }
                self.pipeline_depth = depth;
            }

            // Type definition: compile body (fields + methods), register type name as a global
            AstNode::TypeDef {
                name, kind, body, ..
            } => {
                // Compile any methods in the body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Register the type name as a global
                let type_idx = self.intern(name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                let g_slot = self.global(name);
                self.emit(Instr::a1(OP_STORE_GLOBAL, g_slot));
                // For sum types, register each variant as a global string constant
                if let TypeKind::Sum { variants } = kind {
                    for v in variants {
                        let v_idx = self.intern(&v.name);
                        self.emit(Instr::a1(OP_PUSH_STR, v_idx));
                        let vg_slot = self.global(&v.name);
                        self.emit(Instr::a1(OP_STORE_GLOBAL, vg_slot));
                    }
                }
            }

            // Impl block: compile methods, associate with type
            AstNode::ImplDef {
                type_name,
                trait_name,
                body,
                ..
            } => {
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
            AstNode::TypeTest {
                value, type_name, ..
            } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_TEST));
            }

            // Type cast: $x as TypeName
            AstNode::TypeCast {
                value, type_name, ..
            } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_CAST));
            }

            AstNode::SpecialVar { kind, index, .. } => {
                self.uses_special_vars = true;
                use crate::runtime::opcode::special_var_kind as sv;
                let kind_code = match kind {
                    SpecialVarKind::ScriptName => sv::SCRIPT_NAME,
                    SpecialVarKind::Positional => sv::POSITIONAL,
                    SpecialVarKind::ArgCount => sv::ARG_COUNT,
                    SpecialVarKind::ArgsArray => sv::ARGS_ARRAY,
                    SpecialVarKind::ArgsJoined => sv::ARGS_JOINED,
                    SpecialVarKind::SelfValue => sv::SELF,
                    SpecialVarKind::SuperValue => sv::SUPER,
                };
                // a = positional index (32-bit), c = kind (8-bit)
                self.emit(Instr::make(OP_SPECIAL_VAR, *index, 0, kind_code));
            }
        }
    }
}

/// Recursively collect a dotted name from a chain of FieldAccess nodes.
/// e.g., FieldAccess(FieldAccess(Ident("net"), "tcp"), "connect") -> Some("net.tcp.connect")
/// Returns None if the chain contains anything other than Ident + FieldAccess.
/// Walks one statement or expression, collecting variable reads (first-use
/// order) and the names assigned in this scope.
///
/// Nested function and lambda bodies are skipped: their free variables are
/// resolved when those bodies are compiled.
fn scan_vars(node: &AstNode, reads: &mut Vec<String>, writes: &mut Vec<String>) {
    match node {
        AstNode::Assign { name, value, .. } => {
            if !writes.iter().any(|n| n == name) {
                writes.push(name.clone());
            }
            scan_vars(value, reads, writes);
        }
        AstNode::For {
            variable,
            iterable,
            body,
            ..
        } => {
            if !writes.iter().any(|n| n == variable) {
                writes.push(variable.clone());
            }
            scan_vars(iterable, reads, writes);
            for s in body {
                scan_vars(s, reads, writes);
            }
        }
        AstNode::Return { value: Some(v), .. } => scan_vars(v, reads, writes),
        AstNode::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            scan_vars(condition, reads, writes);
            for s in then_branch {
                scan_vars(s, reads, writes);
            }
            if let Some(b) = else_branch {
                for s in b {
                    scan_vars(s, reads, writes);
                }
            }
        }
        AstNode::While {
            condition, body, ..
        } => {
            scan_vars(condition, reads, writes);
            for s in body {
                scan_vars(s, reads, writes);
            }
        }
        AstNode::Block { statements, .. } => {
            for s in statements {
                scan_vars(s, reads, writes);
            }
        }
        AstNode::Call { callee, args, .. } => {
            scan_vars(callee, reads, writes);
            for a in args {
                scan_vars(&a.value, reads, writes);
            }
        }
        AstNode::DomainCall {
            callee,
            args,
            items,
            ..
        } => {
            scan_vars(callee, reads, writes);
            for a in args {
                scan_vars(&a.value, reads, writes);
            }
            for item in items {
                scan_vars(&item.value, reads, writes);
                for (_, v) in &item.named {
                    scan_vars(v, reads, writes);
                }
            }
        }
        AstNode::Ident { name, .. } => {
            if !reads.iter().any(|n| n == name) {
                reads.push(name.clone());
            }
        }
        AstNode::BinaryOp { left, right, .. } => {
            scan_vars(left, reads, writes);
            scan_vars(right, reads, writes);
        }
        AstNode::UnaryOp { operand, .. } => scan_vars(operand, reads, writes),
        AstNode::Index { target, index, .. } => {
            scan_vars(target, reads, writes);
            scan_vars(index, reads, writes);
        }
        AstNode::FieldAccess { target, .. } => scan_vars(target, reads, writes),
        AstNode::Pipeline { input, stages, .. } => {
            scan_vars(input, reads, writes);
            for stage in stages {
                if let Some(a) = &stage.arg {
                    scan_vars(a, reads, writes);
                }
            }
        }
        AstNode::ArrayLit { elements, .. } => {
            for e in elements {
                scan_vars(e, reads, writes);
            }
        }
        AstNode::DictLit { entries, .. } => {
            for (_, v) in entries {
                scan_vars(v, reads, writes);
            }
        }
        AstNode::Range { start, end, .. } => {
            if let Some(s) = start {
                scan_vars(s, reads, writes);
            }
            if let Some(e) = end {
                scan_vars(e, reads, writes);
            }
        }
        AstNode::TypeTest { value, .. } | AstNode::TypeCast { value, .. } => {
            scan_vars(value, reads, writes);
        }
        // A nested lambda's free variables are also free here (unless one of
        // its parameters or body locals binds them), so they must be captured
        // by this scope too.
        AstNode::Lambda { params, body, .. } => {
            let mut inner_reads = Vec::new();
            let mut inner_writes = Vec::new();
            scan_vars(body, &mut inner_reads, &mut inner_writes);
            for name in inner_reads {
                if params.contains(&name) || inner_writes.iter().any(|w| w == &name) {
                    continue;
                }
                if !reads.iter().any(|n| n == &name) {
                    reads.push(name);
                }
            }
        }
        // Named function bodies resolve free names through the root frame, not
        // through captures, so they are not part of this scope.
        AstNode::FunctionDef { .. } => {}
        _ => {}
    }
}

/// Names assigned in the top-level frame, in first-appearance order.
///
/// These are the variables a function body may read through `OP_LOAD_ROOT`.
fn collect_top_level_vars(ast: &[AstNode]) -> Vec<String> {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    for node in ast {
        scan_vars(node, &mut reads, &mut writes);
    }
    writes
}

fn collect_dotted_name(node: &AstNode) -> Option<String> {
    match node {
        AstNode::Ident { name, .. } => Some(name.clone()),
        AstNode::FieldAccess { target, field, .. } => {
            let base = collect_dotted_name(target)?;
            Some(format!("{}.{}", base, field))
        }
        _ => None,
    }
}

/// Compiles an Abstract Syntax Tree (AST) slice into an executable [`Bytecode`] bundle.
///
/// Translates AST nodes into 64-bit [`Instr`] instructions, performs string interning,
/// registers wide numeric operands, and runs the peephole optimizer.
///
/// # Arguments
///
/// * `ast` - A slice of top-level [`AstNode`] statements to compile.
///
/// # Returns
///
/// A fully optimized [`Bytecode`] ready for execution by [`crate::vm::VM::run`].
///
/// # Examples
///
/// ```
/// use xcl_vm::compiler::lexer::lex;
/// use xcl_vm::compiler::parser::parse;
/// use xcl_vm::bytecode::compile;
///
/// let tokens = lex("$x = 10; $x + 5", "example.xcl");
/// let ast = parse(&tokens, "$x = 10; $x + 5");
/// let bytecode = compile(&ast);
///
/// assert!(!bytecode.instructions.is_empty());
/// ```

/// Check whether an AST expression is "pure" for inlining purposes.
/// Pure = no Call, Assign, While, For, If, Lambda, FunctionDef, SpecialVar, Break, Continue.
fn is_pure_expr(node: &AstNode) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_pure_expr(operand),
        AstNode::BinaryOp { left, right, .. } => is_pure_expr(left) && is_pure_expr(right),
        AstNode::FieldAccess { target, .. } => is_pure_expr(target),
        AstNode::Index { target, index, .. } => is_pure_expr(target) && is_pure_expr(index),
        AstNode::ArrayLit { elements, .. } => elements.iter().all(is_pure_expr),
        AstNode::DictLit { entries, .. } => entries.iter().all(|(_, v)| is_pure_expr(v)),
        AstNode::Range { start, end, .. } => {
            start.as_ref().map_or(true, |s| is_pure_expr(s.as_ref()))
                && end.as_ref().map_or(true, |e| is_pure_expr(e.as_ref()))
        }
        AstNode::Block { statements, .. } => statements.iter().all(is_pure_expr),
        // Impure constructs
        AstNode::Call { .. }
        | AstNode::Assign { .. }
        | AstNode::While { .. }
        | AstNode::For { .. }
        | AstNode::If { .. }
        | AstNode::Lambda { .. }
        | AstNode::FunctionDef { .. }
        | AstNode::SpecialVar { .. }
        | AstNode::Break { .. }
        | AstNode::Continue { .. }
        | AstNode::Return { .. }
        | AstNode::Pipeline { .. }
        | AstNode::TypeDef { .. }
        | AstNode::ImplDef { .. }
        | AstNode::TypeTest { .. }
        | AstNode::TypeCast { .. }
        | AstNode::DomainCall { .. } => false,
    }
}

/// Purity check for multi-statement block expressions.
/// Like `is_pure_expr` but allows `Call` when the callee is an inlineable function.
fn is_pure_block_expr(node: &AstNode, inlineable_fns: &BTreeSet<String>) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_pure_block_expr(operand, inlineable_fns),
        AstNode::BinaryOp { left, right, .. } => {
            is_pure_block_expr(left, inlineable_fns) && is_pure_block_expr(right, inlineable_fns)
        }
        AstNode::FieldAccess { target, .. } => is_pure_block_expr(target, inlineable_fns),
        AstNode::Index { target, index, .. } => {
            is_pure_block_expr(target, inlineable_fns) && is_pure_block_expr(index, inlineable_fns)
        }
        AstNode::ArrayLit { elements, .. } => elements
            .iter()
            .all(|e| is_pure_block_expr(e, inlineable_fns)),
        AstNode::DictLit { entries, .. } => entries
            .iter()
            .all(|(_, v)| is_pure_block_expr(v, inlineable_fns)),
        AstNode::Range { start, end, .. } => {
            start
                .as_ref()
                .map_or(true, |s| is_pure_block_expr(s.as_ref(), inlineable_fns))
                && end
                    .as_ref()
                    .map_or(true, |e| is_pure_block_expr(e.as_ref(), inlineable_fns))
        }
        AstNode::Call { callee, args, .. } => {
            // Call is allowed only if callee is an identifier matching an inlineable function
            if let AstNode::Ident { name, .. } = callee.as_ref() {
                if !inlineable_fns.contains(name) {
                    return false;
                }
            } else {
                return false;
            }
            args.iter()
                .all(|a| is_pure_block_expr(&a.value, inlineable_fns))
        }
        // Impure constructs
        AstNode::Assign { .. }
        | AstNode::While { .. }
        | AstNode::For { .. }
        | AstNode::If { .. }
        | AstNode::Lambda { .. }
        | AstNode::FunctionDef { .. }
        | AstNode::SpecialVar { .. }
        | AstNode::Break { .. }
        | AstNode::Continue { .. }
        | AstNode::Return { .. }
        | AstNode::Pipeline { .. }
        | AstNode::TypeDef { .. }
        | AstNode::ImplDef { .. }
        | AstNode::TypeTest { .. }
        | AstNode::TypeCast { .. }
        | AstNode::DomainCall { .. }
        | AstNode::Block { .. } => false,
    }
}

/// Purity check excluding Call entirely (for the first pass in collect_inlineable).
fn is_pure_expr_no_call(node: &AstNode) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_pure_expr_no_call(operand),
        AstNode::BinaryOp { left, right, .. } => {
            is_pure_expr_no_call(left) && is_pure_expr_no_call(right)
        }
        AstNode::FieldAccess { target, .. } => is_pure_expr_no_call(target),
        AstNode::Index { target, index, .. } => {
            is_pure_expr_no_call(target) && is_pure_expr_no_call(index)
        }
        AstNode::ArrayLit { elements, .. } => elements.iter().all(is_pure_expr_no_call),
        AstNode::DictLit { entries, .. } => entries.iter().all(|(_, v)| is_pure_expr_no_call(v)),
        AstNode::Range { start, end, .. } => {
            start
                .as_ref()
                .map_or(true, |s| is_pure_expr_no_call(s.as_ref()))
                && end
                    .as_ref()
                    .map_or(true, |e| is_pure_expr_no_call(e.as_ref()))
        }
        _ => false,
    }
}

/// Check whether a call argument is "simple" for inlining (criterion 4).
/// Simple = Ident, Int, Float, Str, Bool, Nil, or pure UnaryOp/BinaryOp of simple values.
fn is_simple_arg(node: &AstNode) -> bool {
    match node {
        AstNode::Ident { .. }
        | AstNode::Int { .. }
        | AstNode::Float { .. }
        | AstNode::Str { .. }
        | AstNode::Bool { .. }
        | AstNode::Nil { .. } => true,
        AstNode::UnaryOp { operand, .. } => is_simple_arg(operand),
        AstNode::BinaryOp { left, right, .. } => is_simple_arg(left) && is_simple_arg(right),
        _ => false,
    }
}

/// Collect all identifiers referenced in an expression.
fn collect_idents(node: &AstNode, out: &mut Vec<String>) {
    match node {
        AstNode::Ident { name, .. } => out.push(name.clone()),
        AstNode::UnaryOp { operand, .. } => collect_idents(operand, out),
        AstNode::BinaryOp { left, right, .. } => {
            collect_idents(left, out);
            collect_idents(right, out);
        }
        AstNode::FieldAccess { target, .. } => collect_idents(target, out),
        AstNode::Index { target, index, .. } => {
            collect_idents(target, out);
            collect_idents(index, out);
        }
        AstNode::ArrayLit { elements, .. } => {
            for e in elements {
                collect_idents(e, out);
            }
        }
        AstNode::DictLit { entries, .. } => {
            for (_, v) in entries {
                collect_idents(v, out);
            }
        }
        AstNode::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_idents(s, out);
            }
            if let Some(e) = end {
                collect_idents(e, out);
            }
        }
        AstNode::Block { statements, .. } => {
            for s in statements {
                collect_idents(s, out);
            }
        }
        _ => {}
    }
}

/// Substitute parameter names with argument expressions in a cloned body.
fn substitute(node: &AstNode, subs: &[(String, AstNode)]) -> AstNode {
    match node {
        AstNode::Ident { name, .. } => {
            for (param, arg) in subs {
                if param == name {
                    return arg.clone();
                }
            }
            node.clone()
        }
        AstNode::UnaryOp { op, operand, span } => AstNode::UnaryOp {
            op: *op,
            operand: Box::new(substitute(operand, subs)),
            span: span.clone(),
        },
        AstNode::BinaryOp {
            left,
            op,
            right,
            span,
        } => AstNode::BinaryOp {
            left: Box::new(substitute(left, subs)),
            op: *op,
            right: Box::new(substitute(right, subs)),
            span: span.clone(),
        },
        AstNode::FieldAccess {
            target,
            field,
            span,
        } => AstNode::FieldAccess {
            target: Box::new(substitute(target, subs)),
            field: field.clone(),
            span: span.clone(),
        },
        AstNode::Index {
            target,
            index,
            span,
        } => AstNode::Index {
            target: Box::new(substitute(target, subs)),
            index: Box::new(substitute(index, subs)),
            span: span.clone(),
        },
        AstNode::ArrayLit { elements, span } => AstNode::ArrayLit {
            elements: elements.iter().map(|e| substitute(e, subs)).collect(),
            span: span.clone(),
        },
        AstNode::DictLit { entries, span } => AstNode::DictLit {
            entries: entries
                .iter()
                .map(|(k, v)| (k.clone(), substitute(v, subs)))
                .collect(),
            span: span.clone(),
        },
        AstNode::Range { start, end, span } => AstNode::Range {
            start: start.as_ref().map(|s| Box::new(substitute(s, subs))),
            end: end.as_ref().map(|e| Box::new(substitute(e, subs))),
            span: span.clone(),
        },
        AstNode::Block { statements, span } => AstNode::Block {
            statements: statements.iter().map(|s| substitute(s, subs)).collect(),
            span: span.clone(),
        },
        AstNode::Call { callee, args, span } => AstNode::Call {
            callee: Box::new(substitute(callee, subs)),
            args: args
                .iter()
                .map(|a| CallArg {
                    name: a.name.clone(),
                    value: substitute(&a.value, subs),
                })
                .collect(),
            span: span.clone(),
        },
        _ => node.clone(),
    }
}

/// Pre-pass: collect inlineable functions from the AST.
fn collect_inlineable(ast: &[AstNode]) -> BTreeMap<String, InlineFn> {
    let mut map = BTreeMap::new();

    // First pass: identify which functions are single-expr inlineable (needed for
    // checking whether Call targets in multi-statement bodies are inlineable).
    let single_expr_fns: BTreeSet<String> = ast
        .iter()
        .filter_map(|node| {
            if let AstNode::FunctionDef {
                name, params, body, ..
            } = node
            {
                if body.len() == 1 {
                    if let AstNode::Return {
                        value: Some(expr), ..
                    } = &body[0]
                    {
                        let param_names: Vec<String> =
                            params.iter().map(|p| p.name.clone()).collect();
                        let mut idents = Vec::new();
                        collect_idents(expr, &mut idents);
                        if idents.iter().all(|id| param_names.contains(id))
                            && is_pure_expr_no_call(expr)
                        {
                            return Some(name.clone());
                        }
                    }
                }
            }
            None
        })
        .collect();

    for node in ast {
        if let AstNode::FunctionDef {
            name, params, body, ..
        } = node
        {
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

            // Try single-expression form first
            if body.len() == 1 {
                if let AstNode::Return {
                    value: Some(expr), ..
                } = &body[0]
                {
                    if is_pure_expr(expr) {
                        let mut idents = Vec::new();
                        collect_idents(expr, &mut idents);
                        let all_ok = idents.iter().all(|id| {
                            param_names.contains(id)
                                || is_known_global(id)
                                || single_expr_fns.contains(id)
                        });
                        if all_ok {
                            map.insert(
                                name.clone(),
                                InlineFn {
                                    params: param_names,
                                    body: InlineBody::Expr(expr.as_ref().clone()),
                                },
                            );
                            continue;
                        }
                    }
                }
            }

            // Try multi-statement block form:
            // Zero or more Assign statements followed by exactly one Return { value: Some(result) }
            if !body.is_empty() {
                // Last statement must be Return { value: Some(result) }
                let last = &body[body.len() - 1];
                let AstNode::Return {
                    value: Some(result),
                    ..
                } = last
                else {
                    continue;
                };

                // All preceding statements must be Assign
                let mut assigns: Vec<(String, AstNode)> = Vec::new();
                let mut body_locals: Vec<String> = Vec::new();
                let mut ok = true;

                for stmt in &body[..body.len() - 1] {
                    if let AstNode::Assign {
                        name: aname,
                        value: aval,
                        ..
                    } = stmt
                    {
                        // Name must not be a parameter
                        if param_names.contains(aname) {
                            ok = false;
                            break;
                        }
                        // Each body-local assigned exactly once
                        if body_locals.contains(aname) {
                            ok = false;
                            break;
                        }
                        body_locals.push(aname.clone());
                        assigns.push((aname.clone(), aval.as_ref().clone()));
                    } else {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }

                // Check purity of all assigned expressions and the result
                let all_assigns_pure = assigns
                    .iter()
                    .all(|(_, expr)| is_pure_block_expr(expr, &single_expr_fns));
                if !all_assigns_pure {
                    continue;
                }
                if !is_pure_block_expr(result, &single_expr_fns) {
                    continue;
                }

                // Check all identifiers: must be params, body-locals, known globals, or inlineable callees
                let mut all_idents = Vec::new();
                for (_, expr) in &assigns {
                    collect_idents(expr, &mut all_idents);
                }
                collect_idents(result, &mut all_idents);

                let all_ok = all_idents.iter().all(|id| {
                    param_names.contains(id)
                        || body_locals.contains(id)
                        || is_known_global(id)
                        || single_expr_fns.contains(id)
                });
                if !all_ok {
                    continue;
                }

                // If there are no assigns, treat as Expr form
                if assigns.is_empty() {
                    map.insert(
                        name.clone(),
                        InlineFn {
                            params: param_names,
                            body: InlineBody::Expr(result.as_ref().clone()),
                        },
                    );
                } else {
                    map.insert(
                        name.clone(),
                        InlineFn {
                            params: param_names,
                            body: InlineBody::Block {
                                assigns,
                                result: result.as_ref().clone(),
                            },
                        },
                    );
                }
            }
        }
    }
    map
}

/// Check if an identifier is a known global (type names, enum variants, etc.)
fn is_known_global(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "isize"
            | "usize"
            | "f32"
            | "f64"
            | "bool"
            | "str"
            | "string"
            | "char"
            | "none"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "Result"
            | "Option"
            | "Array"
            | "Map"
            | "Dict"
            | "Range"
            | "Vec"
            | "print"
            | "println"
            | "input"
            | "len"
            | "enumerate"
            | "zip"
            | "filter"
            | "map"
            | "reduce"
            | "fold"
            | "sort"
            | "reverse"
            | "to_string"
            | "to_int"
            | "to_float"
            | "trim"
            | "split"
            | "join"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "replace"
            | "abs"
            | "min"
            | "max"
            | "sqrt"
            | "pow"
            | "sin"
            | "cos"
            | "tan"
            | "log"
            | "exp"
            | "round"
            | "floor"
            | "ceil"
            | "true"
            | "false"
    )
}

pub fn compile(ast: &[AstNode]) -> Bytecode {
    let inline_map = collect_inlineable(ast);
    let root_vars = collect_top_level_vars(ast);
    let mut c = Compiler::new()
        .with_root_vars(&root_vars)
        .with_inline_map(inline_map);
    c.compile(ast)
}

/// Compiles without the peephole and data-flow passes, returning the bytecode
/// together with the source line of every top-level statement.
///
/// Each entry is `(first_ip, line)` and the entries are in source order, so
/// `ip -> line` is a scan of the map. Debuggers and the playground use this to
/// point at the statement that produced an instruction; normal execution keeps
/// using [`compile`], whose optimizing passes rewrite the offsets.
pub fn compile_unoptimized(ast: &[AstNode]) -> (Bytecode, Vec<(u32, u32)>) {
    let inline_map = collect_inlineable(ast);
    let root_vars = collect_top_level_vars(ast);
    let mut c = Compiler::new()
        .with_root_vars(&root_vars)
        .with_inline_map(inline_map);
    let mut ranges = Vec::new();
    let bytecode = c.compile_with(ast, false, &mut ranges);
    (bytecode, ranges)
}

/// First source line of a node. `0` means the span was synthesised.
fn node_line(node: &AstNode) -> u32 {
    let span = match node {
        AstNode::Assign { span, .. }
        | AstNode::Return { span, .. }
        | AstNode::Break { span }
        | AstNode::Continue { span }
        | AstNode::If { span, .. }
        | AstNode::For { span, .. }
        | AstNode::While { span, .. }
        | AstNode::FunctionDef { span, .. }
        | AstNode::Lambda { span, .. }
        | AstNode::Call { span, .. }
        | AstNode::DomainCall { span, .. }
        | AstNode::Ident { span, .. }
        | AstNode::Int { span, .. }
        | AstNode::Float { span, .. }
        | AstNode::Str { span, .. }
        | AstNode::Bool { span, .. }
        | AstNode::Nil { span }
        | AstNode::BinaryOp { span, .. }
        | AstNode::UnaryOp { span, .. }
        | AstNode::Pipeline { span, .. }
        | AstNode::ArrayLit { span, .. }
        | AstNode::DictLit { span, .. }
        | AstNode::Index { span, .. }
        | AstNode::FieldAccess { span, .. }
        | AstNode::Range { span, .. }
        | AstNode::Block { span, .. }
        | AstNode::TypeDef { span, .. }
        | AstNode::ImplDef { span, .. }
        | AstNode::TypeTest { span, .. }
        | AstNode::TypeCast { span, .. }
        | AstNode::SpecialVar { span, .. } => span,
    };
    span.line
}
