//! IR node definitions for the translator.
//!
//! Defines `Ir`, `IrNode`, `IrLiteral`, `SourceSpan`, and `Origin`.
//!
//! Per contract IR3.1: IR is language-agnostic. No Java/Rust/Python names.
//! Per contract IR3.2: IR is lossless enough to reconstruct XCL. Every node carries a SourceSpan.
//! Per contract IR3.5: IR carries origin metadata.

use super::types::IrType;
use super::ops::Hint;
use crate::translate::version::LangSpec;

/// A source code span, used for error reporting and round-trip fidelity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    /// Start line (1-based)
    pub start_line: usize,
    /// Start column (1-based)
    pub start_col: usize,
    /// End line (1-based)
    pub end_line: usize,
    /// End column (1-based)
    pub end_col: usize,
}

impl SourceSpan {
    /// Create a new span.
    pub fn new(start_line: usize, start_col: usize, end_line: usize, end_col: usize) -> Self {
        Self {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    /// Create a point span (single position).
    pub fn point(line: usize, col: usize) -> Self {
        Self {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
        }
    }

    /// Create a dummy span (when source location is unknown).
    pub fn dummy() -> Self {
        Self::point(0, 0)
    }

    /// Merge two spans into a single span covering both.
    pub fn merge(&self, other: &SourceSpan) -> Self {
        Self {
            start_line: self.start_line.min(other.start_line),
            start_col: if self.start_line == other.start_line {
                self.start_col.min(other.start_col)
            } else {
                self.start_col.min(other.start_col)
            },
            end_line: self.end_line.max(other.end_line),
            end_col: if self.end_line == other.end_line {
                self.end_col.max(other.end_col)
            } else {
                self.end_col.max(other.end_col)
            },
        }
    }
}

/// Metadata about the origin of an IR node.
///
/// Per contract IR3.5: IR carries origin metadata — which language/version produced it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Origin {
    /// The language and version that produced this IR node.
    pub lang: LangSpec,
    /// Optional source file path.
    pub file: Option<String>,
}

impl Origin {
    /// Create a new origin.
    pub fn new(lang: LangSpec) -> Self {
        Self { lang, file: None }
    }

    /// Create an origin with a file path.
    pub fn with_file(lang: LangSpec, file: String) -> Self {
        Self {
            lang,
            file: Some(file),
        }
    }
}

/// A literal value in the IR.
#[derive(Clone, Debug, PartialEq)]
pub enum IrLiteral {
    /// `true` or `false`
    Bool(bool),
    /// Integer literal
    Int(i64),
    /// Floating point literal
    Float(f64),
    /// String literal
    Str(String),
    /// Nil / null / None
    Nil,
}

/// A node in the intermediate representation.
///
/// Per contract IR3.1: IR is language-agnostic. No Java/Rust/Python names in this enum.
#[derive(Clone, Debug, PartialEq)]
pub enum IrNode {
    /// A literal value
    Literal(IrLiteral),
    /// A variable reference
    Var(String),
    /// A function call: callee(args...)
    Call {
        callee: Box<Ir>,
        args: Vec<Ir>,
    },
    /// A binary operation: lhs op rhs
    BinOp {
        op: super::ops::IrOp,
        lhs: Box<Ir>,
        rhs: Box<Ir>,
    },
    /// A unary operation
    UnaryOp {
        op: super::ops::IrOp,
        operand: Box<Ir>,
    },
    /// Conditional: if cond { then } [else { else_ }]
    If {
        cond: Box<Ir>,
        then: Box<Ir>,
        else_: Option<Box<Ir>>,
    },
    /// Loop: while cond { body } or for-in
    Loop {
        body: Box<Ir>,
        cond: Option<Box<Ir>>,
    },
    /// A block of statements
    Block(Vec<Ir>),
    /// Return from function
    Return(Option<Box<Ir>>),
    /// Variable declaration: let name = value
    Let {
        name: String,
        ty: IrType,
        value: Box<Ir>,
        mutable: bool,
    },
    /// Record definition: struct Name { fields }
    Record {
        name: String,
        fields: Vec<(String, IrType)>,
    },
    /// Record construction: Name { field: value, ... }
    RecordLit {
        name: String,
        fields: Vec<(String, Ir)>,
    },
    /// Function definition
    Fn {
        name: String,
        params: Vec<(String, IrType)>,
        ret: IrType,
        body: Box<Ir>,
    },
    /// Lambda / closure
    Lambda {
        params: Vec<(String, IrType)>,
        ret: IrType,
        body: Box<Ir>,
    },
    /// Defer block
    Defer(Box<Ir>),
    /// Field access: obj.field
    FieldAccess {
        obj: Box<Ir>,
        field: String,
    },
    /// Indexing: arr[index]
    Index {
        collection: Box<Ir>,
        index: Box<Ir>,
    },
    /// Slicing: arr[start:end]
    Slice {
        collection: Box<Ir>,
        start: Box<Ir>,
        end: Box<Ir>,
    },
    /// Type cast
    Cast {
        value: Box<Ir>,
        to_type: IrType,
    },
    /// Break from loop
    Break,
    /// Continue loop
    Continue,
    /// Match expression
    Match {
        value: Box<Ir>,
        arms: Vec<(String, Vec<String>, Box<Ir>)>,
    },
}

/// An IR expression with type, span, origin, and hints.
///
/// This is the core type of the translator. Every expression, statement,
/// and declaration is represented as an `Ir`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ir {
    /// The actual node content.
    pub node: IrNode,
    /// The type of this expression.
    pub ty: IrType,
    /// Source code span for error reporting.
    pub span: SourceSpan,
    /// Origin metadata — which language/version produced this.
    pub origin: Origin,
    /// Semantic hints (Record, Sealed, Defer, Optional, etc.).
    pub hints: Vec<Hint>,
}

impl Ir {
    /// Create a new IR node.
    pub fn new(
        node: IrNode,
        ty: IrType,
        span: SourceSpan,
        origin: Origin,
        hints: Vec<Hint>,
    ) -> Self {
        Self {
            node,
            ty,
            span,
            origin,
            hints,
        }
    }

    /// Create a simple IR node with dummy span and no hints.
    pub fn simple(node: IrNode, ty: IrType, origin: Origin) -> Self {
        Self {
            node,
            ty,
            span: SourceSpan::dummy(),
            origin,
            hints: Vec::new(),
        }
    }

    /// Create a literal IR node.
    pub fn literal(lit: IrLiteral, origin: Origin) -> Self {
        let ty = match &lit {
            IrLiteral::Bool(_) => IrType::Bool,
            IrLiteral::Int(_) => IrType::Int,
            IrLiteral::Float(_) => IrType::Float,
            IrLiteral::Str(_) => IrType::Str,
            IrLiteral::Nil => IrType::Nil,
        };
        Self::simple(IrNode::Literal(lit), ty, origin)
    }

    /// Create a variable reference IR node.
    pub fn var(name: String, origin: Origin) -> Self {
        Self::simple(IrNode::Var(name), IrType::Any, origin)
    }

    /// Add a hint to this IR node.
    pub fn with_hint(mut self, hint: Hint) -> Self {
        self.hints.push(hint);
        self
    }

    /// Check if this node has a specific hint.
    pub fn has_hint(&self, hint: &Hint) -> bool {
        self.hints.contains(hint)
    }
}
