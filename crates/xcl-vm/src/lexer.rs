#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::span::Span;
use logos::Logos;

#[derive(Debug, Clone, PartialEq, Logos)]
pub enum Token {
    #[regex(r"[ \t\r\n]+", logos::skip)]
    #[regex(r"#[^\n]*", logos::skip, allow_greedy = true)]
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("while")]
    While,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("return")]
    Return,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("none")]
    None_,

    #[token("and")]
    And,
    #[token("or")]
    Or,
    #[token("not")]
    Not,
    #[token("is")]
    Is,
    #[token("as")]
    As,

    #[token("fn")]
    Fn,
    #[token("bool")]
    Bool,
    #[token("any")]
    Any,

    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    IdentPlain,

    #[regex(r"0x[0-9a-fA-F_]+")]
    IntHex,
    #[regex(r"0b[01_]+")]
    IntBin,
    #[regex(r"0o[0-7_]+")]
    IntOct,
    #[regex(r"\d[\d_]*\.\d[\d_]*")]
    Float,
    #[regex(r"\d[\d_]*")]
    Int,

    #[regex(r#""""[\s\S]*?""""#)]
    MultiLineString,
    #[regex(r##"r\"[^\"]*\""##)]
    RawString,
    #[regex(r##""([^"\\]|\\.)*""##)]
    String,

    #[token("||")]
    OrOp,
    #[token("|")]
    Pipe,
    #[token("&&")]
    AndOp,
    #[token("==")]
    Eq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("+=")]
    AddAssign,
    #[token("-=")]
    SubAssign,
    #[token("*=")]
    MulAssign,
    #[token("/=")]
    DivAssign,
    #[token("??=")]
    CoalesceAssign,
    #[token("??")]
    Coalesce,
    #[token("->")]
    RArrow,
    #[token("=>")]
    Arrow,
    #[token("|>")]
    Pipeline,
    #[token("..")]
    Range,
    #[token("+")]
    Add,
    #[token("-")]
    Sub,
    #[token("*")]
    Mul,
    #[token("/")]
    Div,
    #[token("%")]
    Mod,
    #[token("=")]
    Assign,
    #[token("!")]
    Bang,
    #[token(":=")]
    Binding,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,
    #[token("?.")]
    OptionalDot,
    #[token("?")]
    Question,
}

#[derive(Clone, Debug)]
pub struct TokenWithText {
    pub token: Token,
    pub text: String,
    pub span: Span,
    pub byte_offset: usize,
}

/// Precomputed line-start offsets for O(1) offset-to-line conversion.
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
        // Binary search for the line
        let line = match self.offsets.binary_search(&offset) {
            Ok(0) | Err(0) => 1u32,
            Ok(i) | Err(i) => (i + 1) as u32,
        };
        // Column is offset minus the start of the line
        let line_start = *self.offsets.get((line - 1) as usize).unwrap_or(&0);
        let col = (offset - line_start + 1) as u32;
        (line, col)
    }
}

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
