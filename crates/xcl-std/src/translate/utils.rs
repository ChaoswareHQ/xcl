//! Shared utilities for the translator module.
//!
//! Common helpers used by multiple backends to avoid duplication:
//! string escaping, indentation, and identifier conversion.

use crate::translate::ir::node::IrLiteral;

/// Indent a block of text by the given number of levels (4 spaces per level).
pub fn indent(text: &str, level: usize) -> String {
    let prefix = "    ".repeat(level);
    text.lines()
        .map(|line| format!("{}{}", prefix, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escape a string literal for a target language.
///
/// Handles backslashes and quotes. Each backend calls this with its
/// quote character and any additional escape sequences.
pub fn escape_string(s: &str, quote_char: char) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    result.push(quote_char);
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c == quote_char => {
                result.push('\\');
                result.push(c);
            }
            c => result.push(c),
        }
    }
    result.push(quote_char);
    result
}

/// Emit a literal value to a language-specific string.
///
/// Backends provide closures for the language-specific parts:
/// - `bool_true` / `bool_false` for boolean literals
/// - `nil` for null/nil literals
/// - `int_suffix` for integer type suffixes (e.g. "i64" in Rust)
/// - `float_suffix` for float suffixes (e.g. ".0" for whole numbers in C/Java)
/// - `quote_char` for string quoting
pub fn emit_literal(
    lit: &IrLiteral,
    bool_true: &str,
    bool_false: &str,
    nil: &str,
    int_suffix: &str,
    float_suffix: &str,
    quote_char: char,
) -> String {
    match lit {
        IrLiteral::Bool(b) => if *b { bool_true.to_string() } else { bool_false.to_string() },
        IrLiteral::Int(n) => {
            if int_suffix.is_empty() {
                n.to_string()
            } else {
                format!("{}{}", n, int_suffix)
            }
        }
        IrLiteral::Float(n) => {
            let base = if n.fract() == 0.0 && !float_suffix.is_empty() {
                format!("{}{}", n, float_suffix)
            } else {
                n.to_string()
            };
            if int_suffix.is_empty() {
                base
            } else {
                base
            }
        }
        IrLiteral::Str(s) => escape_string(s, quote_char),
        IrLiteral::Nil => nil.to_string(),
    }
}

/// Convert a snake_case or camelCase identifier to camelCase.
pub fn to_camel_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut capitalize_next = false;

    for c in name.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert a snake_case or camelCase identifier to PascalCase.
pub fn to_pascal_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut capitalize_next = true;

    for c in name.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert an identifier to a valid C identifier, replacing reserved keywords.
pub fn to_c_identifier(name: &str, reserved: &[&str]) -> String {
    let candidate = name.to_lowercase();
    if reserved.contains(&candidate.as_str()) {
        format!("{}_value", candidate)
    } else {
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indent() {
        let text = "line1\nline2";
        assert_eq!(indent(text, 1), "    line1\n    line2");
        assert_eq!(indent(text, 2), "        line1\n        line2");
    }

    #[test]
    fn test_escape_string_double_quotes() {
        assert_eq!(escape_string("hello", '"'), "\"hello\"");
        assert_eq!(escape_string("say \"hi\"", '"'), "\"say \\\"hi\\\"\"");
        assert_eq!(escape_string("back\\slash", '"'), "\"back\\\\slash\"");
    }

    #[test]
    fn test_escape_string_single_quotes() {
        assert_eq!(escape_string("hello", '\''), "'hello'");
        assert_eq!(escape_string("it's", '\''), "'it\\'s'");
    }

    #[test]
    fn test_emit_literal_int() {
        assert_eq!(
            emit_literal(&IrLiteral::Int(42), "true", "false", "null", "", "", '"'),
            "42"
        );
        assert_eq!(
            emit_literal(&IrLiteral::Int(42), "true", "false", "null", "i64", "", '"'),
            "42i64"
        );
    }

    #[test]
    fn test_emit_literal_float() {
        assert_eq!(
            emit_literal(&IrLiteral::Float(3.14), "true", "false", "null", "", "", '"'),
            "3.14"
        );
        assert_eq!(
            emit_literal(&IrLiteral::Float(3.0), "true", "false", "null", "", ".0", '"'),
            "3.0"
        );
    }

    #[test]
    fn test_emit_literal_bool() {
        assert_eq!(
            emit_literal(&IrLiteral::Bool(true), "true", "false", "null", "", "", '"'),
            "true"
        );
        assert_eq!(
            emit_literal(&IrLiteral::Bool(false), "1", "0", "NULL", "", "", '"'),
            "0"
        );
    }

    #[test]
    fn test_emit_literal_nil() {
        assert_eq!(
            emit_literal(&IrLiteral::Nil, "true", "false", "null", "", "", '"'),
            "null"
        );
        assert_eq!(
            emit_literal(&IrLiteral::Nil, "true", "false", "None", "", "", '"'),
            "None"
        );
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("hello_world"), "helloWorld");
        assert_eq!(to_camel_case("helloWorld"), "helloWorld");
        assert_eq!(to_camel_case("x"), "x");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("helloWorld"), "HelloWorld");
        assert_eq!(to_pascal_case("x"), "X");
    }

    #[test]
    fn test_to_c_identifier_reserved() {
        let reserved = ["type", "match", "loop"];
        assert_eq!(to_c_identifier("type", &reserved), "type_value");
        assert_eq!(to_c_identifier("foo", &reserved), "foo");
    }
}
