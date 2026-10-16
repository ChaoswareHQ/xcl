//! IR type definitions for the translator.
//!
//! Defines `IrType` — the typed representation used in the intermediate
//! representation. Every IR node carries an `IrType`.
//!
//! Per contract IR3.4: IR is typed. No untyped expressions.

use std::fmt;

/// A type in the intermediate representation.
///
/// IrType is language-agnostic — it contains no Java, Rust, or Python
/// specific names. Target-specific types are resolved via the type map.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IrType {
    /// Unit / void / nil
    Nil,
    /// Boolean
    Bool,
    /// Integer (arbitrary precision in IR, lowered to target int)
    Int,
    /// Floating point
    Float,
    /// String
    Str,
    /// Array of elements of the given type
    Array(Box<IrType>),
    /// Map / dictionary with string keys
    Map(Box<IrType>),
    /// Named record with fields
    Record {
        name: String,
        fields: Vec<(String, IrType)>,
    },
    /// Function type with parameters and return type
    Fn {
        params: Vec<IrType>,
        ret: Box<IrType>,
    },
    /// Optional — may or may not contain a value of the given type
    Optional(Box<IrType>),
    /// Result — Ok(value) or Err(error)
    Result {
        ok: Box<IrType>,
        err: Box<IrType>,
    },
    /// Generic type parameter
    Generic(String),
    /// Any type (top type)
    Any,
}

impl IrType {
    /// Check if this type is a primitive.
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            IrType::Nil | IrType::Bool | IrType::Int | IrType::Float | IrType::Str
        )
    }

    /// Get the display name for this type.
    pub fn name(&self) -> String {
        match self {
            IrType::Nil => "nil".to_string(),
            IrType::Bool => "bool".to_string(),
            IrType::Int => "int".to_string(),
            IrType::Float => "float".to_string(),
            IrType::Str => "str".to_string(),
            IrType::Array(ty) => format!("array[{}]", ty.name()),
            IrType::Map(ty) => format!("map[{}]", ty.name()),
            IrType::Record { name, .. } => name.clone(),
            IrType::Fn { params, ret } => {
                let param_names: Vec<String> = params.iter().map(|t| t.name()).collect();
                format!("fn({}) -> {}", param_names.join(", "), ret.name())
            }
            IrType::Optional(ty) => format!("option[{}]", ty.name()),
            IrType::Result { ok, err } => {
                format!("result[{}, {}]", ok.name(), err.name())
            }
            IrType::Generic(name) => name.clone(),
            IrType::Any => "any".to_string(),
        }
    }
}

impl fmt::Display for IrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
