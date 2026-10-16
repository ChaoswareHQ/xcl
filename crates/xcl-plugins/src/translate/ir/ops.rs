//! IR operations, features, and hints.
//!
//! - `IrOp` — operations in the IR (arithmetic, comparison, logic, etc.)
//! - `Feature` — language features that backends may or may not support
//! - `Hint` — semantic hints attached to IR nodes

use std::fmt;

/// An operation in the intermediate representation.
///
/// IrOp is language-agnostic — no target-specific operation names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IrOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logic
    And,
    Or,
    Not,

    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    // Type operations
    Cast,
    Len,
    Index,
    Slice,

    // String operations
    Concat,

    // Memory
    FieldAccess,
    Call,
}

impl fmt::Display for IrOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrOp::Add => write!(f, "+"),
            IrOp::Sub => write!(f, "-"),
            IrOp::Mul => write!(f, "*"),
            IrOp::Div => write!(f, "/"),
            IrOp::Rem => write!(f, "%"),
            IrOp::Neg => write!(f, "-"),
            IrOp::Eq => write!(f, "=="),
            IrOp::Ne => write!(f, "!="),
            IrOp::Lt => write!(f, "<"),
            IrOp::Le => write!(f, "<="),
            IrOp::Gt => write!(f, ">"),
            IrOp::Ge => write!(f, ">="),
            IrOp::And => write!(f, "&&"),
            IrOp::Or => write!(f, "||"),
            IrOp::Not => write!(f, "!"),
            IrOp::BitAnd => write!(f, "&"),
            IrOp::BitOr => write!(f, "|"),
            IrOp::BitXor => write!(f, "^"),
            IrOp::Shl => write!(f, "<<"),
            IrOp::Shr => write!(f, ">>"),
            IrOp::Cast => write!(f, "cast"),
            IrOp::Len => write!(f, "len"),
            IrOp::Index => write!(f, "index"),
            IrOp::Slice => write!(f, "slice"),
            IrOp::Concat => write!(f, "concat"),
            IrOp::FieldAccess => write!(f, "."),
            IrOp::Call => write!(f, "call"),
        }
    }
}

/// Language features that backends declare support for.
///
/// Per contract BE5.3: a backend must declare `supports(Feature) -> bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Records / data classes / structs
    Records,
    /// Pattern matching
    PatternMatching,
    /// Generics
    Generics,
    /// Null safety / option types
    NullSafety,
    /// Defer / finally blocks
    Defer,
    /// Async/await
    Async,
    /// Closures / lambdas
    Closures,
    /// Traits / interfaces
    Traits,
    /// Enums / sum types
    SumTypes,
    /// String interpolation
    StringInterpolation,
    /// Multi-line strings
    MultiLineStrings,
    /// Raw strings
    RawStrings,
    /// Type inference
    TypeInference,
    /// Operator overloading
    OperatorOverloading,
    /// Match expressions
    MatchExpressions,
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Feature::Records => write!(f, "records"),
            Feature::PatternMatching => write!(f, "pattern_matching"),
            Feature::Generics => write!(f, "generics"),
            Feature::NullSafety => write!(f, "null_safety"),
            Feature::Defer => write!(f, "defer"),
            Feature::Async => write!(f, "async"),
            Feature::Closures => write!(f, "closures"),
            Feature::Traits => write!(f, "traits"),
            Feature::SumTypes => write!(f, "sum_types"),
            Feature::StringInterpolation => write!(f, "string_interpolation"),
            Feature::MultiLineStrings => write!(f, "multiline_strings"),
            Feature::RawStrings => write!(f, "raw_strings"),
            Feature::TypeInference => write!(f, "type_inference"),
            Feature::OperatorOverloading => write!(f, "operator_overloading"),
            Feature::MatchExpressions => write!(f, "match_expressions"),
        }
    }
}

/// Semantic hints attached to IR nodes.
///
/// Per contract IR3.6: IR carries hints for Record, Sealed, Defer, Optional.
/// Frontends emit hints; backends use them to generate idiomatic code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hint {
    /// This node represents a record/data class (immutable, value-based)
    Record,
    /// This enum is sealed — all variants are known
    Sealed,
    /// This is a defer/finally block
    Defer,
    /// This value is optional
    Optional,
    /// This function is async
    Async,
    /// This is a const/immutable value
    Const,
    /// This is a mutable reference
    Mutable,
    /// This function is pure (no side effects)
    Pure,
    /// This type is generic
    Generic,
}

impl fmt::Display for Hint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Hint::Record => write!(f, "record"),
            Hint::Sealed => write!(f, "sealed"),
            Hint::Defer => write!(f, "defer"),
            Hint::Optional => write!(f, "optional"),
            Hint::Async => write!(f, "async"),
            Hint::Const => write!(f, "const"),
            Hint::Mutable => write!(f, "mutable"),
            Hint::Pure => write!(f, "pure"),
            Hint::Generic => write!(f, "generic"),
        }
    }
}
