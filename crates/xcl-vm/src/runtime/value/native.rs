//! Native function registration: entries, kinds and dispatch types.

use super::*;

/// Static descriptor for a registered native function.
pub struct NativeEntry {
    /// Domain name (e.g., `"fs"`, `"math"`), or `None` for global functions.
    pub domain: Option<&'static str>,
    /// Impl type name this function belongs to (e.g., `"CloudflareProvider"`), or `None` for generic functions.
    pub impl_type: Option<&'static str>,
    /// Trait method name this function overrides (e.g., `"bucket_config"`), or `None` if not overriding.
    pub override_method: Option<&'static str>,
    /// Method identifier name.
    pub method: &'static str,
    /// Function execution style ([`NativeKind::Pure`] or [`NativeKind::Ctx`]).
    pub kind: NativeKind,
    /// Function pointer address.
    pub ptr: RawPtr,
    /// Human-readable argument signature.
    pub args: Option<&'static str>,
    /// Return type signature string.
    pub ret: Option<&'static str>,
    /// Named parameter specifications.
    pub named: &'static [NamedParam],
    /// Documentation summary string.
    pub summary: Option<&'static str>,
    /// Description pairs for each argument: `(name, description)`.
    pub arg_desc: &'static [(&'static str, &'static str)],
    /// If true, this method is eligible for auto-import: bare identifiers like `map` will
    /// resolve to `collections.map` when the domain is auto-import enabled.
    /// Domains like `https` should set this to false to avoid conflicts (e.g., `get` is too common).
    pub auto_import: bool,
    /// Capabilities of the function value itself, declared by the registering macro
    /// (`#[xcl_fn(flags = "PURE | DETERMINISTIC")]`). Empty is always safe.
    pub caps: crate::runtime::primitives::CapSet,
}

/// A wrapper around a raw function pointer that implements [`Sync`] and [`Send`].
#[derive(Debug, Clone, Copy)]
pub struct RawPtr(pub *const ());

unsafe impl Sync for RawPtr {}
unsafe impl Send for RawPtr {}

/// Descriptor for a function parameter that accepts named arguments with a default value.
pub struct NamedParam {
    /// Parameter identifier name.
    pub name: &'static str,
    /// Type name string.
    pub ty: &'static str,
    /// Default value representation as code.
    pub default: &'static str,
}

/// Calling convention kind for native functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeKind {
    /// Pure function receiving an argument slice: `fn(&[Value]) -> Value`.
    Pure,
    /// Context-aware function receiving mutable [`DomainCtx`]: `fn(&mut DomainCtx) -> Value`.
    Ctx,
}

/// Function pointer type for pure native functions.
pub type BuiltinFn = fn(&[Value]) -> Value;
/// Function pointer type for context-aware native functions.
pub type BuiltinCtxFn = fn(&mut DomainCtx<'_>) -> Value;

/// Function implementation wrapper storing either a pure or context-aware function pointer.
#[derive(Clone, Copy)]
pub enum BuiltinImpl {
    /// Pure function pointer.
    Pure(BuiltinFn),
    /// Context-aware function pointer.
    Ctx(BuiltinCtxFn),
}

impl BuiltinImpl {
    /// Executes the underlying native function using the provided VM and arguments.
    #[inline]
    pub fn call(&self, vm: &mut crate::runtime::vm::VM, args: &[Value]) -> Value {
        match self {
            BuiltinImpl::Pure(f) => f(args),
            BuiltinImpl::Ctx(f) => {
                let mut ctx = DomainCtx { vm, args };
                f(&mut ctx)
            }
        }
    }
}

// — Type descriptor types —
