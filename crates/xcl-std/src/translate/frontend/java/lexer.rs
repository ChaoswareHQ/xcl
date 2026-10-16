//! Token-based lexer for Java source code.
//!
//! This lexer tokenizes Java source in a single O(n) pass, producing a vector
//! of tokens that the parser consumes. This is dramatically faster than the
//! previous character-by-character approach where `skip_ws()` and `eat()` were
//! called thousands of times scanning the same characters.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Keyword(Keyword),

    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    Null,

    // Identifiers
    Ident(String),

    // Operators
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    Eq,         // ==
    Ne,         // !=
    Lt,         // <
    Gt,         // >
    Le,         // <=
    Ge,         // >=
    And,        // &&
    Or,         // ||
    Not,        // !
    Assign,     // =
    AddAssign,  // +=
    SubAssign,  // -=
    MulAssign,  // *=
    DivAssign,  // /=
    Arrow,      // ->
    DoubleColon,// ::
    Inc,        // ++
    Dec,        // --

    // Delimiters
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    Comma,      // ,
    Semicolon,  // ;
    Dot,        // .
    At,         // @
    Question,   // ?
    Colon,      // :

    // Special
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Keyword {
    Abstract, Assert, Boolean, Break, Byte, Case, Catch, Char, Class,
    Const, Continue, Default, Do, Double, Else, Enum, Extends, Final,
    Finally, Float, For, Goto, If, Implements, Import, Instanceof, Int,
    Interface, Long, Native, New, Package, Private, Protected, Public,
    Return, Short, Static, Strictfp, Super, Switch, Synchronized,
    This, Throw, Throws, Transient, Try, Var, Void, Volatile, While, Yield,
}

impl Token {
    pub fn is_keyword(&self) -> bool {
        matches!(self, Token::Keyword(_))
    }

    pub fn is_operator(&self) -> bool {
        matches!(self, Token::Plus | Token::Minus | Token::Star | Token::Slash
            | Token::Percent | Token::Eq | Token::Ne | Token::Lt | Token::Gt
            | Token::Le | Token::Ge | Token::And | Token::Or | Token::Not
            | Token::Assign | Token::AddAssign | Token::SubAssign
            | Token::MulAssign | Token::DivAssign | Token::Arrow | Token::DoubleColon
            | Token::Inc | Token::Dec)
    }

    pub fn is_delimiter(&self) -> bool {
        matches!(self, Token::LParen | Token::RParen | Token::LBrace | Token::RBrace
            | Token::LBracket | Token::RBracket | Token::Comma | Token::Semicolon
            | Token::Dot | Token::At | Token::Question | Token::Colon)
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Token::Int(_) | Token::Float(_) | Token::Str(_)
            | Token::Char(_) | Token::Bool(_) | Token::Null | Token::Ident(_))
    }
}

impl Keyword {
    pub fn as_str(&self) -> &'static str {
        match self {
            Keyword::Abstract => "abstract",
            Keyword::Assert => "assert",
            Keyword::Boolean => "boolean",
            Keyword::Break => "break",
            Keyword::Byte => "byte",
            Keyword::Case => "case",
            Keyword::Catch => "catch",
            Keyword::Char => "char",
            Keyword::Class => "class",
            Keyword::Const => "const",
            Keyword::Continue => "continue",
            Keyword::Default => "default",
            Keyword::Do => "do",
            Keyword::Double => "double",
            Keyword::Else => "else",
            Keyword::Enum => "enum",
            Keyword::Extends => "extends",
            Keyword::Final => "final",
            Keyword::Finally => "finally",
            Keyword::Float => "float",
            Keyword::For => "for",
            Keyword::Goto => "goto",
            Keyword::If => "if",
            Keyword::Implements => "implements",
            Keyword::Import => "import",
            Keyword::Instanceof => "instanceof",
            Keyword::Int => "int",
            Keyword::Interface => "interface",
            Keyword::Long => "long",
            Keyword::Native => "native",
            Keyword::New => "new",
            Keyword::Package => "package",
            Keyword::Private => "private",
            Keyword::Protected => "protected",
            Keyword::Public => "public",
            Keyword::Return => "return",
            Keyword::Short => "short",
            Keyword::Static => "static",
            Keyword::Strictfp => "strictfp",
            Keyword::Super => "super",
            Keyword::Switch => "switch",
            Keyword::Synchronized => "synchronized",
            Keyword::This => "this",
            Keyword::Throw => "throw",
            Keyword::Throws => "throws",
            Keyword::Transient => "transient",
            Keyword::Try => "try",
            Keyword::Var => "var",
            Keyword::Void => "void",
            Keyword::Volatile => "volatile",
            Keyword::While => "while",
            Keyword::Yield => "yield",
        }
    }
}

/// Fast keyword lookup using a static hash set approach.
fn lookup_keyword(s: &str) -> Option<Keyword> {
    match s {
        "abstract" => Some(Keyword::Abstract),
        "assert" => Some(Keyword::Assert),
        "boolean" => Some(Keyword::Boolean),
        "break" => Some(Keyword::Break),
        "byte" => Some(Keyword::Byte),
        "case" => Some(Keyword::Case),
        "catch" => Some(Keyword::Catch),
        "char" => Some(Keyword::Char),
        "class" => Some(Keyword::Class),
        "const" => Some(Keyword::Const),
        "continue" => Some(Keyword::Continue),
        "default" => Some(Keyword::Default),
        "do" => Some(Keyword::Do),
        "double" => Some(Keyword::Double),
        "else" => Some(Keyword::Else),
        "enum" => Some(Keyword::Enum),
        "extends" => Some(Keyword::Extends),
        "final" => Some(Keyword::Final),
        "finally" => Some(Keyword::Finally),
        "float" => Some(Keyword::Float),
        "for" => Some(Keyword::For),
        "goto" => Some(Keyword::Goto),
        "if" => Some(Keyword::If),
        "implements" => Some(Keyword::Implements),
        "import" => Some(Keyword::Import),
        "instanceof" => Some(Keyword::Instanceof),
        "int" => Some(Keyword::Int),
        "interface" => Some(Keyword::Interface),
        "long" => Some(Keyword::Long),
        "native" => Some(Keyword::Native),
        "new" => Some(Keyword::New),
        "package" => Some(Keyword::Package),
        "private" => Some(Keyword::Private),
        "protected" => Some(Keyword::Protected),
        "public" => Some(Keyword::Public),
        "return" => Some(Keyword::Return),
        "short" => Some(Keyword::Short),
        "static" => Some(Keyword::Static),
        "strictfp" => Some(Keyword::Strictfp),
        "super" => Some(Keyword::Super),
        "switch" => Some(Keyword::Switch),
        "synchronized" => Some(Keyword::Synchronized),
        "this" => Some(Keyword::This),
        "throw" => Some(Keyword::Throw),
        "throws" => Some(Keyword::Throws),
        "transient" => Some(Keyword::Transient),
        "try" => Some(Keyword::Try),
        "var" => Some(Keyword::Var),
        "void" => Some(Keyword::Void),
        "volatile" => Some(Keyword::Volatile),
        "while" => Some(Keyword::While),
        "yield" => Some(Keyword::Yield),
        _ => None,
    }
}

pub struct Lexer {
    tokens: Vec<Token>,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        let tokens = lex(source);
        Self { tokens }
    }

    pub fn into_tokens(self) -> Vec<Token> {
        self.tokens
    }
}

/// Tokenize Java source in a single O(n) pass.
pub fn lex(source: &str) -> Vec<Token> {
    let chars: &[u8] = source.as_bytes();
    let len = chars.len();
    let mut tokens = Vec::with_capacity(len / 8); // Rough estimate
    let mut pos = 0;

    while pos < len {
        // Skip whitespace
        while pos < len && chars[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= len {
            break;
        }

        // Skip single-line comments
        if pos + 1 < len && chars[pos] == b'/' && chars[pos + 1] == b'/' {
            while pos < len && chars[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        // Skip multi-line comments
        if pos + 1 < len && chars[pos] == b'/' && chars[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < len {
                if chars[pos] == b'*' && chars[pos + 1] == b'/' {
                    pos += 2;
                    break;
                }
                pos += 1;
            }
            continue;
        }

        // Skip text blocks (""")
        if pos + 2 < len && chars[pos] == b'"' && chars[pos + 1] == b'"' && chars[pos + 2] == b'"' {
            pos += 3;
            while pos + 2 < len {
                if chars[pos] == b'"' && chars[pos + 1] == b'"' && chars[pos + 2] == b'"' {
                    pos += 3;
                    break;
                }
                pos += 1;
            }
            tokens.push(Token::Str(String::new())); // Simplified: skip text block content
            continue;
        }

        // String literal
        if chars[pos] == b'"' {
            let start = pos + 1;
            pos += 1;
            while pos < len && chars[pos] != b'"' {
                if chars[pos] == b'\\' {
                    pos += 1; // Skip escaped char
                }
                pos += 1;
            }
            if pos < len {
                pos += 1; // Skip closing quote
            }
            let s = String::from_utf8_lossy(&chars[start..pos.saturating_sub(1)]).to_string();
            tokens.push(Token::Str(s));
            continue;
        }

        // Character literal
        if chars[pos] == b'\'' {
            let start = pos + 1;
            pos += 1;
            while pos < len && chars[pos] != b'\'' {
                if chars[pos] == b'\\' {
                    pos += 1;
                }
                pos += 1;
            }
            if pos < len {
                pos += 1;
            }
            let c = if start + 1 <= pos && chars[start] == b'\\' {
                match chars.get(start + 1) {
                    Some(b'n') => '\n',
                    Some(b't') => '\t',
                    Some(b'r') => '\r',
                    Some(b'\'') => '\'',
                    Some(b'\\') => '\\',
                    Some(c) => *c as char,
                    _ => '?',
                }
            } else if start + 1 <= pos {
                chars[start] as char
            } else {
                '?'
            };
            tokens.push(Token::Char(c));
            continue;
        }

        // Numeric literal
        if chars[pos].is_ascii_digit() || (chars[pos] == b'.' && pos + 1 < len && chars[pos + 1].is_ascii_digit()) {
            let start = pos;
            let mut is_float = chars[pos] == b'.';
            while pos < len && (chars[pos].is_ascii_digit() || chars[pos] == b'.') {
                pos += 1;
            }
            // Handle suffixes
            if pos < len && matches!(chars[pos], b'L' | b'l' | b'F' | b'f' | b'D' | b'd') {
                is_float = is_float || chars[pos] == b'F' || chars[pos] == b'f' || chars[pos] == b'D' || chars[pos] == b'd';
                pos += 1;
            }
            let num_str = &source[start..pos];

            // Handle hex
            if num_str.starts_with("0x") || num_str.starts_with("0X") {
                if let Ok(val) = i64::from_str_radix(&num_str[2..], 16) {
                    tokens.push(Token::Int(val));
                } else {
                    tokens.push(Token::Int(0));
                }
                continue;
            }

            if is_float || num_str.contains('.') {
                if let Ok(val) = num_str.parse::<f64>() {
                    tokens.push(Token::Float(val));
                } else {
                    tokens.push(Token::Float(0.0));
                }
            } else if let Ok(val) = num_str.parse::<i64>() {
                tokens.push(Token::Int(val));
            } else {
                tokens.push(Token::Int(0));
            }
            continue;
        }

        // Identifiers and keywords
        if chars[pos].is_ascii_alphabetic() || chars[pos] == b'_' || chars[pos] == b'$' {
            let start = pos;
            while pos < len && (chars[pos].is_ascii_alphanumeric() || chars[pos] == b'_' || chars[pos] == b'$') {
                pos += 1;
            }
            let word = &source[start..pos];

            // Check for boolean/null literals
            if word == "true" {
                tokens.push(Token::Bool(true));
            } else if word == "false" {
                tokens.push(Token::Bool(false));
            } else if word == "null" {
                tokens.push(Token::Null);
            } else if let Some(kw) = lookup_keyword(word) {
                tokens.push(Token::Keyword(kw));
            } else {
                tokens.push(Token::Ident(word.to_string()));
            }
            continue;
        }

        // Two-character operators
        if pos + 1 < len {
            let c0 = chars[pos];
            let c1 = chars[pos + 1];
            if c0 == b'=' && c1 == b'=' { tokens.push(Token::Eq); pos += 2; continue; }
            if c0 == b'!' && c1 == b'=' { tokens.push(Token::Ne); pos += 2; continue; }
            if c0 == b'<' && c1 == b'=' { tokens.push(Token::Le); pos += 2; continue; }
            if c0 == b'>' && c1 == b'=' { tokens.push(Token::Ge); pos += 2; continue; }
            if c0 == b'&' && c1 == b'&' { tokens.push(Token::And); pos += 2; continue; }
            if c0 == b'|' && c1 == b'|' { tokens.push(Token::Or); pos += 2; continue; }
            if c0 == b'+' && c1 == b'=' { tokens.push(Token::AddAssign); pos += 2; continue; }
            if c0 == b'-' && c1 == b'=' { tokens.push(Token::SubAssign); pos += 2; continue; }
            if c0 == b'*' && c1 == b'=' { tokens.push(Token::MulAssign); pos += 2; continue; }
            if c0 == b'/' && c1 == b'=' { tokens.push(Token::DivAssign); pos += 2; continue; }
            if c0 == b'-' && c1 == b'>' { tokens.push(Token::Arrow); pos += 2; continue; }
            if c0 == b':' && c1 == b':' { tokens.push(Token::DoubleColon); pos += 2; continue; }
            if c0 == b'+' && c1 == b'+' { tokens.push(Token::Inc); pos += 2; continue; }
            if c0 == b'-' && c1 == b'-' { tokens.push(Token::Dec); pos += 2; continue; }
        }

        // Single-character tokens
        match chars[pos] {
            b'+' => { tokens.push(Token::Plus); pos += 1; }
            b'-' => { tokens.push(Token::Minus); pos += 1; }
            b'*' => { tokens.push(Token::Star); pos += 1; }
            b'/' => { tokens.push(Token::Slash); pos += 1; }
            b'%' => { tokens.push(Token::Percent); pos += 1; }
            b'<' => { tokens.push(Token::Lt); pos += 1; }
            b'>' => { tokens.push(Token::Gt); pos += 1; }
            b'!' => { tokens.push(Token::Not); pos += 1; }
            b'=' => { tokens.push(Token::Assign); pos += 1; }
            b'(' => { tokens.push(Token::LParen); pos += 1; }
            b')' => { tokens.push(Token::RParen); pos += 1; }
            b'{' => { tokens.push(Token::LBrace); pos += 1; }
            b'}' => { tokens.push(Token::RBrace); pos += 1; }
            b'[' => { tokens.push(Token::LBracket); pos += 1; }
            b']' => { tokens.push(Token::RBracket); pos += 1; }
            b',' => { tokens.push(Token::Comma); pos += 1; }
            b';' => { tokens.push(Token::Semicolon); pos += 1; }
            b'.' => { tokens.push(Token::Dot); pos += 1; }
            b'@' => { tokens.push(Token::At); pos += 1; }
            b'?' => { tokens.push(Token::Question); pos += 1; }
            b':' => { tokens.push(Token::Colon); pos += 1; }
            _ => {
                // Unknown character, skip
                pos += 1;
            }
        }
    }

    tokens.push(Token::Eof);
    tokens
}

/// A simple token stream for the parser.
pub struct TokenStream {
    tokens: Vec<Token>,
    pos: usize,
}

impl TokenStream {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    pub fn peek_keyword(&self) -> Option<&Keyword> {
        match self.peek() {
            Token::Keyword(kw) => Some(kw),
            _ => None,
        }
    }

    pub fn advance(&mut self) -> Token {
        let token = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    pub fn expect(&mut self, expected: &str) -> Token {
        let token = self.advance();
        token
    }

    pub fn is(&self, token: &Token) -> bool {
        self.peek() == token
    }

    pub fn is_keyword(&self, kw: &Keyword) -> bool {
        matches!(self.peek(), Token::Keyword(k) if k == kw)
    }

    pub fn is_one_of(&self, kws: &[&Keyword]) -> bool {
        if let Token::Keyword(k) = self.peek() {
            kws.iter().any(|&kw| k == kw)
        } else {
            false
        }
    }

    pub fn is_value(&self) -> bool {
        self.peek().is_value()
    }

    pub fn is_operator(&self) -> bool {
        self.peek().is_operator()
    }

    pub fn is_delimiter(&self) -> bool {
        self.peek().is_delimiter()
    }

    pub fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    /// Return the current position in the token stream
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Check if the current token is an identifier or keyword (for type reading)
    pub fn is_type_start(&self) -> bool {
        matches!(self.peek(), Token::Ident(_) | Token::Keyword(_))
    }

    /// Read a type name (identifier, possibly with generics)
    pub fn read_type(&mut self) -> String {
        let name = match self.advance() {
            Token::Ident(s) => s,
            Token::Keyword(kw) => kw.as_str().to_string(),
            _ => String::new(),
        };

        // Handle generics
        if self.is(&Token::Lt) {
            self.advance(); // eat '<'
            let mut depth = 1usize;
            let mut generic = String::new();
            while !self.at_eof() && depth > 0 {
                let t = self.advance();
                if matches!(t, Token::Lt) { depth += 1; }
                if matches!(t, Token::Gt) { depth -= 1; }
                if depth > 0 {
                    if !generic.is_empty() { generic.push(' '); }
                    generic.push_str(&token_to_string(&t));
                }
            }
            format!("{}<{}>", name, generic)
        } else {
            name
        }
    }

    /// Read an identifier, returning empty string if not an ident
    pub fn read_ident(&mut self) -> String {
        match self.advance() {
            Token::Ident(s) => s,
            Token::Keyword(kw) => kw.as_str().to_string(),
            _ => String::new(),
        }
    }

    /// Skip tokens until we find a semicolon, respecting nesting
    pub fn skip_to_semicolon(&mut self) {
        let mut depth_paren = 0usize;
        let mut depth_brace = 0usize;

        loop {
            let token = self.advance();
            match &token {
                Token::LParen => depth_paren += 1,
                Token::RParen => depth_paren = depth_paren.saturating_sub(1),
                Token::LBrace => depth_brace += 1,
                Token::RBrace => depth_brace = depth_brace.saturating_sub(1),
                Token::Semicolon if depth_paren == 0 && depth_brace == 0 => return,
                Token::RBrace if depth_paren == 0 && depth_brace == 0 => return,
                Token::Eof => return,
                _ => {}
            }
        }
    }

    /// Skip tokens until we find a closing brace, respecting nesting
    pub fn skip_to_rbrace(&mut self) {
        let mut depth = 1usize;
        while !self.at_eof() {
            let token = self.advance();
            match &token {
                Token::LBrace => depth += 1,
                Token::RBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 { return; }
                }
                Token::Eof => return,
                _ => {}
            }
        }
    }

    /// Skip tokens until we find a closing paren, respecting nesting
    pub fn skip_to_rparen(&mut self) {
        let mut depth = 1usize;
        while !self.at_eof() {
            let token = self.advance();
            match &token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 { return; }
                }
                Token::Eof => return,
                _ => {}
            }
        }
    }

    /// Skip an annotation
    pub fn skip_annotation(&mut self) {
        // Skip the annotation name
        if self.is_type_start() {
            self.advance();
        }
        // Skip annotation arguments if present
        if self.is(&Token::LParen) {
            self.advance(); // consume '('
            self.skip_to_rparen();
        }
        // Skip annotation body if present (e.g., @interface style)
        if self.is(&Token::LBrace) {
            self.advance(); // consume '{'
            self.skip_to_rbrace();
        }
    }

    /// Skip modifiers (public, private, protected, static, final, etc.)
    pub fn skip_modifiers(&mut self) -> bool {
        let mut has_static = false;
        let modifiers = [
            &Keyword::Public, &Keyword::Private, &Keyword::Protected,
            &Keyword::Static, &Keyword::Final, &Keyword::Abstract,
            &Keyword::Synchronized, &Keyword::Volatile, &Keyword::Transient,
            &Keyword::Native, &Keyword::Strictfp,
        ];

        loop {
            if self.is_one_of(&modifiers) {
                let token = self.advance();
                if let Token::Keyword(Keyword::Static) = token {
                    has_static = true;
                }
            } else if self.is(&Token::At) {
                self.advance();
                self.skip_annotation();
            } else {
                break;
            }
        }

        has_static
    }
}

/// Convert a token to a string representation (for generic type parsing)
fn token_to_string(token: &Token) -> String {
    match token {
        Token::Ident(s) => s.clone(),
        Token::Keyword(kw) => kw.as_str().to_string(),
        Token::Int(n) => n.to_string(),
        Token::Float(f) => f.to_string(),
        Token::Str(s) => s.clone(),
        Token::Bool(b) => b.to_string(),
        Token::Char(c) => c.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_simple() {
        let tokens = lex("public class Main { }");
        assert_eq!(tokens[0], Token::Keyword(Keyword::Public));
        assert_eq!(tokens[1], Token::Keyword(Keyword::Class));
        assert_eq!(tokens[2], Token::Ident("Main".to_string()));
        assert_eq!(tokens[3], Token::LBrace);
        assert_eq!(tokens[4], Token::RBrace);
        assert_eq!(tokens[5], Token::Eof);
    }

    #[test]
    fn test_lex_literals() {
        let tokens = lex("42 3.14 true false null \"hello\" 'x'");
        assert_eq!(tokens[0], Token::Int(42));
        assert_eq!(tokens[1], Token::Float(3.14));
        assert_eq!(tokens[2], Token::Bool(true));
        assert_eq!(tokens[3], Token::Bool(false));
        assert_eq!(tokens[4], Token::Null);
        assert_eq!(tokens[5], Token::Str("hello".to_string()));
        assert_eq!(tokens[6], Token::Char('x'));
    }

    #[test]
    fn test_lex_operators() {
        let tokens = lex("+ - * / == != && || -> ::");
        assert_eq!(tokens[0], Token::Plus);
        assert_eq!(tokens[1], Token::Minus);
        assert_eq!(tokens[2], Token::Star);
        assert_eq!(tokens[3], Token::Slash);
        assert_eq!(tokens[4], Token::Eq);
        assert_eq!(tokens[5], Token::Ne);
        assert_eq!(tokens[6], Token::And);
        assert_eq!(tokens[7], Token::Or);
        assert_eq!(tokens[8], Token::Arrow);
        assert_eq!(tokens[9], Token::DoubleColon);
    }

    #[test]
    fn test_lex_comments() {
        let tokens = lex("// comment\n/* block */ int x;");
        // Should skip comments and produce: int x ;
        assert_eq!(tokens[0], Token::Keyword(Keyword::Int));
        assert_eq!(tokens[1], Token::Ident("x".to_string()));
        assert_eq!(tokens[2], Token::Semicolon);
    }

    #[test]
    fn test_lex_lambda() {
        let tokens = lex("item -> item.getTitle()");
        assert_eq!(tokens[0], Token::Ident("item".to_string()));
        assert_eq!(tokens[1], Token::Arrow);
        assert_eq!(tokens[2], Token::Ident("item".to_string()));
        assert_eq!(tokens[3], Token::Dot);
        assert_eq!(tokens[4], Token::Ident("getTitle".to_string()));
        assert_eq!(tokens[5], Token::LParen);
        assert_eq!(tokens[6], Token::RParen);
    }
}
