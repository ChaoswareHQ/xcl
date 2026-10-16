//! Standard library function mapping between IR operations and target languages.
//!
//! Per contract SM8.1: maps/stdlib_map.rs holds `IROp → (Language, Version) → TargetCall`.
//! Per contract SM8.2: Missing entries return `TranslateError::NoStdlibMapping`.
//! Per contract SM8.3: Import paths are part of the mapping.
//! Per contract SM8.4: Semantic equivalence is required, not textual similarity.

use crate::translate::ir::ops::IrOp;
use crate::translate::version::{Language, LangSpec};

/// Result of mapping an IR operation to a target language call.
#[derive(Clone, Debug)]
pub struct StdlibMapping {
    /// The target function/method name.
    pub call: String,
    /// Optional import/using statement needed.
    pub import: Option<String>,
}

/// Map an IR operation to a target language standard library call.
///
/// # Arguments
/// - `op` — The IR operation to map
/// - `target` — The target language specification
///
/// # Returns
/// The target call mapping, or `None` if no mapping exists
pub fn map_stdlib(op: &IrOp, target: &LangSpec) -> Option<StdlibMapping> {
    match target.language {
        Language::Java => map_stdlib_java(op, target),
        Language::Rust => map_stdlib_rust(op, target),
        Language::Python => map_stdlib_python(op, target),
        Language::C | Language::Cpp => map_stdlib_c(op, target),
        Language::Xcl => map_stdlib_xcl(op),
    }
}

/// Map IR operation to Java standard library call.
fn map_stdlib_java(op: &IrOp, _target: &LangSpec) -> Option<StdlibMapping> {
    match op {
        IrOp::Len => Some(StdlibMapping {
            call: ".length".to_string(),
            import: None,
        }),
        IrOp::Concat => Some(StdlibMapping {
            call: ".concat(...)".to_string(),
            import: None,
        }),
        IrOp::Cast => Some(StdlibMapping {
            call: "(Type) ...".to_string(),
            import: None,
        }),
        IrOp::Index => Some(StdlibMapping {
            call: "[...]".to_string(),
            import: None,
        }),
        IrOp::Slice => Some(StdlibMapping {
            call: ".substring(...)".to_string(),
            import: None,
        }),
        _ => None,
    }
}

/// Map IR operation to Rust standard library call.
fn map_stdlib_rust(op: &IrOp, _target: &LangSpec) -> Option<StdlibMapping> {
    match op {
        IrOp::Len => Some(StdlibMapping {
            call: ".len()".to_string(),
            import: None,
        }),
        IrOp::Concat => Some(StdlibMapping {
            call: ".concat(...)".to_string(),
            import: None,
        }),
        IrOp::Cast => Some(StdlibMapping {
            call: "as Type".to_string(),
            import: None,
        }),
        IrOp::Index => Some(StdlibMapping {
            call: "[...]".to_string(),
            import: None,
        }),
        IrOp::Slice => Some(StdlibMapping {
            call: "[start..end]".to_string(),
            import: None,
        }),
        _ => None,
    }
}

/// Map IR operation to Python standard library call.
fn map_stdlib_python(op: &IrOp, _target: &LangSpec) -> Option<StdlibMapping> {
    match op {
        IrOp::Len => Some(StdlibMapping {
            call: "len(...)".to_string(),
            import: None,
        }),
        IrOp::Concat => Some(StdlibMapping {
            call: "+".to_string(),
            import: None,
        }),
        IrOp::Cast => Some(StdlibMapping {
            call: "Type(...)".to_string(),
            import: None,
        }),
        IrOp::Index => Some(StdlibMapping {
            call: "[...]".to_string(),
            import: None,
        }),
        IrOp::Slice => Some(StdlibMapping {
            call: "[start:end]".to_string(),
            import: None,
        }),
        _ => None,
    }
}

/// Map IR operation to C/C++ standard library call.
fn map_stdlib_c(op: &IrOp, _target: &LangSpec) -> Option<StdlibMapping> {
    match op {
        IrOp::Len => Some(StdlibMapping {
            call: "sizeof(...)".to_string(),
            import: Some("#include <stddef.h>".to_string()),
        }),
        IrOp::Concat => Some(StdlibMapping {
            call: "strcat(...)".to_string(),
            import: Some("#include <string.h>".to_string()),
        }),
        IrOp::Cast => Some(StdlibMapping {
            call: "(Type) ...".to_string(),
            import: None,
        }),
        IrOp::Index => Some(StdlibMapping {
            call: "[...]".to_string(),
            import: None,
        }),
        IrOp::Slice => Some(StdlibMapping {
            call: "strncpy(...)".to_string(),
            import: Some("#include <string.h>".to_string()),
        }),
        _ => None,
    }
}

/// Map IR operation to XCL standard library call.
fn map_stdlib_xcl(op: &IrOp) -> Option<StdlibMapping> {
    match op {
        IrOp::Len => Some(StdlibMapping {
            call: "len(...)".to_string(),
            import: None,
        }),
        IrOp::Concat => Some(StdlibMapping {
            call: "+".to_string(),
            import: None,
        }),
        IrOp::Cast => Some(StdlibMapping {
            call: "as Type(...)".to_string(),
            import: None,
        }),
        IrOp::Index => Some(StdlibMapping {
            call: "[...]".to_string(),
            import: None,
        }),
        IrOp::Slice => Some(StdlibMapping {
            call: "[start:end]".to_string(),
            import: None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_len_to_java() {
        let target = LangSpec::java("25");
        let mapping = map_stdlib(&IrOp::Len, &target);
        assert!(mapping.is_some());
        assert_eq!(mapping.unwrap().call, ".length");
    }

    #[test]
    fn test_map_len_to_rust() {
        let target = LangSpec::rust("2024");
        let mapping = map_stdlib(&IrOp::Len, &target);
        assert!(mapping.is_some());
        assert_eq!(mapping.unwrap().call, ".len()");
    }

    #[test]
    fn test_map_len_to_python() {
        let target = LangSpec::python("313");
        let mapping = map_stdlib(&IrOp::Len, &target);
        assert!(mapping.is_some());
        assert_eq!(mapping.unwrap().call, "len(...)");
    }
}
