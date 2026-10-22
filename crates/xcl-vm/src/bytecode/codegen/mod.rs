//! AST-to-bytecode code generation.
//!
//! Translates parsed XCL AST nodes into 64-bit packed instructions. The
//! [`Compiler`] struct accumulates instructions, wide operands, string interning
//! and local/global slot allocation; the peephole optimizer and data-flow passes
//! then rewrite the stream before it becomes the final [`Bytecode`].
//!
//! The front end (source → AST) is [`crate::compiler`]; this module is the back
//! end (AST → instructions) and is private to [`super`].
//!
//! # Layout
//!
//! | File | Holds |
//! |---|---|
//! | `mod.rs` | `Compiler` state, slot/interning helpers, entry points |
//! | `expr.rs` | [`Compiler::node`], the AST dispatcher |
//! | `emit.rs` | slot-directed emission and call arguments |
//! | `fuse.rs` | superinstruction fusion |
//! | `inline.rs` | whole-program purity / inlining pre-pass |
//! | `opcode_map.rs` | operator → opcode tables |
//!
//! Everything except the entry points is `pub(super)`: the split is for readers,
//! not a public API.

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::string::String;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::collections::BTreeMap;
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
use crate::runtime::vm::HotTable;

/// Temp-slot namespace for pipeline accumulators. Ordinary expression temps use
/// small depths, so this range cannot collide with them.
const PIPE_TEMP_BASE: u32 = 1 << 20;

mod emit;
mod expr;
mod fuse;
mod inline;
mod opcode_map;

use self::inline::{collect_inlineable, collect_top_level_vars};

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

struct Compiler<'h> {
    code: Vec<Instr>,
    wide: Vec<i64>,
    strings: Vec<Box<str>>,
    locals: Vec<Box<str>>,
    globals: Vec<Box<str>>,
    functions: Vec<(Box<str>, Arc<FuncValue>)>,
    closures: Vec<FuncValue>,
    /// Callable name → `OP_CALL_HOT` slot, when compiling against a live VM.
    /// `None` compiles every call as `OP_CALL_NAME`.
    hot: Option<&'h HotTable>,
    /// Whether the current function body reads `$1..$N`, `$#`, `$@`, or `$*`.
    ///
    /// `$self`, `$super`, and `$0` do not need the positional argument vector:
    /// the first argument is always stored in `Frame::arg0`, and `$0` comes from
    /// the VM. Keeping this flag precise avoids an `argv` allocation per call.
    uses_argv: bool,
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
    /// Side maps for O(1) lookup instead of O(n) `Vec::position` scans.
    string_index: HashMap<Box<str>, u32>,
    local_index: HashMap<Box<str>, u32>,
    global_index: HashMap<Box<str>, u32>,
}

impl<'h> Compiler<'h> {
    fn new() -> Self {
        Self {
            code: Vec::with_capacity(64),
            wide: Vec::new(),
            strings: Vec::new(),
            locals: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            closures: Vec::new(),
            hot: None,
            uses_argv: false,
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

    /// Compiles call sites against the VM's hot-native slots.
    fn with_hot_table(mut self, hot: Option<&'h HotTable>) -> Self {
        self.hot = hot;
        self
    }

    /// Slot for a callable name, or `None` when the call must resolve by name.
    #[inline]
    fn hot_slot(&self, name_idx: u32) -> Option<u16> {
        let table = self.hot?;
        table.slot(&self.strings[name_idx as usize])
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
        c.hot = self.hot;
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

        // Callee lookup for `OP_CALL_NAME`: one array load instead of a hash.
        // Function names are interned by their call sites, so a function that is
        // never called simply stays absent.
        let mut function_by_name: Vec<u32> = core::iter::repeat_n(0u32, strings.len()).collect();
        for (ordinal, (name, _)) in self.functions.iter().enumerate() {
            if let Some(&idx) = self.string_index.get(name) {
                function_by_name[idx as usize] = ordinal as u32 + 1;
            }
        }

        Bytecode {
            instructions: code.into(),
            wide_operands: core::mem::take(&mut self.wide).into(),
            string_table: strings.into(),
            string_values: string_values.into(),
            local_names: core::mem::take(&mut self.locals).into(),
            global_names: core::mem::take(&mut self.globals).into(),
            user_functions: core::mem::take(&mut self.functions).into(),
            function_by_name: function_by_name.into(),
            closures: core::mem::take(&mut self.closures).into(),
        }
    }
}

/// Recursively collect a dotted name from a chain of FieldAccess nodes.
/// e.g., FieldAccess(FieldAccess(Ident("net"), "tcp"), "connect") -> Some("net.tcp.connect")
/// Returns None if the chain contains anything other than Ident + FieldAccess.
pub(super) fn collect_dotted_name(node: &AstNode) -> Option<String> {
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
/// A fully optimized [`Bytecode`] ready for execution by [`VM::run`](crate::VM::run).
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
pub fn compile(ast: &[AstNode]) -> Bytecode {
    let inline_map = collect_inlineable(ast);
    let root_vars = collect_top_level_vars(ast);
    let mut c = Compiler::new()
        .with_root_vars(&root_vars)
        .with_inline_map(inline_map);
    c.compile(ast)
}

/// Compiles `ast`, binding every call whose name the VM made hot to a direct
/// `OP_CALL_HOT` instruction.
///
/// The slots come from the VM that will run the result, which is why this takes
/// a [`HotTable`] rather than reading a global registry: `xcl_vm::evaluate`
/// compiles and runs with the same VM, so the slot numbers always agree.
/// Bytecode containing `OP_CALL_HOT` must not be run on a different VM; run it
/// through [`compile`] when it needs to be portable.
pub fn compile_hot(ast: &[AstNode], hot: &HotTable) -> Bytecode {
    let inline_map = collect_inlineable(ast);
    let root_vars = collect_top_level_vars(ast);
    let mut c = Compiler::new()
        .with_root_vars(&root_vars)
        .with_inline_map(inline_map)
        .with_hot_table(Some(hot));
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
