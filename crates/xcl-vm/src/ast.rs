use crate::span::Span;

#[derive(Debug, Clone)]
pub enum AstNode {
    Assign { span: Span, name: String, value: Box<AstNode>, type_annotation: Option<String> },
    Return { span: Span, value: Option<Box<AstNode>> },
    Break { span: Span },
    Continue { span: Span },

    If { span: Span, condition: Box<AstNode>, then_branch: Vec<AstNode>, else_branch: Option<Vec<AstNode>> },
    For { span: Span, variable: String, iterable: Box<AstNode>, body: Vec<AstNode> },
    While { span: Span, condition: Box<AstNode>, body: Vec<AstNode> },

    FunctionDef { span: Span, name: String, params: Vec<Param>, return_type: Option<String>, body: Vec<AstNode> },
    Lambda { span: Span, params: Vec<String>, body: Box<AstNode>, implicit_it: bool },
    Call { span: Span, callee: Box<AstNode>, args: Vec<CallArg> },

    Ident { span: Span, name: String },
    Int { span: Span, value: i64 },
    Float { span: Span, value: f64 },
    Str { span: Span, value: String },
    Bool { span: Span, value: bool },
    Nil { span: Span },

    BinaryOp { span: Span, left: Box<AstNode>, op: BinaryOperator, right: Box<AstNode> },
    UnaryOp { span: Span, op: UnaryOperator, operand: Box<AstNode> },

    Pipeline { span: Span, input: Box<AstNode>, stages: Vec<PipelineStage> },
    ArrayLit { span: Span, elements: Vec<AstNode> },
    DictLit { span: Span, entries: Vec<(String, AstNode)> },
    Index { span: Span, target: Box<AstNode>, index: Box<AstNode> },
    FieldAccess { span: Span, target: Box<AstNode>, field: String },
    Range { span: Span, start: Option<Box<AstNode>>, end: Option<Box<AstNode>> },
    Block { span: Span, statements: Vec<AstNode> },

    /// Type definition: `Point = { x: f64; y: f64; dist() -> f64 { ... } }`
    TypeDef { span: Span, name: String, kind: TypeKind, body: Vec<AstNode> },
    /// Impl block: `Circle is Shape { area() -> f64 { ... } }`
    ImplDef { span: Span, type_name: String, trait_name: String, body: Vec<AstNode> },
    /// Type test expression: `$x is Shape`
    TypeTest { span: Span, value: Box<AstNode>, type_name: String },
}

/// Kind of type definition
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// Record: `Point = { x: f64; y: f64 }`
    Record,
    /// Sum type: `Shape = | Circle { r: f64 } | Square { s: f64 }`
    Sum { variants: Vec<SumVariant> },
    /// Trait: `Printable = { to_string() -> str }`
    Trait,
}

/// A variant in a sum type definition
#[derive(Debug, Clone)]
pub struct SumVariant {
    pub name: String,
    pub fields: Vec<TypeField>,
}

/// A field in a type definition
#[derive(Debug, Clone)]
pub struct TypeField {
    pub name: String,
    pub ty: String,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_annotation: Option<String>,
    pub default_value: Option<Box<AstNode>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub value: AstNode,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOperator {
    Add, Sub, Mul, Div, Mod,
    Eq, NotEq, Lt, Gt, Le, Ge,
    And, Or, Coalesce, In,
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOperator { Neg, Not }

#[derive(Debug, Clone)]
pub struct PipelineStage { pub name: String, pub arg: Option<Box<AstNode>> }