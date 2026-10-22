//! Type expressions.
//!
//! `parse_type` reads a type position (`name: T`, a record field, a parameter)
//! into the string the AST stores: a primitive keyword or a named type, with
//! the optional punctuation the grammar allows around it.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_type(&mut self) -> String {
        let base = match self.current().map(|t| &t.token) {
            Some(Token::Str) => {
                self.consume();
                "str".to_string()
            }
            Some(Token::I8) => {
                self.consume();
                "i8".to_string()
            }
            Some(Token::I16) => {
                self.consume();
                "i16".to_string()
            }
            Some(Token::I32) => {
                self.consume();
                "i32".to_string()
            }
            Some(Token::I64) => {
                self.consume();
                "i64".to_string()
            }
            Some(Token::U8) => {
                self.consume();
                "u8".to_string()
            }
            Some(Token::U16) => {
                self.consume();
                "u16".to_string()
            }
            Some(Token::U32) => {
                self.consume();
                "u32".to_string()
            }
            Some(Token::U64) => {
                self.consume();
                "u64".to_string()
            }
            Some(Token::F32) => {
                self.consume();
                "f32".to_string()
            }
            Some(Token::F64) => {
                self.consume();
                "f64".to_string()
            }
            _ => self.consume().text.clone(),
        };
        if matches!(self.current().map(|t| &t.token), Some(Token::Lt)) {
            self.consume();
            let mut args = Vec::new();
            args.push(self.parse_type());
            while self.at(Token::Comma) {
                self.consume();
                args.push(self.parse_type());
            }
            self.expect(Token::Gt);
            return format!("{}<{}>", base, args.join(", "));
        }
        if base == "fn" && self.at(Token::LParen) {
            self.consume();
            let mut ps = Vec::new();
            while !self.at(Token::RParen) && !self.at_eof() {
                ps.push(self.parse_type());
                if self.at(Token::Comma) {
                    self.consume();
                }
            }
            self.expect(Token::RParen);
            if self.at(Token::RArrow) {
                self.consume();
                let r = self.parse_type();
                return format!("fn({}) -> {}", ps.join(", "), r);
            }
            return format!("fn({})", ps.join(", "));
        }
        base
    }
}
