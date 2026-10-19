//! Source code to AST compilation: lexer, parser, AST nodes, and source spans.
//!
//! This module handles the first two phases of the XCL compilation pipeline:
//! 1. **Lexical analysis** — transforms raw UTF-8 source text into tokens
//! 2. **Parsing** — converts tokens into an Abstract Syntax Tree

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use ast::{AstNode, BinaryOperator};
pub use lexer::lex;
pub use parser::parse;
pub use span::Span;
