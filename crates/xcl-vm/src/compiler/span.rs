//! Source code location tracking for AST nodes and parser diagnostics.
//!
//! # Overview
//!
//! The [`Span`] struct records the source file name, 1-based line number, and
//! 1-based column number associated with an AST node or token. It allows the
//! parser and VM to provide rich error diagnostics and stack traces pointing
//! directly to the source code location where an error occurred.
//!
//! # Representation
//!
//! [`Span`] stores line and column numbers as compact `u32` fields to minimize
//! memory overhead across thousands of AST nodes.
//!
//! # Examples
//!
//! ```
//! use xcl_vm::Span;
//!
//! let span = Span::new("main.xcl".to_string(), 12, 4);
//! assert_eq!(span.file, "main.xcl");
//! assert_eq!(span.line, 12);
//! assert_eq!(span.col, 4);
//! ```

#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Lightweight source location representation.
///
/// Records the source file name and 1-based line and column coordinates
/// where a token or AST node was defined.
///
/// # Examples
///
/// ```
/// use xcl_vm::Span;
///
/// let empty_span = Span::empty();
/// assert_eq!(empty_span.line, 1);
/// assert_eq!(empty_span.col, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct Span {
    /// Source file path or identifier (e.g., `"<input>"` or `"script.xcl"`).
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number (in bytes or characters).
    pub col: u32,
}

impl Span {
    /// Creates a new [`Span`] with the given file name, line, and column.
    ///
    /// # Arguments
    ///
    /// * `file` - Source file path or buffer name.
    /// * `line` - 1-based line number.
    /// * `col` - 1-based column number.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Span;
    ///
    /// let span = Span::new("example.xcl".into(), 42, 10);
    /// assert_eq!(span.line, 42);
    /// assert_eq!(span.col, 10);
    /// ```
    #[inline]
    pub fn new(file: String, line: u32, col: u32) -> Self {
        Self { file, line, col }
    }

    /// Creates an empty fallback [`Span`] pointing to line 1, column 1 of an empty file string.
    ///
    /// Useful for synthetic AST nodes or compiler-generated instructions that
    /// do not correspond directly to user source text.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Span;
    ///
    /// let span = Span::empty();
    /// assert!(span.file.is_empty());
    /// assert_eq!(span.line, 1);
    /// assert_eq!(span.col, 1);
    /// ```
    #[inline]
    pub fn empty() -> Self {
        Self {
            file: String::new(),
            line: 1,
            col: 1,
        }
    }
}
