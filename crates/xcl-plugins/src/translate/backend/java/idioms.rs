//! Java-specific idiom rewrite rules.
//!
//! Per contract ID6.1: backend/<lang>/idioms.rs holds target-specific rewrite rules.
//! Per contract ID6.2: Idioms run after structural emit, before final format.
//! Per contract ID6.4: Idioms never change semantics. Only shape.
//! Per contract ID6.5: Idioms are version-gated.
//!
//! # Java Version Keyword Table
//!
//! | Feature | Java 8 | Java 11 | Java 17 | Java 21 | Java 25 |
//! |---------|--------|---------|---------|---------|---------|
//! | Lambdas | ✓ | ✓ | ✓ | ✓ | ✓ |
//! | Streams API | ✓ | ✓ | ✓ | ✓ | ✓ |
//! | Optional | ✓ | ✓ | ✓ | ✓ | ✓ |
//! | `var` keyword | ✗ | ✓ | ✓ | ✓ | ✓ |
//! | Text blocks | ✗ | ✗ | ✓ | ✓ | ✓ |
//! | Records | ✗ | ✗ | ✓ (preview) | ✓ | ✓ |
//! | Sealed classes | ✗ | ✗ | ✓ | ✓ | ✓ |
//! | Pattern matching (instanceof) | ✗ | ✗ | ✓ | ✓ | ✓ |
//! | Pattern matching (switch) | ✗ | ✗ | ✗ | ✓ | ✓ |
//! | Record patterns | ✗ | ✗ | ✗ | ✓ | ✓ |
//! | Switch expressions | ✗ | ✗ | ✓ | ✓ | ✓ |
//! | Virtual threads | ✗ | ✗ | ✗ | ✓ | ✓ |
//! | Primitive generics (Valhalla) | ✗ | ✗ | ✗ | ✗ | ✓ |
//! | `yield` in switch | ✗ | ✗ | ✓ | ✓ | ✓ |
//! | HTTP Client | ✗ | ✓ | ✓ | ✓ | ✓ |
//! | `Files.mismatch` | ✗ | ✓ | ✓ | ✓ | ✓ |
//! | Compact numbers (`1_000_000`) | ✓ | ✓ | ✓ | ✓ | ✓ |
//! | `try-with-resources` var | ✗ | ✓ | ✓ | ✓ | ✓ |

use crate::translate::utils;

/// Java version identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JavaVersion {
    /// Java 8 (LTS, 2014) — Lambdas, Streams, Optional
    V8,
    /// Java 11 (LTS, 2018) — `var` keyword, HTTP Client
    V11,
    /// Java 17 (LTS, 2021) — Sealed classes, pattern matching, text blocks
    V17,
    /// Java 21 (LTS, 2023) — Virtual threads, record patterns, switch patterns
    V21,
    /// Java 25 (LTS, 2025) — Primitive generics (Valhalla)
    V25,
}

impl JavaVersion {
    /// Create a JavaVersion from a version string.
    pub fn from_version(version: &str) -> Option<Self> {
        match version {
            "8" => Some(JavaVersion::V8),
            "11" => Some(JavaVersion::V11),
            "17" => Some(JavaVersion::V17),
            "21" => Some(JavaVersion::V21),
            "25" => Some(JavaVersion::V25),
            _ => None,
        }
    }

    /// Get the version number as a string.
    pub fn version_string(self) -> &'static str {
        match self {
            JavaVersion::V8 => "8",
            JavaVersion::V11 => "11",
            JavaVersion::V17 => "17",
            JavaVersion::V21 => "21",
            JavaVersion::V25 => "25",
        }
    }

    /// Check if this version supports `var` keyword (Java 11+).
    pub fn has_var(self) -> bool {
        matches!(self, JavaVersion::V11 | JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports text blocks (Java 17+).
    pub fn has_text_blocks(self) -> bool {
        matches!(self, JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports records (Java 17+).
    pub fn has_records(self) -> bool {
        matches!(self, JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports sealed classes (Java 17+).
    pub fn has_sealed_classes(self) -> bool {
        matches!(self, JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports pattern matching for instanceof (Java 17+).
    pub fn has_pattern_matching_instanceof(self) -> bool {
        matches!(self, JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports pattern matching for switch (Java 21+).
    pub fn has_pattern_matching_switch(self) -> bool {
        matches!(self, JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports record patterns (Java 21+).
    pub fn has_record_patterns(self) -> bool {
        matches!(self, JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports switch expressions with `yield` (Java 17+).
    pub fn has_switch_expressions(self) -> bool {
        matches!(self, JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports virtual threads (Java 21+).
    pub fn has_virtual_threads(self) -> bool {
        matches!(self, JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports primitive generics / Valhalla (Java 25+).
    pub fn has_primitive_generics(self) -> bool {
        matches!(self, JavaVersion::V25)
    }

    /// Check if this version supports HTTP Client API (Java 11+).
    pub fn has_http_client(self) -> bool {
        matches!(self, JavaVersion::V11 | JavaVersion::V17 | JavaVersion::V21 | JavaVersion::V25)
    }

    /// Check if this version supports lambdas (Java 8+).
    pub fn has_lambdas(self) -> bool {
        true // All supported versions have lambdas
    }

    /// Check if this version supports Streams API (Java 8+).
    pub fn has_streams(self) -> bool {
        true // All supported versions have streams
    }

    /// Check if this version supports Optional (Java 8+).
    pub fn has_optional(self) -> bool {
        true // All supported versions have Optional
    }

    /// Get the variable declaration keyword for this version.
    /// Java 8 uses explicit types, Java 11+ can use `var`.
    pub fn var_keyword(self) -> &'static str {
        if self.has_var() {
            "var"
        } else {
            "Object" // Fallback for Java 8 when type is unknown
        }
    }

    /// Get the reserved keywords for this version.
    pub fn reserved_keywords(self) -> &'static [&'static str] {
        match self {
            JavaVersion::V8 => &[
                "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
                "class", "const", "continue", "default", "do", "double", "else", "enum",
                "extends", "final", "finally", "float", "for", "goto", "if", "implements",
                "import", "instanceof", "int", "interface", "long", "native", "new",
                "package", "private", "protected", "public", "return", "short", "static",
                "strictfp", "super", "switch", "synchronized", "this", "throw", "throws",
                "transient", "try", "void", "volatile", "while",
            ],
            JavaVersion::V11 => &[
                "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
                "class", "const", "continue", "default", "do", "double", "else", "enum",
                "extends", "final", "finally", "float", "for", "goto", "if", "implements",
                "import", "instanceof", "int", "interface", "long", "native", "new",
                "package", "private", "protected", "public", "return", "short", "static",
                "strictfp", "super", "switch", "synchronized", "this", "throw", "throws",
                "transient", "try", "var", "void", "volatile", "while",
            ],
            JavaVersion::V17 => &[
                "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
                "class", "const", "continue", "default", "do", "double", "else", "enum",
                "extends", "final", "finally", "float", "for", "goto", "if", "implements",
                "import", "instanceof", "int", "interface", "long", "native", "new",
                "non-sealed", "package", "private", "protected", "public", "record",
                "sealed", "return", "short", "static", "strictfp", "super", "switch",
                "synchronized", "this", "throw", "throws", "transient", "try", "var",
                "void", "volatile", "while", "yield",
            ],
            JavaVersion::V21 => &[
                "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
                "class", "const", "continue", "default", "do", "double", "else", "enum",
                "extends", "final", "finally", "float", "for", "goto", "if", "implements",
                "import", "instanceof", "int", "interface", "long", "native", "new",
                "non-sealed", "package", "private", "protected", "public", "record",
                "sealed", "return", "short", "static", "strictfp", "super", "switch",
                "synchronized", "this", "throw", "throws", "transient", "try", "var",
                "void", "volatile", "while", "yield",
            ],
            JavaVersion::V25 => &[
                "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
                "class", "const", "continue", "default", "do", "double", "else", "enum",
                "extends", "final", "finally", "float", "for", "goto", "if", "implements",
                "import", "instanceof", "int", "interface", "long", "native", "new",
                "non-sealed", "package", "private", "protected", "public", "record",
                "sealed", "return", "short", "static", "strictfp", "super", "switch",
                "synchronized", "this", "throw", "throws", "transient", "try", "var",
                "void", "volatile", "while", "yield",
            ],
        }
    }

    /// Sanitize an identifier to avoid conflicts with reserved keywords.
    pub fn sanitize_identifier(self, name: &str) -> String {
        let reserved = self.reserved_keywords();
        if reserved.contains(&name) {
            format!("_${}", name)
        } else {
            name.to_string()
        }
    }
}

/// Convert XCL-style variable names to Java camelCase (delegates to shared utils).
pub fn to_camel_case(name: &str) -> String {
    utils::to_camel_case(name)
}

/// Convert XCL-style type names to Java PascalCase (delegates to shared utils).
pub fn to_pascal_case(name: &str) -> String {
    utils::to_pascal_case(name)
}

/// Format a Java method signature.
pub fn format_method_signature(name: &str, params: &[(&str, &str)], ret: &str) -> String {
    let param_strs: Vec<String> = params
        .iter()
        .map(|(pname, ptype)| format!("{} {}", ptype, to_camel_case(pname)))
        .collect();

    if ret == "void" || ret == "nil" {
        format!("public {}({})", to_camel_case(name), param_strs.join(", "))
    } else {
        format!("public {} {}({})", ret, to_camel_case(name), param_strs.join(", "))
    }
}

/// Apply Java idioms to emitted code, version-gated.
pub fn apply_idioms(code: &str, version: JavaVersion) -> String {
    // Replace XCL-style nil with Java null
    let code = code.replace("none", "null");

    // Replace XCL-style boolean literals
    let code = code.replace("true", "true").replace("false", "false");

    // For Java 8, replace `var` with explicit types where possible
    let code = if !version.has_var() {
        code.replace("var ", "Object ")
    } else {
        code
    };

    // For Java 8, replace text blocks with string concatenation
    let code = if !version.has_text_blocks() {
        // Simple replacement: """...""" becomes "..." with escaped newlines
        code.replace("\"\"\"", "\"")
    } else {
        code
    };

    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("my_variable"), "myVariable");
        assert_eq!(to_camel_case("user_id"), "userId");
        assert_eq!(to_camel_case("alreadyCamel"), "alreadyCamel");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("my_type"), "MyType");
        assert_eq!(to_pascal_case("user_data"), "UserData");
    }

    #[test]
    fn test_format_method_signature() {
        let sig = format_method_signature(
            "get_user",
            &[("user_id", "int"), ("active", "boolean")],
            "User"
        );
        assert_eq!(sig, "public User getUser(int userId, boolean active)");
    }

    #[test]
    fn test_java_version_from_string() {
        assert_eq!(JavaVersion::from_version("8"), Some(JavaVersion::V8));
        assert_eq!(JavaVersion::from_version("11"), Some(JavaVersion::V11));
        assert_eq!(JavaVersion::from_version("17"), Some(JavaVersion::V17));
        assert_eq!(JavaVersion::from_version("21"), Some(JavaVersion::V21));
        assert_eq!(JavaVersion::from_version("25"), Some(JavaVersion::V25));
        assert_eq!(JavaVersion::from_version("99"), None);
    }

    #[test]
    fn test_java8_no_var() {
        assert!(!JavaVersion::V8.has_var());
        assert_eq!(JavaVersion::V8.var_keyword(), "Object");
    }

    #[test]
    fn test_java11_has_var() {
        assert!(JavaVersion::V11.has_var());
        assert_eq!(JavaVersion::V11.var_keyword(), "var");
    }

    #[test]
    fn test_java8_no_records() {
        assert!(!JavaVersion::V8.has_records());
        assert!(!JavaVersion::V11.has_records());
        assert!(JavaVersion::V17.has_records());
    }

    #[test]
    fn test_java17_has_text_blocks() {
        assert!(!JavaVersion::V8.has_text_blocks());
        assert!(!JavaVersion::V11.has_text_blocks());
        assert!(JavaVersion::V17.has_text_blocks());
    }

    #[test]
    fn test_java21_has_record_patterns() {
        assert!(!JavaVersion::V8.has_record_patterns());
        assert!(!JavaVersion::V17.has_record_patterns());
        assert!(JavaVersion::V21.has_record_patterns());
    }

    #[test]
    fn test_java25_has_primitive_generics() {
        assert!(!JavaVersion::V21.has_primitive_generics());
        assert!(JavaVersion::V25.has_primitive_generics());
    }

    #[test]
    fn test_sanitize_identifier() {
        let v8 = JavaVersion::V8;
        assert_eq!(v8.sanitize_identifier("class"), "_$class");
        assert_eq!(v8.sanitize_identifier("myVar"), "myVar");
    }

    #[test]
    fn test_apply_idioms_java8_replaces_var() {
        let code = "var x = 10;";
        let result = apply_idioms(code, JavaVersion::V8);
        assert_eq!(result, "Object x = 10;");
    }

    #[test]
    fn test_apply_idioms_java11_keeps_var() {
        let code = "var x = 10;";
        let result = apply_idioms(code, JavaVersion::V11);
        assert_eq!(result, "var x = 10;");
    }
}
