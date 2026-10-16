#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::span::Span;

/// Abstract syntax tree node representing a parsed XCL language construct.
#[derive(Debug, Clone)]
pub enum AstNode {
    /// Variable assignment: `x = 42` or `x: i64 = 42`.
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
        /// Optional return value expression.
        value: Option<Box<AstNode>>,
    },
    /// Break statement to exit a loop.
    Break {
        /// Source location of the break statement.
        span: Span,
    },
    /// Continue statement to skip to the next loop iteration.
    Continue {
        /// Source location of the continue statement.
        span: Span,
    },
    /// Conditional: `if cond { ... } else { ... }`.
    If {
        /// Source location of the if statement.
        span: Span,
        /// Condition expression that determines which branch executes.
        condition: Box<AstNode>,
        /// Statements executed when the condition is true.
        then_branch: Vec<AstNode>,
        /// Optional statements executed when the condition is false.
        else_branch: Option<Vec<AstNode>>,
    },
    /// For loop: `for item in iterable { ... }`.
    For {
        /// Source location of the for loop.
        span: Span,
        /// Loop variable name.
        variable: String,
        /// Expression producing the iterable collection.
        iterable: Box<AstNode>,
        /// Body statements executed for each iteration.
        body: Vec<AstNode>,
    },
    /// While loop: `while cond { ... }`.
    While {
        /// Source location of the while loop.
        span: Span,
        /// Condition expression checked before each iteration.
        condition: Box<AstNode>,
        /// Body statements executed while the condition is true.
        body: Vec<AstNode>,
    },
    /// Function definition: `fn name(params) -> type { ... }`.
    FunctionDef {
        /// Source location of the function definition.
        span: Span,
        /// Function name.
        name: String,
        /// Parameter list.
        params: Vec<Param>,
        /// Optional return type annotation.
        return_type: Option<String>,
        /// Function body statements.
        body: Vec<AstNode>,
    },
    /// Lambda expression: `|x, y| x + y`.
    Lambda {
        /// Source location of the lambda.
        span: Span,
        /// Parameter names.
        params: Vec<String>,
        /// Lambda body expression.
        body: Box<AstNode>,
        /// Whether the lambda has an implicit `$it` parameter.
        implicit_it: bool,
    },
    /// Function call: `foo(a, b, c)` or `foo(a: 1, b: 2)`.
    Call {
        /// Source location of the call expression.
        span: Span,
        /// Expression being called (function, method, etc.).
        callee: Box<AstNode>,
        /// Positional and/or named arguments.
        args: Vec<CallArg>,
    },
    /// Variable or function identifier reference.
    Ident {
        /// Source location of the identifier.
        span: Span,
        /// Identifier name.
        name: String,
    },
    /// Integer literal: `42`, `-100`.
    Int {
        /// Source location of the literal.
        span: Span,
        /// Integer value.
        value: i64,
    },
    /// Floating-point literal: `3.14`, `1.0e-5`.
    Float {
        /// Source location of the literal.
        span: Span,
        /// Floating-point value.
        value: f64,
    },
    /// String literal: `"hello"`, `"""multiline"""`.
    Str {
        /// Source location of the literal.
        span: Span,
        /// String value.
        value: String,
    },
    /// Boolean literal: `true`, `false`.
    Bool {
        /// Source location of the literal.
        span: Span,
        /// Boolean value.
        value: bool,
    },
    /// Nil literal representing absence of a value.
    Nil {
        /// Source location of the nil literal.
        span: Span,
    },
    /// Binary operation: `a + b`, `x == y`, `a && b`.
    BinaryOp {
        /// Source location of the operation.
        span: Span,
        /// Left-hand side operand.
        left: Box<AstNode>,
        /// Operator (add, sub, eq, and, etc.).
        op: BinaryOperator,
        /// Right-hand side operand.
        right: Box<AstNode>,
    },
    /// Unary operation: `-x`, `!flag`.
    UnaryOp {
        /// Source location of the operation.
        span: Span,
        /// Operator (neg, not).
        op: UnaryOperator,
        /// Operand expression.
        operand: Box<AstNode>,
    },
    /// Pipeline expression: `input | stage1 | stage2`.
    Pipeline {
        /// Source location of the pipeline.
        span: Span,
        /// Initial input expression piped through stages.
        input: Box<AstNode>,
        /// Sequence of pipeline stages applied to the input.
        stages: Vec<PipelineStage>,
    },
    /// Array literal: `[1, 2, 3]`.
    ArrayLit {
        /// Source location of the literal.
        span: Span,
        /// Array element expressions.
        elements: Vec<AstNode>,
    },
    /// Dictionary/map literal: `{ key: value }`.
    DictLit {
        /// Source location of the literal.
        span: Span,
        /// Key-value pairs.
        entries: Vec<(String, AstNode)>,
    },
    /// Indexing expression: `arr[i]`.
    Index {
        /// Source location of the index expression.
        span: Span,
        /// Target collection being indexed.
        target: Box<AstNode>,
        /// Index expression.
        index: Box<AstNode>,
    },
    /// Field/property access: `obj.field`.
    FieldAccess {
        /// Source location of the field access.
        span: Span,
        /// Target object expression.
        target: Box<AstNode>,
        /// Field name.
        field: String,
    },
    /// Range expression: `1..10`, `..end`, `start..`.
    Range {
        /// Source location of the range.
        span: Span,
        /// Optional start expression (inclusive).
        start: Option<Box<AstNode>>,
        /// Optional end expression (exclusive).
        end: Option<Box<AstNode>>,
    },
    /// Block of statements: `{ stmt1; stmt2; stmt3 }`.
    Block {
        /// Source location of the block.
        span: Span,
        /// Ordered sequence of statements.
        statements: Vec<AstNode>,
    },
    /// Type definition: `Point = { x: f64; y: f64; dist() -> f64 { ... } }`.
    TypeDef {
        /// Source location of the type definition.
        span: Span,
        /// Type name.
        name: String,
        /// Kind of type (record, sum, trait).
        kind: TypeKind,
        /// Body containing fields and methods.
        body: Vec<AstNode>,
    },
    /// Impl block: `Circle is Shape { area() -> f64 { ... } }`.
    ImplDef {
        /// Source location of the impl block.
        span: Span,
        /// Name of the type being implemented.
        type_name: String,
        /// Name of the trait being implemented.
        trait_name: String,
        /// Method implementations.
        body: Vec<AstNode>,
    },
    /// Type test expression: `$x is Shape`.
    TypeTest {
        /// Source location of the type test.
        span: Span,
        /// Value expression being tested.
        value: Box<AstNode>,
        /// Name of the type to test against.
        type_name: String,
    },
}

/// Kind of type definition in XCL.
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// Record type with named fields: `Point = { x: f64; y: f64 }`.
    Record,
    /// Sum type with multiple variants: `Shape = | Circle { r: f64 } | Square { s: f64 }`.
    Sum {
        /// The list of variants in the sum type.
        variants: Vec<SumVariant>,
    },
    /// Trait/interface definition: `Printable = { to_string() -> str }`.
    Trait,
}

/// A variant in a sum type definition.
#[derive(Debug, Clone)]
pub struct SumVariant {
    /// Name of the variant (e.g., `Circle`, `Square`).
    pub name: String,
    /// Fields contained in this variant.
    pub fields: Vec<TypeField>,
}

/// A field in a type definition.
#[derive(Debug, Clone)]
pub struct TypeField {
    /// Field name.
    pub name: String,
    /// Field type annotation (e.g., `f64`, `str`).
    pub ty: String,
    /// Whether the field is mutable.
    pub mutable: bool,
}

/// Function parameter definition.
#[derive(Debug, Clone)]
pub struct Param {
    /// Parameter name.
    pub name: String,
    /// Optional type annotation.
    pub type_annotation: Option<String>,
    /// Optional default value expression.
    pub default_value: Option<Box<AstNode>>,
    /// Source location of the parameter.
    pub span: Span,
}

/// Function call argument (positional or named).
#[derive(Debug, Clone)]
pub struct CallArg {
    /// Argument value expression.
    pub value: AstNode,
    /// Optional parameter name for named arguments.
    pub name: Option<String>,
}

/// Binary operators supported by XCL.
#[derive(Debug, Clone, Copy)]
pub enum BinaryOperator {
    /// Addition: `+`.
    Add,
    /// Subtraction: `-`.
    Sub,
    /// Multiplication: `*`.
    Mul,
    /// Division: `/`.
    Div,
    /// Modulo: `%`.
    Mod,
    /// Equality: `==`.
    Eq,
    /// Inequality: `!=`.
    NotEq,
    /// Less than: `<`.
    Lt,
    /// Greater than: `>`.
    Gt,
    /// Less than or equal: `<=`.
    Le,
    /// Greater than or equal: `>=`.
    Ge,
    /// Logical AND: `&&`.
    And,
    /// Logical OR: `||`.
    Or,
    /// Null coalescing: `??`.
    Coalesce,
    /// Membership test: `in`.
    In,
}

/// Unary operators supported by XCL.
#[derive(Debug, Clone, Copy)]
pub enum UnaryOperator {
    /// Negation: `-x`.
    Neg,
    /// Logical NOT: `!x`.
    Not,
}

/// A single stage in a pipeline expression.
#[derive(Debug, Clone)]
pub struct PipelineStage {
    /// Name of the function/method to call.
    pub name: String,
    /// Optional additional argument (input is piped as the first argument).
    pub arg: Option<Box<AstNode>>,
}