//! Primitive value types and the capabilities the VM guarantees for them.
//!
//! XCL's runtime has a **closed** set of value representations. Every script
//! value is one of the [`PrimKind`]s below, optionally refined by a header
//! subtype (`range`, `result`, `error`, `handle` are maps carrying a
//! [`SUBTYPE_*`](crate::runtime::value::SUBTYPE_RANGE) tag).
//!
//! This table is the single source of truth for three questions that used to be
//! answered by three separate hardcoded lists:
//!
//! * *what is this value's type called?* — [`primitive_of_value`]
//! * *is this value of type `T`?* — [`primitive_of_name`] + [`Primitive::matches`]
//! * *what can I do with it?* — [`Primitive::has`] + [`Cap`]
//!
//! # Aliases
//!
//! The sized names are **aliases**, not separate types: the VM has exactly one
//! integer representation and one float representation, so a value carries no
//! width. `i8`…`u64` all resolve to `int`, and `f32`/`f64` resolve to `float`.
//! Declaring a capability on an alias therefore declares it on the primitive.
//!
//! # Capabilities
//!
//! A [`Cap`] is a promise about an operation the VM already implements, so the
//! set is deliberately smaller than Rust's trait zoo — it lists only what an
//! interpreter can honestly claim:
//!
//! | Cap | Means | Not promised |
//! |---|---|---|
//! | `Numeric` | `+ - * / %` are defined | a ring/field (int division truncates) |
//! | `Eq` | `==` / `!=` are defined | reflexivity for IEEE-754 `NaN` |
//! | `Ord` | `< <= > >=` are defined | totality for IEEE-754 `NaN` |
//! | `Hash` | usable as a map key | any particular hash function |
//! | `Display` | prints as itself | round-tripping |
//! | `Index` | `target[i]` is defined | bounds errors (out of range is `none`) |
//! | `Iterable` | usable in a `for` header | multi-pass iteration |
//! | `Copy` | no shared mutable identity | deep copy semantics |
//!
//! Arrays and maps are deliberately **not** `Copy`: `OP_ARRAY_PUSH` and index
//! assignment mutate them in place, so aliasing a container is observable.

use crate::runtime::value::{SUBTYPE_ERROR, SUBTYPE_HANDLE, SUBTYPE_RANGE, SUBTYPE_RESULT, Value};

/// Runtime representation family of a primitive value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimKind {
    /// 48-bit tagged integer.
    Int,
    /// 64-bit IEEE-754 float.
    Float,
    /// `true` / `false`.
    Bool,
    /// The single `none` value.
    None,
    /// Heap string.
    Str,
    /// Heap array.
    Array,
    /// Heap map, optionally refined by a header subtype.
    Map,
    /// Heap function or closure.
    Func,
}

/// An operation the VM implements for a type, or a property of a function.
///
/// Type capabilities (`Numeric` … `Disposable`) answer `value is Cap`. Function
/// capabilities (`Pure`, `Deterministic`, `Diverges`) describe a function value
/// and answer `fn is Cap`; they are registered by the macros that declare the
/// function (`#[xcl_fn(flags = "PURE")]`, `#[opcode(flags = "PURE")]`).
///
/// Variants after `Copy` are part of the vocabulary but no primitive declares
/// them yet: their operations (`len`, `repr`, index assignment, `in`, `as`,
/// `defer`-close) are still to be implemented, and a capability is only claimed
/// once the VM can actually perform it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Cap {
    /// `+ - * / %` are defined.
    Numeric,
    /// `==` / `!=` are defined.
    Eq,
    /// `< <= > >=` are defined.
    Ord,
    /// Usable as a map key.
    Hash,
    /// Prints as itself.
    Display,
    /// `target[i]` is defined.
    Index,
    /// Usable in a `for` header.
    Iterable,
    /// No shared mutable identity; safe to duplicate.
    Copy,

    /// A printed form that quotes strings and shows structure.
    Repr,
    /// `len value` is defined.
    Len,
    /// `target[i] = v` is defined.
    IndexMut,
    /// `needle in target` is defined.
    Contains,
    /// `value as T` is defined for at least one `T`.
    Cast,
    /// `defer value` closes it on scope exit.
    Disposable,

    /// Calling has no observable side effects.
    Pure,
    /// Same inputs produce the same output.
    Deterministic,
    /// Operands may be reordered without changing the result.
    Commutative,
    /// The call never returns normally.
    Diverges,
}

impl Cap {
    /// This capability's bit in a [`CapSet`].
    #[inline]
    pub const fn bit(self) -> u32 {
        1u32 << (self as u32)
    }

    /// Every capability, in declaration order.
    pub const ALL: &'static [Cap] = &[
        Cap::Numeric,
        Cap::Eq,
        Cap::Ord,
        Cap::Hash,
        Cap::Display,
        Cap::Index,
        Cap::Iterable,
        Cap::Copy,
        Cap::Repr,
        Cap::Len,
        Cap::IndexMut,
        Cap::Contains,
        Cap::Cast,
        Cap::Disposable,
        Cap::Pure,
        Cap::Deterministic,
        Cap::Commutative,
        Cap::Diverges,
    ];
}

/// A set of capabilities packed into a bitmask.
///
/// Used in static descriptors (`TypeDescriptor`, `NativeEntry`) so a capability
/// costs one bit and one `&` to test.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct CapSet(u32);

impl CapSet {
    /// No capabilities.
    pub const EMPTY: CapSet = CapSet(0);

    /// Returns this set plus `cap`.
    #[inline]
    pub const fn with(self, cap: Cap) -> Self {
        CapSet(self.0 | cap.bit())
    }

    /// Returns the union of two sets.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        CapSet(self.0 | other.0)
    }

    /// Returns `true` when `cap` is in the set.
    #[inline]
    pub const fn has(self, cap: Cap) -> bool {
        self.0 & cap.bit() != 0
    }

    /// Returns `true` when the set is empty.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bits, for macro expansion and debugging.
    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Rebuilds a set from raw bits.
    #[inline]
    pub const fn from_bits(bits: u32) -> Self {
        CapSet(bits)
    }
}

impl core::fmt::Debug for CapSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("CapSet(")?;
        let mut first = true;
        for cap in Cap::ALL {
            if self.has(*cap) {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(capability_name(*cap))?;
                first = false;
            }
        }
        f.write_str(")")
    }
}

/// One primitive type: its canonical name, aliases, representation and capabilities.
#[derive(Debug)]
pub struct Primitive {
    /// Canonical type name, as reported by `type` and used by `is`.
    pub name: &'static str,
    /// Alternative spellings that resolve to this primitive.
    pub aliases: &'static [&'static str],
    /// Representation family.
    pub kind: PrimKind,
    /// Header subtype that refines a map-backed primitive.
    pub subtype: Option<u16>,
    /// Operations the VM implements for this type.
    pub caps: CapSet,
}

/// Builds a [`CapSet`] from a list, for use in static tables.
pub const fn caps(list: &[Cap]) -> CapSet {
    let mut bits = 0u32;
    let mut rest = list;
    while let [first, tail @ ..] = rest {
        bits |= first.bit();
        rest = tail;
    }
    CapSet::from_bits(bits)
}

/// The primitive type table.
///
/// Subtype-refined entries come first so [`primitive_of_value`] reports `range`
/// rather than `map`, and so a range is never mistaken for a plain map.
pub static PRIMITIVES: &[Primitive] = &[
    Primitive {
        name: "range",
        aliases: &[],
        kind: PrimKind::Map,
        subtype: Some(SUBTYPE_RANGE),
        caps: caps(&[Cap::Display, Cap::Iterable]),
    },
    Primitive {
        name: "result",
        aliases: &[],
        kind: PrimKind::Map,
        subtype: Some(SUBTYPE_RESULT),
        caps: caps(&[Cap::Display]),
    },
    Primitive {
        name: "error",
        aliases: &[],
        kind: PrimKind::Map,
        subtype: Some(SUBTYPE_ERROR),
        caps: caps(&[Cap::Display]),
    },
    Primitive {
        name: "handle",
        aliases: &[],
        kind: PrimKind::Map,
        subtype: Some(SUBTYPE_HANDLE),
        caps: caps(&[Cap::Display]),
    },
    Primitive {
        name: "int",
        aliases: &["i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64"],
        kind: PrimKind::Int,
        subtype: None,
        caps: caps(&[Cap::Numeric, Cap::Eq, Cap::Ord, Cap::Display, Cap::Copy]),
    },
    Primitive {
        name: "float",
        aliases: &["f32", "f64"],
        kind: PrimKind::Float,
        subtype: None,
        caps: caps(&[Cap::Numeric, Cap::Eq, Cap::Ord, Cap::Display, Cap::Copy]),
    },
    Primitive {
        name: "bool",
        aliases: &[],
        kind: PrimKind::Bool,
        subtype: None,
        caps: caps(&[Cap::Eq, Cap::Display, Cap::Copy]),
    },
    Primitive {
        name: "none",
        aliases: &[],
        kind: PrimKind::None,
        subtype: None,
        caps: caps(&[Cap::Eq, Cap::Display, Cap::Copy]),
    },
    Primitive {
        name: "str",
        aliases: &["string"],
        kind: PrimKind::Str,
        subtype: None,
        caps: caps(&[Cap::Eq, Cap::Ord, Cap::Hash, Cap::Display, Cap::Copy]),
    },
    Primitive {
        name: "array",
        aliases: &[],
        kind: PrimKind::Array,
        subtype: None,
        caps: caps(&[Cap::Display, Cap::Index, Cap::IndexMut, Cap::Iterable]),
    },
    Primitive {
        name: "map",
        aliases: &["dict"],
        kind: PrimKind::Map,
        subtype: None,
        caps: caps(&[Cap::Display, Cap::Index, Cap::IndexMut]),
    },
    Primitive {
        name: "function",
        aliases: &["func"],
        kind: PrimKind::Func,
        subtype: None,
        caps: caps(&[Cap::Display]),
    },
];

impl Primitive {
    /// Returns `true` when `value` is represented as this primitive.
    #[inline]
    pub fn matches(&self, value: Value) -> bool {
        match self.subtype {
            Some(subtype) => value.map_subtype() == Some(subtype),
            None => match self.kind {
                PrimKind::Int => value.is_int(),
                PrimKind::Float => value.is_float(),
                PrimKind::Bool => value.is_bool(),
                PrimKind::None => value.is_nil(),
                PrimKind::Str => value.as_str().is_some(),
                PrimKind::Array => value.as_array().is_some(),
                PrimKind::Map => value.as_map().is_some(),
                PrimKind::Func => value.as_func().is_some(),
            },
        }
    }

    /// Returns `true` when the VM implements `cap` for this type.
    #[inline]
    pub fn has(&self, cap: Cap) -> bool {
        self.caps.has(cap)
    }
}

/// Resolves a canonical name or an alias to its primitive.
///
/// ```ignore
/// assert_eq!(primitive_of_name("u32").unwrap().name, "int");
/// ```
#[inline]
pub fn primitive_of_name(name: &str) -> Option<&'static Primitive> {
    PRIMITIVES
        .iter()
        .find(|p| p.name == name || p.aliases.contains(&name))
}

/// Returns the primitive that represents `value`, preferring subtype-refined
/// entries (`range`, `result`, …) over the plain `map`.
#[inline]
pub fn primitive_of_value(value: Value) -> Option<&'static Primitive> {
    if let Some(subtype) = value.map_subtype()
        && let Some(found) = PRIMITIVES.iter().find(|p| p.subtype == Some(subtype))
    {
        return Some(found);
    }
    PRIMITIVES
        .iter()
        .find(|p| p.subtype.is_none() && p.matches(value))
}

/// Resolves a capability name such as `"Ord"` to its capability.
#[inline]
pub fn capability_of_name(name: &str) -> Option<Cap> {
    Some(match name {
        "Numeric" => Cap::Numeric,
        "Eq" => Cap::Eq,
        "Ord" => Cap::Ord,
        "Hash" => Cap::Hash,
        "Display" => Cap::Display,
        "Index" => Cap::Index,
        "Iterable" => Cap::Iterable,
        "Copy" => Cap::Copy,
        "Repr" => Cap::Repr,
        "Len" => Cap::Len,
        "IndexMut" => Cap::IndexMut,
        "Contains" => Cap::Contains,
        "Cast" => Cap::Cast,
        "Disposable" => Cap::Disposable,
        "Pure" => Cap::Pure,
        "Deterministic" => Cap::Deterministic,
        "Commutative" => Cap::Commutative,
        "Diverges" => Cap::Diverges,
        _ => return None,
    })
}

/// Resolves a capability to its name, for diagnostics and documentation.
#[inline]
pub fn capability_name(cap: Cap) -> &'static str {
    match cap {
        Cap::Numeric => "Numeric",
        Cap::Eq => "Eq",
        Cap::Ord => "Ord",
        Cap::Hash => "Hash",
        Cap::Display => "Display",
        Cap::Index => "Index",
        Cap::Iterable => "Iterable",
        Cap::Copy => "Copy",
        Cap::Repr => "Repr",
        Cap::Len => "Len",
        Cap::IndexMut => "IndexMut",
        Cap::Contains => "Contains",
        Cap::Cast => "Cast",
        Cap::Disposable => "Disposable",
        Cap::Pure => "Pure",
        Cap::Deterministic => "Deterministic",
        Cap::Commutative => "Commutative",
        Cap::Diverges => "Diverges",
    }
}

/// Answers `value is type_name` for primitives, aliases and capabilities.
///
/// `name` is first resolved as a primitive (or alias), then as a capability. An
/// unknown name returns `false`; record and sum-type tests are handled by the
/// caller.
#[inline]
pub fn is_primitive_of(value: Value, name: &str) -> bool {
    if name == "any" {
        return true;
    }
    if let Some(prim) = primitive_of_name(name) {
        return prim.matches(value);
    }
    if let Some(cap) = capability_of_name(name) {
        return primitive_of_value(value).is_some_and(|p| p.has(cap));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_to_the_one_runtime_type() {
        for name in ["i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64"] {
            assert_eq!(primitive_of_name(name).unwrap().name, "int");
        }
        assert_eq!(primitive_of_name("f32").unwrap().name, "float");
        assert_eq!(primitive_of_name("f64").unwrap().name, "float");
        assert_eq!(primitive_of_name("dict").unwrap().name, "map");
    }

    #[test]
    fn value_classification_matches_the_representation() {
        assert_eq!(primitive_of_value(Value::from_i48(1)).unwrap().name, "int");
        assert_eq!(
            primitive_of_value(Value::from_f64(1.5)).unwrap().name,
            "float"
        );
        assert_eq!(primitive_of_value(Value::bool(true)).unwrap().name, "bool");
        assert_eq!(primitive_of_value(Value::nil()).unwrap().name, "none");
    }

    #[test]
    fn aliases_are_not_separate_types() {
        let n = Value::from_i48(300);
        assert!(is_primitive_of(n, "u32"));
        assert_eq!(is_primitive_of(n, "u32"), is_primitive_of(n, "i64"));
        assert!(!is_primitive_of(n, "float"));
    }

    #[test]
    fn capabilities_describe_real_operations() {
        let i = Value::from_i48(2);
        assert!(is_primitive_of(i, "Numeric"));
        assert!(is_primitive_of(i, "Ord"));
        assert!(is_primitive_of(i, "Copy"));
        // Strings are ordered and hashable but not numeric.
        let s = Value::from_str("hi".to_string(), &bumpalo::Bump::new());
        assert!(is_primitive_of(s, "Ord"));
        assert!(is_primitive_of(s, "Hash"));
        assert!(!is_primitive_of(s, "Numeric"));
    }

    #[test]
    fn containers_are_not_copy() {
        let bump = bumpalo::Bump::new();
        let arr = Value::from_array(vec![Value::from_i48(1)], &bump);
        assert!(!is_primitive_of(arr, "Copy"));
        assert!(is_primitive_of(arr, "Iterable"));
        assert!(is_primitive_of(arr, "Index"));
    }

    #[test]
    fn unknown_names_are_not_types() {
        assert!(!is_primitive_of(Value::from_i48(1), "Ordinary"));
        assert!(is_primitive_of(Value::from_i48(1), "any"));
    }

    #[test]
    fn cap_sets_pack_into_bits() {
        let set = CapSet::EMPTY.with(Cap::Pure).with(Cap::Len);
        assert!(set.has(Cap::Pure));
        assert!(set.has(Cap::Len));
        assert!(!set.has(Cap::Numeric));
        assert_eq!(set.bits(), Cap::Pure.bit() | Cap::Len.bit());

        let union = set.union(CapSet::EMPTY.with(Cap::Diverges));
        assert!(union.has(Cap::Pure) && union.has(Cap::Diverges));

        assert!(CapSet::EMPTY.is_empty());
        assert!(!set.is_empty());
        assert_eq!(CapSet::from_bits(set.bits()), set);
    }

    #[test]
    fn every_variant_has_a_unique_bit_and_round_trips_by_name() {
        let mut seen = 0u32;
        for cap in Cap::ALL {
            assert_eq!(seen & cap.bit(), 0, "duplicate bit for {:?}", cap);
            seen |= cap.bit();
            assert_eq!(capability_of_name(capability_name(*cap)), Some(*cap));
        }
        assert_eq!(seen.count_ones() as usize, Cap::ALL.len());
    }
}
