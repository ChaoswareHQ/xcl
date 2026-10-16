//! C idiom conversion utilities.
//!
//! Applies C-specific rewrite rules after structural emit, before final format.
//! Per contract ID6.1: backend/c/idioms.rs holds target-specific rewrite rules.
//! Per contract ID6.4: Idioms never change semantics. Only shape.
//! Per contract ID6.5: Idioms are version-gated.

use std::collections::HashSet;

/// C standard version identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CVersion {
    /// C99 standard
    C99,
    /// C11 standard
    C11,
    /// C17 standard (maintenance release, same features as C11)
    C17,
    /// C23 standard
    C23,
}

impl CVersion {
    /// Check if this version supports `true`/`false` keywords (C23+).
    pub fn has_true_false(self) -> bool {
        matches!(self, CVersion::C23)
    }

    /// Check if this version supports `nullptr` keyword (C23+).
    pub fn has_nullptr(self) -> bool {
        matches!(self, CVersion::C23)
    }

    /// Check if this version supports `auto` type inference (C23+).
    pub fn has_auto_inference(self) -> bool {
        matches!(self, CVersion::C23)
    }

    /// Check if this version supports `typeof` keyword (C23+).
    pub fn has_typeof(self) -> bool {
        matches!(self, CVersion::C23)
    }

    /// Check if this version supports `_Bool` keyword (C99+).
    pub fn has_bool(self) -> bool {
        matches!(self, CVersion::C99 | CVersion::C11 | CVersion::C17 | CVersion::C23)
    }

    /// Check if this version supports `_Static_assert` (C11+).
    pub fn has_static_assert(self) -> bool {
        matches!(self, CVersion::C11 | CVersion::C17 | CVersion::C23)
    }

    /// Check if this version supports `_Atomic` (C11+).
    pub fn has_atomic(self) -> bool {
        matches!(self, CVersion::C11 | CVersion::C17 | CVersion::C23)
    }

    /// Check if this version supports `//` comments (C99+).
    pub fn has_single_line_comments(self) -> bool {
        matches!(self, CVersion::C99 | CVersion::C11 | CVersion::C17 | CVersion::C23)
    }

    /// Get the null literal for this version.
    pub fn null_literal(self) -> &'static str {
        if self.has_nullptr() {
            "nullptr"
        } else {
            "NULL"
        }
    }

    /// Get the boolean type name for this version.
    pub fn bool_type(self) -> &'static str {
        if matches!(self, CVersion::C23) {
            "bool"
        } else {
            "_Bool"
        }
    }

    /// Get the boolean true literal for this version.
    pub fn bool_true(self) -> &'static str {
        if self.has_true_false() {
            "true"
        } else {
            "1"
        }
    }

    /// Get the boolean false literal for this version.
    pub fn bool_false(self) -> &'static str {
        if self.has_true_false() {
            "false"
        } else {
            "0"
        }
    }

    /// Get the static assert keyword for this version.
    pub fn static_assert_keyword(self) -> &'static str {
        if matches!(self, CVersion::C23) {
            "static_assert"
        } else {
            "_Static_assert"
        }
    }

    /// Get the __STDC_VERSION__ macro value for this version.
    pub fn stdc_version_macro(self) -> &'static str {
        match self {
            CVersion::C99 => "199901L",
            CVersion::C11 => "201112L",
            CVersion::C17 => "201710L",
            CVersion::C23 => "202311L",
        }
    }

    /// Get the compiler flag for this version.
    pub fn compiler_flag(self) -> &'static str {
        match self {
            CVersion::C99 => "-std=c99",
            CVersion::C11 => "-std=c11",
            CVersion::C17 => "-std=c17",
            CVersion::C23 => "-std=c23",
        }
    }
}

/// Reserved keywords by C version.
pub fn reserved_keywords(version: CVersion) -> HashSet<&'static str> {
    let mut keywords: HashSet<&'static str> = [
        // C89 base keywords (32)
        "auto", "break", "case", "char", "const", "continue", "default", "do",
        "double", "else", "enum", "extern", "float", "for", "goto", "if",
        "int", "long", "register", "return", "short", "signed", "sizeof",
        "static", "struct", "switch", "typedef", "union", "unsigned", "void",
        "volatile", "while",
    ].into_iter().collect();

    if version >= CVersion::C99 {
        for kw in ["inline", "restrict", "_Bool", "_Complex", "_Imaginary"] {
            keywords.insert(kw);
        }
    }

    if version >= CVersion::C11 {
        for kw in ["_Alignas", "_Alignof", "_Atomic", "_Generic", "_Noreturn",
                    "_Static_assert", "_Thread_local"] {
            keywords.insert(kw);
        }
    }

    if version >= CVersion::C23 {
        for kw in ["alignas", "alignof", "bool", "static_assert", "thread_local",
                    "true", "false", "typeof", "typeof_unqual", "nullptr",
                    "constexpr", "_BitInt", "_Decimal32", "_Decimal64", "_Decimal128"] {
            keywords.insert(kw);
        }
    }

    keywords
}

impl PartialOrd for CVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let to_ord = |v: CVersion| -> u32 {
            match v {
                CVersion::C99 => 1,
                CVersion::C11 => 2,
                CVersion::C17 => 3,
                CVersion::C23 => 4,
            }
        };
        to_ord(*self).cmp(&to_ord(*other))
    }
}

/// Convert a variable name to a valid C identifier.
///
/// C identifiers cannot conflict with reserved keywords.
/// If the name conflicts, append an underscore.
pub fn to_c_identifier(name: &str, version: CVersion) -> String {
    let keywords = reserved_keywords(version);
    let mut result = String::with_capacity(name.len() + 1);

    for (i, c) in name.chars().enumerate() {
        if c.is_alphanumeric() || c == '_' {
            result.push(c);
        } else if i == 0 && c.is_ascii_alphabetic() || c == '_' {
            result.push(c);
        } else {
            result.push('_');
        }
    }

    if keywords.contains(result.as_str()) {
        result.push('_');
    }

    result
}

/// Apply C-specific idioms to the generated code.
///
/// This function performs version-gated rewrites on the emitted C code
/// to make it more idiomatic for the target standard.
pub fn apply_idioms(code: &str, version: CVersion) -> String {
    let mut result = code.to_string();

    // C23: Replace NULL with nullptr if supported
    if version.has_nullptr() {
        result = result.replace("NULL", "nullptr");
    }

    // C23: Replace _Bool with bool if supported
    if matches!(version, CVersion::C23) {
        result = result.replace("_Bool", "bool");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c99_null_literal() {
        assert_eq!(CVersion::C99.null_literal(), "NULL");
    }

    #[test]
    fn test_c23_null_literal() {
        assert_eq!(CVersion::C23.null_literal(), "nullptr");
    }

    #[test]
    fn test_c99_bool_type() {
        assert_eq!(CVersion::C99.bool_type(), "_Bool");
    }

    #[test]
    fn test_c23_bool_type() {
        assert_eq!(CVersion::C23.bool_type(), "bool");
    }

    #[test]
    fn test_c23_has_true_false() {
        assert!(CVersion::C23.has_true_false());
        assert!(!CVersion::C99.has_true_false());
    }

    #[test]
    fn test_reserved_keywords_c99() {
        let keywords = reserved_keywords(CVersion::C99);
        assert!(keywords.contains("_Bool"));
        assert!(!keywords.contains("bool"));
    }

    #[test]
    fn test_reserved_keywords_c23() {
        let keywords = reserved_keywords(CVersion::C23);
        assert!(keywords.contains("bool"));
        assert!(keywords.contains("nullptr"));
        assert!(keywords.contains("true"));
        assert!(keywords.contains("false"));
    }

    #[test]
    fn test_to_c_identifier_keyword_conflict() {
        // "for" is a reserved keyword in all C versions
        let result = to_c_identifier("for", CVersion::C99);
        assert_eq!(result, "for_");
    }

    #[test]
    fn test_to_c_identifier_normal() {
        let result = to_c_identifier("my_var", CVersion::C99);
        assert_eq!(result, "my_var");
    }

    #[test]
    fn test_version_ordering() {
        assert!(CVersion::C99 < CVersion::C11);
        assert!(CVersion::C11 < CVersion::C17);
        assert!(CVersion::C17 < CVersion::C23);
    }
}
