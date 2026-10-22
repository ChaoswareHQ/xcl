//! Function, type, trait and impl descriptors.

use super::*;

/// Executable function descriptor containing bytecode and closure environment.
#[derive(Clone, Debug)]
pub struct FuncValue {
    /// Function identifier name.
    pub name: Arc<str>,
    /// Compiled bytecode chunk for user-defined functions (`None` for native functions).
    pub bytecode: Option<Arc<crate::bytecode::Bytecode>>,
    /// Formal parameter names.
    ///
    /// Shared so cloning a function (once per closure creation) is a refcount
    /// bump rather than a deep copy.
    pub params: Arc<[String]>,
    /// Captured variable names for closures (layout metadata only).
    pub closure_captures: Arc<[String]>,
    /// Captured environment values, in `closure_captures` order.
    ///
    /// Positional: the names above are never consulted at run time, so the
    /// environment does not repeat them.
    pub closure_env: smallvec::SmallVec<[Value; 4]>,
    /// True if the function body references `$1..$N`, `$#`, `$@` or `$*`.
    pub uses_argv: bool,
}

impl PartialEq for FuncValue {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

// — Native function registry types —

/// Static metadata describing a registered custom type.
pub struct TypeDescriptor {
    /// Type identifier name.
    pub name: &'static str,
    /// Domain owning this type, or `None` if globally scoped.
    pub domain: Option<&'static str>,
    /// Kind of type ([`TypeKind`]).
    pub kind: TypeKind,
    /// Field specifications.
    pub fields: &'static [TypeField],
    /// Variant specifications for sum types.
    pub variants: &'static [TypeVariant],
    /// Documentation summary string.
    pub summary: Option<&'static str>,
    /// Parent type name for inheritance via composition (Super support).
    /// When set, method resolution falls back to this parent type if a method
    /// is not found on the current type.
    pub parent_type: Option<&'static str>,
    /// Capabilities this type provides, declared by the registering macro
    /// (`#[xcl(caps = "Len, Contains")]`). A declared capability is a promise the
    /// type's methods must keep.
    pub caps: crate::runtime::primitives::CapSet,
}

/// Classification of custom types in the type registry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeKind {
    /// Opaque handle type.
    Opaque,
    /// Record struct with fields.
    Record,
    /// Tagged sum type / enum.
    Sum,
    /// Type alias.
    Alias,
}

/// Field descriptor in a custom type definition.
pub struct TypeField {
    /// Field identifier name.
    pub name: &'static str,
    /// Type name string.
    pub ty: &'static str,
    /// Whether the field is optional.
    pub optional: bool,
}

/// Variant descriptor in a sum type definition.
pub struct TypeVariant {
    /// Variant identifier name.
    pub name: &'static str,
    /// Fields contained within this variant.
    pub fields: &'static [TypeField],
}

// — Impl descriptor types —

/// Static metadata describing a registered trait interface.
pub struct TraitDescriptor {
    /// Trait identifier name.
    pub name: &'static str,
    /// Domain namespace, or `None` for global traits.
    pub domain: Option<&'static str>,
    /// List of method signatures required by this trait.
    pub methods: &'static [TraitMethod],
    /// Documentation summary string.
    pub summary: Option<&'static str>,
}

/// Method signature in a trait definition.
pub struct TraitMethod {
    /// Method identifier name.
    pub name: &'static str,
    /// Argument signature string (e.g., `"key: str; value: str"`).
    pub args: Option<&'static str>,
    /// Return type signature string (e.g., `"Result"`).
    pub ret: Option<&'static str>,
    /// Method documentation summary.
    pub summary: Option<&'static str>,
}

/// Metadata describing a trait implementation block.
pub struct ImplDescriptor {
    /// Name of the implementing type.
    pub type_name: &'static str,
    /// Name of the implemented trait.
    pub trait_name: &'static str,
    /// Domain namespace.
    pub domain: Option<&'static str>,
    /// List of implemented methods.
    pub methods: &'static [ImplMethod],
}

/// Method descriptor within a trait implementation block.
pub struct ImplMethod {
    /// Method name.
    pub name: &'static str,
    /// Argument signature string.
    pub args: Option<&'static str>,
    /// Return type signature string.
    pub ret: Option<&'static str>,
    /// Method documentation summary.
    pub summary: Option<&'static str>,
}

/// Core trait for any Rust type that can be serialized to/from XCL `Value::Map`.
///
/// Types that derive `#[derive(Xcl)]` with `kind = "Record"` should implement this trait
/// to enable `&self` deserialization and `Self` serialization in `#[xcl_impl]` inherent impls.
pub trait XclType: Sized {
    /// Deserialize an XCL `Value::Map` into this Rust struct.
    fn from_value(v: Value) -> Self;

    /// Serialize this Rust struct into an XCL `Value::Map`.
    fn to_value(&self) -> Value;
}
