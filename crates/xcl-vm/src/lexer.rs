//! Lexical analyzer (tokenizer) for XCL source code.
//!
//! # Overview
//!
//! The lexer transforms raw UTF-8 source text into a stream of structured [`Token`](crate::lexer::Token) instances
//! tagged with precise source location metadata ([`Span`]).
//!
//! Built upon the [`logos`] regular expression lexer generator, tokenization is
//! zero-allocation for punctuation and keywords, reserving memory only for token
//! text slices and location tracking.
//!
//! # Lexical Grammar
//!
//! * **Identifiers**:
//!   - Variable identifiers begin with `$` (e.g., `$count`, `$user_id`).
//!   - Plain identifiers represent type names, function names, and keywords (e.g., `Point`, `len`, `map`).
//! * **Literals**:
//!   - Integers: decimal (`42`), hexadecimal (`0x2A`), binary (`0b101010`), octal (`0o52`), with optional underscores (`1_000_000`).
//!   - Floats: standard decimal notation (`3.14159`, `0.5`).
//!   - Booleans: `true`, `false`.
//!   - Nil: `none`.
//!   - Strings:
//!     - Double-quoted: `"hello \n world"` with escape sequences (`\n`, `\t`, `\r`, `\\`, `\"`).
//!     - Raw strings: `r"C:\path\to\file"` without escape processing.
//!     - Multiline strings: `"""line 1\nline 2"""`.
//! * **Comments**:
//!   - Single-line comments begin with `#` and extend to end of line: `# this is a comment`.
//! * **Pipelining and Operators**:
//!   - Pipelines: `|>` (pipeline), `|` (lambda pipe).
//!   - Arithmetic: `+`, `-`, `*`, `/`, `%`.
//!   - Comparisons: `==`, `!=`, `<`, `>`, `<=`, `>=`.
//!   - Assignment & Compound: `=`, `+=`, `-=`, `*=`, `/=`, `??=`, `:=`.
//!   - Null coalescing: `??`.
//!   - Safe navigation: `?.`.
//!
//! # Examples
//!
//! Tokenizing an XCL expression:
//!
//! ```
//! use xcl_vm::lexer::{lex, Token};
//!
//! let tokens = lex("$x = 10 + 20;", "script.xcl");
//! assert_eq!(tokens[0].token, Token::Ident);
//! assert_eq!(tokens[0].text, "$x");
//! assert_eq!(tokens[1].token, Token::Assign);
//! assert_eq!(tokens[2].token, Token::Int);
//! assert_eq!(tokens[2].text, "10");
//! assert_eq!(tokens[3].token, Token::Add);
//! assert_eq!(tokens[4].token, Token::Int);
//! assert_eq!(tokens[4].text, "20");
//! assert_eq!(tokens[5].token, Token::Semi);
//! ```

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::span::Span;
use logos::Logos;

/// Tokens produced by the XCL lexer.
///
/// Generated with [`logos::Logos`] for fast deterministic finite automaton (DFA) matching.
#[derive(Debug, Clone, PartialEq, Eq, Logos)]
pub enum Token {
    /// Whitespace (skipped automatically by the lexer).
    #[regex(r"[ \t\r\n]+", logos::skip)]
    /// Single-line comment starting with `#` (skipped automatically by the lexer).
    #[regex(r"#[^\n]*", logos::skip, allow_greedy = true)]
    /// `if` keyword.
    #[token("if")]
    If,
    /// `else` keyword.
    #[token("else")]
    Else,
    /// `for` keyword.
    #[token("for")]
    For,
    /// `in` keyword.
    #[token("in")]
    In,
    /// `while` keyword.
    #[token("while")]
    While,
    /// `break` keyword.
    #[token("break")]
    Break,
    /// `continue` keyword.
    #[token("continue")]
    Continue,
    /// `return` keyword.
    #[token("return")]
    Return,
    /// `true` boolean literal.
    #[token("true")]
    True,
    /// `false` boolean literal.
    #[token("false")]
    False,
    /// `none` nil literal.
    #[token("none")]
    None_,

    /// `and` logical conjunction keyword.
    #[token("and")]
    And,
    /// `or` logical disjunction keyword.
    #[token("or")]
    Or,
    /// `not` logical inversion keyword.
    #[token("not")]
    Not,
    /// `is` type test/implementation keyword.
    #[token("is")]
    Is,
    /// `as` type cast keyword.
    #[token("as")]
    As,

    /// `fn` function definition keyword.
    #[token("fn")]
    Fn,
    /// `bool` type keyword.
    #[token("bool")]
    Bool,
    /// `any` dynamic type keyword.
    #[token("any")]
    Any,

    /// Variable identifier prefixed with `$` (e.g. `$total`, `$x_1`).
    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,
    /// Plain identifier without `$` prefix (e.g. function or type name).
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    IdentPlain,

    /// Hexadecimal integer literal (e.g. `0xFF`, `0x10_00`).
    #[regex(r"0x[0-9a-fA-F_]+")]
    IntHex,
    /// Binary integer literal (e.g. `0b1010`, `0b1111_0000`).
    #[regex(r"0b[01_]+")]
    IntBin,
    /// Octal integer literal (e.g. `0o755`, `0o644`).
    #[regex(r"0o[0-7_]+")]
    IntOct,
    /// Floating-point number literal (e.g. `3.1415`, `0.0`).
    #[regex(r"\d[\d_]*\.\d[\d_]*")]
    Float,
    /// Decimal integer literal (e.g. `42`, `10_000`).
    #[regex(r"\d[\d_]*")]
    Int,

    /// Multi-line triple-quoted string literal (e.g. `"""line1\nline2"""`).
    #[regex(r#""""[\s\S]*?""""#)]
    MultiLineString,
    /// Raw unescaped string literal (e.g. `r"raw\path"`).
    #[regex(r##"r\"[^\"]*\""##)]
    RawString,
    /// Standard double-quoted string literal (e.g. `"hello world"`).
    #[regex(r##""([^"\\]|\\.)*""##)]
    String,

    /// Logical OR operator `||`.
    #[token("||")]
    OrOp,
    /// Single pipe `|` (used in lambda definitions or sum variants).
    #[token("|")]
    Pipe,
    /// Logical AND operator `&&`.
    #[token("&&")]
    AndOp,
    /// Equality operator `==`.
    #[token("==")]
    Eq,
    /// Inequality operator `!=`.
    #[token("!=")]
    NotEq,
    /// Less than or equal operator `<=`.
    #[token("<=")]
    Le,
    /// Greater than or equal operator `>=`.
    #[token(">=")]
    Ge,
    /// Less than operator `<`.
    #[token("<")]
    Lt,
    /// Greater than operator `>`.
    #[token(">")]
    Gt,
    /// Addition assignment operator `+=`.
    #[token("+=")]
    AddAssign,
    /// Subtraction assignment operator `-=`.
    #[token("-=")]
    SubAssign,
    /// Multiplication assignment operator `*=`.
    #[token("*=")]
    MulAssign,
    /// Division assignment operator `/=`.
    #[token("/=")]
    DivAssign,
    /// Null coalescing assignment operator `??=`.
    #[token("??=")]
    CoalesceAssign,
    /// Null coalescing operator `??`.
    #[token("??")]
    Coalesce,
    /// Return type arrow `->`.
    #[token("->")]
    RArrow,
    /// Match / function arrow `=>`.
    #[token("=>")]
    Arrow,
    /// Pipeline operator `|>`.
    #[token("|>")]
    Pipeline,
    /// Range operator `..`.
    #[token("..")]
    Range,
    /// Addition operator `+`.
    #[token("+")]
    Add,
    /// Subtraction / negation operator `-`.
    #[token("-")]
    Sub,
    /// Multiplication operator `*`.
    #[token("*")]
    Mul,
    /// Division operator `/`.
    #[token("/")]
    Div,
    /// Modulo operator `%`.
    #[token("%")]
    Mod,
    /// Assignment operator `=`.
    #[token("=")]
    Assign,
    /// Logical NOT operator `!`.
    #[token("!")]
    Bang,
    /// Walrus / declaration binding operator `:=`.
    #[token(":=")]
    Binding,

    /// Opening parenthesis `(`.
    #[token("(")]
    LParen,
    /// Closing parenthesis `)`.
    #[token(")")]
    RParen,
    /// Opening bracket `[`.
    #[token("[")]
    LBracket,
    /// Closing bracket `]`.
    #[token("]")]
    RBracket,
    /// Opening brace `{`.
    #[token("{")]
    LBrace,
    /// Closing brace `}`.
    #[token("}")]
    RBrace,
    /// Comma `,`.
    #[token(",")]
    Comma,
    /// Semicolon `;`.
    #[token(";")]
    Semi,
    /// Colon `:`.
    #[token(":")]
    Colon,
    /// Member access dot `.`.
    #[token(".")]
    Dot,
    /// Optional chaining dot `?.`.
    #[token("?.")]
    OptionalDot,
    /// Question mark `?` (error unwrapping or optional types).
    #[token("?")]
    Question,
}

/// A token enriched with its raw source text, location span, and byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenWithText {
    /// The token kind recognized by the lexer.
    pub token: Token,
    /// The exact substring from the source file corresponding to this token.
    pub text: String,
    /// Diagnostic source location ([`Span`]) containing file, line, and column.
    pub span: Span,
    /// Zero-based byte offset in the input source buffer.
    pub byte_offset: usize,
}

/// Precomputed line-start offsets for O(1) / O(log n) offset-to-line conversion.
/// Built once during lexing to avoid O(n) per-token scanning.
struct LineMap {
    offsets: Vec<usize>,
}

impl LineMap {
    #[inline]
    fn new(source: &str) -> Self {
        let mut offsets = Vec::with_capacity(source.len() / 40 + 1);
        offsets.push(0);
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                let next = i + 1;
                if next < source.len() {
                    offsets.push(next);
                }
            }
        }
        Self { offsets }
    }

    /// Convert byte offset to (line, col) in O(log n) time using binary search.
    #[inline]
    fn offset_to_line_col(&self, offset: usize) -> (u32, u32) {
        // Binary search for the largest line start <= offset
        let line = match self.offsets.binary_search(&offset) {
            Ok(i) => (i + 1) as u32,
            Err(i) => {
                // i is the insertion point; the line before insertion is our line
                if i == 0 { 1u32 } else { i as u32 }
            }
        };
        // Column is offset minus the start of the line
        let line_idx = if line > 0 { (line - 1) as usize } else { 0 };
        let line_start = *self.offsets.get(line_idx).unwrap_or(&0);
        let col = if offset >= line_start {
            (offset - line_start + 1) as u32
        } else {
            1u32
        };
        (line, col)
    }
}

/// Tokenizes an XCL source string into a sequence of [`TokenWithText`] elements.
///
/// Line numbers and column numbers are computed using a fast line-map lookup table.
/// Any invalid or unrecognized characters are gracefully skipped.
///
/// # Arguments
///
/// * `source` - The complete source code to tokenize.
/// * `file` - The file path or buffer name to associate with the tokens.
///
/// # Returns
///
/// A vector of [`TokenWithText`] structs containing tokens, source text, spans, and offsets.
///
/// # Examples
///
/// ```
/// use xcl_vm::lexer::{lex, Token};
///
/// let source = "fn add(a, b) { a + b }";
/// let tokens = lex(source, "math.xcl");
///
/// assert_eq!(tokens[0].token, Token::Fn);
/// assert_eq!(tokens[1].text, "add");
/// assert_eq!(tokens[2].token, Token::LParen);
/// ```
#[inline]
pub fn lex(source: &str, file: &str) -> Vec<TokenWithText> {
    let line_map = LineMap::new(source);
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::with_capacity(source.len() / 6);

    while let Some(tok) = lexer.next() {
        let sp = lexer.span();
        let (start, end) = (sp.start, sp.end);
        let (line, col) = line_map.offset_to_line_col(start);
        let diag_span = Span::new(file.to_string(), line, col);

        let Ok(token) = tok else { continue };
        let text = source[start..end].to_string();
        tokens.push(TokenWithText {
            token,
            text,
            span: diag_span,
            byte_offset: start,
        });
    }

    tokens
}

/// Unescapes and extracts the inner string value from raw literal text.
///
/// Handles three types of string syntax:
/// 1. Multiline triple-quoted strings: `"""content"""` -> `"content"`
/// 2. Raw strings: `r"content"` -> `"content"` (no escape decoding)
/// 3. Standard double-quoted strings: `"content\n"` -> `"content\n"` (with `\n`, `\t`, `\r`, `\\`, `\"` decoded)
///
/// # Arguments
///
/// * `source` - The raw string literal text including surrounding quotes.
///
/// # Returns
///
/// The decoded inner string.
///
/// # Examples
///
/// ```
/// use xcl_vm::lexer::extract_string;
///
/// assert_eq!(extract_string(r#""hello\nworld""#), "hello\nworld");
/// assert_eq!(extract_string(r#"r"raw\path""#), r#"raw\path"#);
/// assert_eq!(extract_string(r#""""multiline text""""#), "multiline text");
/// ```
#[inline]
pub fn extract_string(source: &str) -> String {
    let s = source.trim();
    if s.starts_with("\"\"\"") && s.ends_with("\"\"\"") {
        return s[3..s.len() - 3].to_string();
    }
    if s.starts_with("r\"") && s.ends_with('"') {
        return s[2..s.len() - 1].to_string();
    }
    if s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(ch);
            }
        }
        return out;
    }
    s.to_string()
}
