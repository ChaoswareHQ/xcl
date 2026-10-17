//! Abstract Syntax Tree (AST) node definitions for XCL.
//!
//! # Overview
//!
//! The AST represents the syntactic structure of an XCL program produced by the
//! [`parser`](crate::parser) and consumed by the [`bytecode`](crate::bytecode) compiler.
//!
//! Every AST node records a [`Span`] indicating its original source location for
//! accurate error diagnostics, debugging symbols, and stack traces.
//!
//! # Expression-Oriented Design
//!
//! In XCL, almost all constructs can be used as expressions:
//!
//! * [`AstNode::If`](crate::ast::AstNode::If) evaluates to the value of its executed branch.
//! * [`AstNode::Block`](crate::ast::AstNode::Block) evaluates to the value of its trailing statement.
//! * [`AstNode::Pipeline`](crate::ast::AstNode::Pipeline) chains expressions through sequential function stages.
//!
//! # Examples
//!
//! Matching on an AST node to inspect its type:
//!
//! ```
//! use xcl_vm::ast::{AstNode, BinaryOperator};
//! use xcl_vm::Span;
//!
//! let node = AstNode::BinaryOp {
//!     span: Span::empty(),
//!     left: Box::new(AstNode::Int { span: Span::empty(), value: 10 }),
//!     op: BinaryOperator::Add,
//!     right: Box::new(AstNode::Int { span: Span::empty(), value: 20 }),
//! };
//!
//! match &node {
//!     AstNode::BinaryOp { op: BinaryOperator::Add, left, right, .. } => {
//!         println!("Addition expression with left={:?}, right={:?}", left, right);
//!     }
//!     _ => {}
//! }
//! ```

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::span::Span;

/// Abstract syntax tree node representing a parsed XCL language construct.
///
/// Each variant models a specific statement or expression in the XCL grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum AstNode {
    /// Variable assignment or binding: `x = 42` or `x: i64 = 42`.
    Assign {
        /// Source location of the assignment.
        span: Span,
        /// Name of the variable being assigned.
        name: String,
        /// Right-hand side expression being assigned.
        value: Box<AstNode>,
        /// Optional type annotation (e.g., `i64`, `str`).
        type_annotation: Option<String>,
    },
    /// Return statement: `return value` or `return`.
    Return {
        /// Source location of the return statement.
        span: Span,
        /// Optional return value expression (`None` evaluates to `none`/unit).
        value: Option<Box<AstNode>>,
    },
    /// Break statement to prematurely exit a loop: `break`.
    Break {
        /// Source location of the break statement.
        span: Span,
    },
    /// Continue statement to jump to the next loop iteration: `continue`.
    Continue {
        /// Source location of the continue statement.
        span: Span,
    },
    /// Conditional branch: `if cond { ... } else { ... }`.
    If {
        /// Source location of the if statement.
        span: Span,
        /// Condition expression that determines which branch executes.
        condition: Box<AstNode>,
        /// Statements executed when the condition evaluates to truthy.
        then_branch: Vec<AstNode>,
        /// Optional statements executed when the condition evaluates to falsy.
        else_branch: Option<Vec<AstNode>>,
    },
    /// For-in loop over a collection or range: `for item in iterable { ... }`.
    For {
        /// Source location of the for loop.
        span: Span,
        /// Loop variable identifier.
        variable: String,
        /// Expression producing the iterable collection or range.
        iterable: Box<AstNode>,
        /// Body statements executed for each iteration.
        body: Vec<AstNode>,
    },
    /// While loop: `while cond { ... }`.
    While {
        /// Source location of the while loop.
        span: Span,
        /// Condition expression evaluated before each iteration.
        condition: Box<AstNode>,
        /// Body statements executed while the condition remains truthy.
        body: Vec<AstNode>,
    },
    /// Function definition: `fn name(params) -> type { ... }`.
    FunctionDef {
        /// Source location of the function definition.
        span: Span,
        /// Function identifier name.
        name: String,
        /// List of formal parameters.
        params: Vec<Param>,
        /// Optional declared return type annotation.
        return_type: Option<String>,
        /// Body statements comprising the function execution.
        body: Vec<AstNode>,
    },
    /// Anonymous lambda expression: `|x, y| x + y` or `|x| { ... }`.
    Lambda {
        /// Source location of the lambda expression.
        span: Span,
        /// Parameter names captured by the lambda.
        params: Vec<String>,
        /// Expression evaluated when the lambda is called.
        body: Box<AstNode>,
        /// Whether the lambda references the implicit `$it` parameter.
        implicit_it: bool,
    },
    /// Function or method invocation: `foo(a, b, c)` or `foo(a: 1, b: 2)`.
    Call {
        /// Source location of the call expression.
        span: Span,
        /// Expression evaluated to obtain the callable object.
        callee: Box<AstNode>,
        /// Positional and named arguments passed to the call.
        args: Vec<CallArg>,
    },
    /// Variable or symbol identifier reference (e.g., `$x`, `print`).
    Ident {
        /// Source location of the identifier.
        span: Span,
        /// Identifier name text.
        name: String,
    },
    /// 64-bit signed integer literal: `42`, `-100`, `0xFF`.
    Int {
        /// Source location of the literal.
        span: Span,
        /// Parsed integer value.
        value: i64,
    },
    /// 64-bit double-precision floating-point literal: `3.14`, `1.0e-5`.
    Float {
        /// Source location of the literal.
        span: Span,
        /// Parsed floating-point value.
        value: f64,
    },
    /// UTF-8 string literal: `"hello"`, `"""multiline"""`, `r"raw"`.
    Str {
        /// Source location of the literal.
        span: Span,
        /// String contents.
        value: String,
    },
    /// Boolean literal: `true` or `false`.
    Bool {
        /// Source location of the literal.
        span: Span,
        /// Boolean truth value.
        value: bool,
    },
    /// Nil literal (`none`) representing the absence of a value or unit.
    Nil {
        /// Source location of the nil literal.
        span: Span,
    },
    /// Binary operation: `a + b`, `x == y`, `a && b`.
    BinaryOp {
        /// Source location of the operation.
        span: Span,
        /// Left-hand side operand expression.
        left: Box<AstNode>,
        /// Binary operator kind.
        op: BinaryOperator,
        /// Right-hand side operand expression.
        right: Box<AstNode>,
    },
    /// Unary operation: `-x`, `!flag`.
    UnaryOp {
        /// Source location of the operation.
        span: Span,
        /// Unary operator kind.
        op: UnaryOperator,
        /// Operand expression.
        operand: Box<AstNode>,
    },
    /// Pipeline transformation expression: `input |> stage1 |> stage2`.
    Pipeline {
        /// Source location of the pipeline.
        span: Span,
        /// Initial input expression piped into the first stage.
        input: Box<AstNode>,
        /// Ordered sequence of pipeline stages applied to the input.
        stages: Vec<PipelineStage>,
    },
    /// Array literal: `[1, 2, 3]`.
    ArrayLit {
        /// Source location of the literal.
        span: Span,
        /// Array element expressions in order.
        elements: Vec<AstNode>,
    },
    /// Dictionary/map literal: `{ "key": value, "other": 42 }`.
    DictLit {
        /// Source location of the literal.
        span: Span,
        /// Key-value pair expressions.
        entries: Vec<(String, AstNode)>,
    },
    /// Indexing expression: `target[index]`.
    Index {
        /// Source location of the index expression.
        span: Span,
        /// Target collection being indexed.
        target: Box<AstNode>,
        /// Index key or offset expression.
        index: Box<AstNode>,
    },
    /// Field or property access: `obj.field`.
    FieldAccess {
        /// Source location of the field access.
        span: Span,
        /// Target object expression.
        target: Box<AstNode>,
        /// Field name string.
        field: String,
    },
    /// Range expression: `1..10`, `..end`, `start..`.
    Range {
        /// Source location of the range.
        span: Span,
        /// Optional inclusive start expression.
        start: Option<Box<AstNode>>,
        /// Optional exclusive end expression.
        end: Option<Box<AstNode>>,
    },
    /// Lexical block of statements: `{ stmt1; stmt2; expr }`.
    Block {
        /// Source location of the block.
        span: Span,
        /// Ordered statements evaluated within the block.
        statements: Vec<AstNode>,
    },
    /// Type definition: `Point = { x: f64, y: f64 }`.
    TypeDef {
        /// Source location of the type definition.
        span: Span,
        /// Defined type identifier name.
        name: String,
        /// Kind of type (record, sum type, or trait).
        kind: TypeKind,
        /// Body definitions (fields, variants, or methods).
        body: Vec<AstNode>,
    },
    /// Trait implementation block: `Circle is Shape { area() -> f64 { ... } }`.
    ImplDef {
        /// Source location of the impl block.
        span: Span,
        /// Name of the implementing type.
        type_name: String,
        /// Name of the implemented trait.
        trait_name: String,
        /// Method implementation definitions.
        body: Vec<AstNode>,
    },
    /// Dynamic type test expression: `$x is Shape`.
    TypeTest {
        /// Source location of the type test.
        span: Span,
        /// Value expression whose runtime type is being checked.
        value: Box<AstNode>,
        /// Target type name to test against.
        type_name: String,
    },
}

/// Classification of custom type definitions in XCL.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    /// Record type with named fields: `Point = { x: f64; y: f64 }`.
    Record,
    /// Sum type (tagged union) with variants: `Shape = | Circle { r: f64 } | Square { s: f64 }`.
    Sum {
        /// The list of variants comprising this sum type.
        variants: Vec<SumVariant>,
    },
    /// Trait/interface definition: `Printable = { to_string() -> str }`.
    Trait,
}

/// A single variant within a sum type definition.
#[derive(Debug, Clone, PartialEq)]
pub struct SumVariant {
    /// Variant constructor name (e.g., `Circle`, `Square`).
    pub name: String,
    /// Fields contained within this variant.
    pub fields: Vec<TypeField>,
}

/// A field within a record or variant definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeField {
    /// Field identifier name.
    pub name: String,
    /// Type annotation string (e.g., `"f64"`, `"str"`).
    pub ty: String,
    /// Whether the field is declared mutable.
    pub mutable: bool,
}

/// Formal parameter in a function or method definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Parameter identifier name.
    pub name: String,
    /// Optional type annotation.
    pub type_annotation: Option<String>,
    /// Optional default value expression if omitted at the call site.
    pub default_value: Option<Box<AstNode>>,
    /// Source location of the parameter definition.
    pub span: Span,
}

/// Function call argument, supporting positional and named forms.
#[derive(Debug, Clone, PartialEq)]
pub struct CallArg {
    /// Argument value expression.
    pub value: AstNode,
    /// Optional parameter name for named arguments (e.g. `timeout: 5000`).
    pub name: Option<String>,
}

/// Binary operators supported by XCL expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    /// Addition: `+`.
    Add,
    /// Subtraction: `-`.
    Sub,
    /// Multiplication: `*`.
    Mul,
    /// Division: `/`.
    Div,
    /// Modulo / remainder: `%`.
    Mod,
    /// Equality comparison: `==`.
    Eq,
    /// Inequality comparison: `!=`.
    NotEq,
    /// Less than: `<`.
    Lt,
    /// Greater than: `>`.
    Gt,
    /// Less than or equal: `<=`.
    Le,
    /// Greater than or equal: `>=`.
    Ge,
    /// Short-circuiting logical AND: `&&` or `and`.
    And,
    /// Short-circuiting logical OR: `||` or `or`.
    Or,
    /// Null coalescing operator: `??`.
    Coalesce,
    /// Membership test: `in`.
    In,
}

/// Unary operators supported by XCL expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    /// Arithmetic negation: `-x`.
    Neg,
    /// Logical inversion: `!x` or `not x`.
    Not,
}

/// A single step in a pipeline expression.
///
/// In `data |> filter(|x| x > 0) |> sum()`, each method call is a `PipelineStage`.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineStage {
    /// Name of the target function or method to call.
    pub name: String,
    /// Optional additional argument passed after the piped input value.
    pub arg: Option<Box<AstNode>>,
}
