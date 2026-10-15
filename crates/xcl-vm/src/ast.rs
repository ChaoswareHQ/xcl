use crate::span::Span;

#[derive(Debug, Clone)]
pub enum AstNode {
    Assign { span: Span, name: String, value: Box<AstNode>, type_annotation: Option<String> },
    Return { span: Span, value: Option<Box<AstNode>> },
    Break { span: Span },
    Continue { span: Span },
    Throw { span: Span, value: Box<AstNode> },

    If { span: Span, condition: Box<AstNode>, then_branch: Vec<AstNode>, else_branch: Option<Vec<AstNode>> },
    For { span: Span, variable: String, iterable: Box<AstNode>, body: Vec<AstNode> },
    While { span: Span, condition: Box<AstNode>, body: Vec<AstNode> },
    Try { span: Span, body: Vec<AstNode>, catch_var: Option<String>, catch_body: Option<Vec<AstNode>> },

    FunctionDef { span: Span, name: String, params: Vec<Param>, return_type: Option<String>, body: Vec<AstNode> },
    Lambda { span: Span, params: Vec<String>, body: Box<AstNode> },
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