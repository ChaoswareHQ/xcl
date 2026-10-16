//! Language version specification for the translator.
//!
//! Defines `Language`, `Version`, and `LangSpec` types used to identify
//! source and target languages with their specific versions.
//!
//! Every backend and frontend declares its `LangSpec`. Version strings
//! are exact — no fuzzy matching.

use std::fmt;

/// Supported programming languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Language {
    /// XCL — the source language
    #[default]
    Xcl,
    /// Java
    Java,
    /// Rust
    Rust,
    /// Python
    Python,
    /// C
    C,
    /// C++
    Cpp,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Language::Xcl => write!(f, "xcl"),
            Language::Java => write!(f, "java"),
            Language::Rust => write!(f, "rust"),
            Language::Python => write!(f, "python"),
            Language::C => write!(f, "c"),
            Language::Cpp => write!(f, "cpp"),
        }
    }
}

/// A language version identifier.
///
/// Versions use the language's own scheme:
/// - Java: "17", "21", "25"
/// - Rust: "2021", "2024"
/// - Python: "311", "312", "313"
/// - C: "99", "11", "17", "23"
/// - C++: "11", "14", "17", "20", "23"
/// - XCL: "1" (current)
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Version(pub String);

impl Version {
    /// Create a new version from a string.
    pub fn new(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Specifies a language and its version.
///
/// This is the primary key for looking up frontends and backends
/// in the registry.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct LangSpec {
    pub language: Language,
    pub version: Version,
}

impl LangSpec {
    /// Create an XCL language spec.
    pub fn xcl(version: &str) -> Self {
        Self {
            language: Language::Xcl,
            version: Version::new(version),
        }
    }

    /// Create a Java language spec.
    pub fn java(version: &str) -> Self {
        Self {
            language: Language::Java,
            version: Version::new(version),
        }
    }

    /// Create a Rust language spec.
    pub fn rust(version: &str) -> Self {
        Self {
            language: Language::Rust,
            version: Version::new(version),
        }
    }

    /// Create a Python language spec.
    pub fn python(version: &str) -> Self {
        Self {
            language: Language::Python,
            version: Version::new(version),
        }
    }

    /// Create a C language spec.
    pub fn c(version: &str) -> Self {
        Self {
            language: Language::C,
            version: Version::new(version),
        }
    }

    /// Create a C++ language spec.
    pub fn cpp(version: &str) -> Self {
        Self {
            language: Language::Cpp,
            version: Version::new(version),
        }
    }
}

impl fmt::Display for LangSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.language, self.version)
    }
}
