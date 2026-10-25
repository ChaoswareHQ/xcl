//! Source code to AST compilation: lexer, parser, AST nodes, and source spans.
//!
//! This module handles the first two phases of the XCL compilation pipeline:
//! 1. **Lexical analysis** — transforms raw UTF-8 source text into tokens
//! 2. **Parsing** — converts tokens into an Abstract Syntax Tree
//!
//! The parser is split by grammar area under `parser/`; see its module doc for
//! the file map. The back end (AST → bytecode) lives in `bytecode/codegen/`.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use ast::{AstNode, BinaryOperator};
pub use lexer::lex;
pub use parser::{PARSE_ERROR_CODES, ParseError, ParseOutput, parse, parse_recover};
pub use span::Span;
