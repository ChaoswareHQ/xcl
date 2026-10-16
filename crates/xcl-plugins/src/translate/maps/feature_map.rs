//! Feature mapping between IR features and target language support.
//!
//! Per contract V9.2: Feature gates are per-version.
//! E.g., Java 16+ supports records, Java 8 does not.

use crate::translate::ir::ops::Feature;
use crate::translate::version::{Language, LangSpec};

/// Check if a target language version supports a given feature.
///
/// # Arguments
/// - `feature` — The feature to check
/// - `target` — The target language specification
///
/// # Returns
/// `true` if the target supports the feature
pub fn supports_feature(feature: &Feature, target: &LangSpec) -> bool {
    match target.language {
        Language::Java => supports_java(feature, target),
        Language::Rust => supports_rust(feature, target),
        Language::Python => supports_python(feature, target),
        Language::C | Language::Cpp => supports_c(feature, target),
        Language::Xcl => supports_xcl(feature),
    }
}

/// Check Java feature support.
fn supports_java(feature: &Feature, target: &LangSpec) -> bool {
    let version_num = target.version.0.parse::<u32>().unwrap_or(0);

    match feature {
        Feature::Records => version_num >= 16,
        Feature::PatternMatching => version_num >= 14,
        Feature::Generics => version_num >= 5,
        Feature::NullSafety => false, // Java has nulls
        Feature::Defer => false, // Use try-with-resources
        Feature::Async => version_num >= 9, // Virtual threads in 19+, Project Loom
        Feature::Closures => version_num >= 8,
        Feature::Traits => version_num >= 8, // Interfaces with default methods
        Feature::SumTypes => false, // No native sum types
        Feature::StringInterpolation => false, // Use String.format or text blocks
        Feature::MultiLineStrings => version_num >= 13, // Text blocks
        Feature::RawStrings => version_num >= 13, // Text blocks
        Feature::TypeInference => version_num >= 10, // var
        Feature::OperatorOverloading => false,
        Feature::MatchExpressions => version_num >= 14, // switch expressions
    }
}

/// Check Rust feature support.
fn supports_rust(feature: &Feature, target: &LangSpec) -> bool {
    let version_num = target.version.0.parse::<u32>().unwrap_or(0);

    match feature {
        Feature::Records => true, // Structs
        Feature::PatternMatching => true,
        Feature::Generics => true,
        Feature::NullSafety => true, // Option/Result
        Feature::Defer => version_num >= 2024, // defer stabilizing in 2024
        Feature::Async => true,
        Feature::Closures => true,
        Feature::Traits => true,
        Feature::SumTypes => true, // Enums
        Feature::StringInterpolation => true, // format!
        Feature::MultiLineStrings => true,
        Feature::RawStrings => true, // r#"..."#
        Feature::TypeInference => true,
        Feature::OperatorOverloading => true,
        Feature::MatchExpressions => true,
    }
}

/// Check Python feature support.
fn supports_python(feature: &Feature, target: &LangSpec) -> bool {
    let version_num = target.version.0.parse::<u32>().unwrap_or(0);

    match feature {
        Feature::Records => version_num >= 312, // dataclasses since 3.7, improved in 3.12
        Feature::PatternMatching => version_num >= 310,
        Feature::Generics => version_num >= 311, // PEP 695
        Feature::NullSafety => false, // Python has None
        Feature::Defer => false, // Use context managers
        Feature::Async => version_num >= 311, // async/await since 3.5
        Feature::Closures => true,
        Feature::Traits => false, // ABCs are different
        Feature::SumTypes => version_num >= 311, // typing.Union improvements
        Feature::StringInterpolation => version_num >= 312, // f-strings since 3.6
        Feature::MultiLineStrings => true,
        Feature::RawStrings => true,
        Feature::TypeInference => false,
        Feature::OperatorOverloading => true,
        Feature::MatchExpressions => version_num >= 310,
    }
}

/// Check C/C++ feature support.
fn supports_c(feature: &Feature, target: &LangSpec) -> bool {
    let version_num = target.version.0.parse::<u32>().unwrap_or(0);

    match feature {
        Feature::Records => true, // struct
        Feature::PatternMatching => false,
        Feature::Generics => version_num >= 11, // _Generic in C11
        Feature::NullSafety => version_num >= 23, // nullptr in C23
        Feature::Defer => false,
        Feature::Async => false,
        Feature::Closures => false,
        Feature::Traits => false,
        Feature::SumTypes => false,
        Feature::StringInterpolation => false,
        Feature::MultiLineStrings => true,
        Feature::RawStrings => false,
        Feature::TypeInference => version_num >= 23, // auto in C23
        Feature::OperatorOverloading => false,
        Feature::MatchExpressions => false,
    }
}

/// Check XCL feature support.
fn supports_xcl(feature: &Feature) -> bool {
    match feature {
        Feature::Records => true,
        Feature::PatternMatching => true,
        Feature::Generics => true,
        Feature::NullSafety => true,
        Feature::Defer => true,
        Feature::Async => true,
        Feature::Closures => true,
        Feature::Traits => true,
        Feature::SumTypes => true,
        Feature::StringInterpolation => true,
        Feature::MultiLineStrings => true,
        Feature::RawStrings => true,
        Feature::TypeInference => true,
        Feature::OperatorOverloading => true,
        Feature::MatchExpressions => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_java_25_supports_records() {
        let target = LangSpec::java("25");
        assert!(supports_feature(&Feature::Records, &target));
    }

    #[test]
    fn test_java_8_no_records() {
        let target = LangSpec::java("8");
        assert!(!supports_feature(&Feature::Records, &target));
    }

    #[test]
    fn test_rust_2024_supports_defer() {
        let target = LangSpec::rust("2024");
        assert!(supports_feature(&Feature::Defer, &target));
    }

    #[test]
    fn test_python_313_supports_pattern_matching() {
        let target = LangSpec::python("313");
        assert!(supports_feature(&Feature::PatternMatching, &target));
    }

    #[test]
    fn test_xcl_supports_all() {
        let features = [
            Feature::Records, Feature::PatternMatching, Feature::Generics,
            Feature::NullSafety, Feature::Defer, Feature::Async,
        ];
        for feature in &features {
            assert!(supports_xcl(feature), "XCL should support {:?}", feature);
        }
    }
}
