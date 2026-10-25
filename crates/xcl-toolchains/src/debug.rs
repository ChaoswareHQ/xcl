//! Development-only mirrors of the language surface.
//!
//! `xcl-vm` is pure language mechanics: primitives, keywords, bytecode and the
//! execution loop. The compiler never needs a name table to produce correct
//! code — the inliner only asks whether a free identifier in a pure function
//! body *could* resolve at run time, and "unknown" is always a safe answer
//! because it just declines to inline.
//!
//! This module keeps the hand-maintained mirror of that surface for debug
//! builds, so a developer can see why a function was or was not inlined, and so
//! drift is caught here instead of in a benchmark. **Nothing in this module is
//! compiled into a release build**: `is_language_name` becomes a constant
//! `false` and the tables disappear.
//!
//! Contents are limited to the language itself — primitive type names, the
//! keywords the AST delivers as identifiers, and the built-in variants. Std and
//! plugin names (`print`, `len`, `sqrt`, `Option`, `Vec`, …) belong to
//! `xcl-std` and must never be added here; the VM does not define them.

/// Primitive type names: the sized aliases and the runtime type names.
#[cfg(debug_assertions)]
const PRIMITIVES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "isize", "usize", "f32",
    "f64", "int", "float", "bool", "str", "string", "any", "none", "function", "func", "array",
    "map", "dict", "range", "result", "error", "handle",
];

/// Keywords that the parser hands to the compiler as identifiers.
#[cfg(debug_assertions)]
const KEYWORDS: &[&str] = &["true", "false"];

/// Names the runtime defines for type expressions and sum types.
#[cfg(debug_assertions)]
const VARIANTS: &[&str] = &["Result", "Ok", "Err"];

/// Whether `name` is part of the language surface rather than a free local.
///
/// Debug builds consult the tables above. Release builds answer `false`, which
/// only costs an inlining opportunity — never correctness.
#[cfg(debug_assertions)]
pub(crate) fn is_language_name(name: &str) -> bool {
    PRIMITIVES.contains(&name) || KEYWORDS.contains(&name) || VARIANTS.contains(&name)
}

/// Release counterpart: no name table, so nothing is assumed to be a language
/// name.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub(crate) fn is_language_name(_name: &str) -> bool {
    false
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::is_language_name;

    #[test]
    fn recognises_primitives_keywords_and_variants() {
        for name in ["i64", "u8", "f64", "str", "range", "none", "handle"] {
            assert!(is_language_name(name), "{name} is a primitive");
        }
        for name in ["true", "false", "Result", "Ok", "Err"] {
            assert!(is_language_name(name), "{name} is a language name");
        }
    }

    #[test]
    fn contains_no_std_or_plugin_names() {
        // The VM has no builtin functions; these live in `xcl-std`.
        for name in [
            "print", "println", "input", "len", "sqrt", "pow", "sin", "Option", "Vec", "String",
            "HashMap",
        ] {
            assert!(!is_language_name(name), "{name} is a std/plugin name");
        }
    }

    #[test]
    fn ordinary_identifiers_are_not_language_names() {
        for name in ["$n", "total", "x1", "fact", ""] {
            assert!(!is_language_name(name), "{name} must be treated as a local");
        }
    }
}
