//! Primary expressions and argument lookaheads.
//!
//! `primary` handles the atoms: literals, identifiers, arrays, maps, grouping,
//! and the block-lambda forms. The lookaheads decide whether the next token
//! starts a positional argument of an enclosing call.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn primary(&mut self) -> AstNode {
        let t = self.consume();
        let span = t.span.clone();
        match t.token {
            Token::Int => AstNode::Int {
                span,
                value: t.text.trim().parse().unwrap_or(0),
            },
            Token::Float => AstNode::Float {
                span,
                value: t.text.trim().parse().unwrap_or(0.0),
            },
            Token::IntHex => {
                let hex = t.text.trim_start_matches("0x").trim_start_matches("0X");
                AstNode::Int {
                    span,
                    value: i64::from_str_radix(hex, 16).unwrap_or(0),
                }
            }
            Token::IntBin => {
                let bin = t.text.trim_start_matches("0b").trim_start_matches("0B");
                AstNode::Int {
                    span,
                    value: i64::from_str_radix(bin, 2).unwrap_or(0),
                }
            }
            Token::IntOct => {
                let oct = t.text.trim_start_matches("0o");
                AstNode::Int {
                    span,
                    value: i64::from_str_radix(oct, 8).unwrap_or(0),
                }
            }
            Token::String | Token::RawString | Token::MultiLineString => AstNode::Str {
                span,
                value: extract_string(&t.text),
            },
            Token::True => AstNode::Bool { span, value: true },
            Token::False => AstNode::Bool { span, value: false },
            Token::None_ => AstNode::Nil { span },

            Token::Ident => {
                let name = t.text.trim_start_matches('$').to_string();
                AstNode::Ident { span, name }
            }

            Token::SpecialVar => {
                let (kind, index) = match t.text.as_str() {
                    "$0" => (SpecialVarKind::ScriptName, 0),
                    "$#" => (SpecialVarKind::ArgCount, 0),
                    "$@" => (SpecialVarKind::ArgsArray, 0),
                    "$*" => (SpecialVarKind::ArgsJoined, 0),
                    "$self" => (SpecialVarKind::SelfValue, 0),
                    "$super" => (SpecialVarKind::SuperValue, 0),
                    text => match text[1..].parse::<u32>() {
                        Ok(index) => {
                            if index == 0 {
                                (SpecialVarKind::ScriptName, 0)
                            } else {
                                (SpecialVarKind::Positional, index)
                            }
                        }
                        Err(_) => {
                            let msg = format!("invalid positional argument `{}`", text);
                            self.error("E0007", t.span.clone(), msg);
                            (SpecialVarKind::ScriptName, 0)
                        }
                    },
                };
                AstNode::SpecialVar { span, kind, index }
            }

            Token::UnsupportedSpecialVar => {
                let msg = format!(
                    "shell special variable `{}` is not supported in XCL",
                    t.text
                );
                self.error("E0006", t.span.clone(), msg);
                AstNode::Nil { span }
            }

            Token::IdentPlain => {
                let name = t.text.clone();
                // Check if this looks like a function definition: `name( ... ) {` or `name( ... ) ->`
                // If so, don't parse as a call — let the caller handle it
                if self.at(Token::LParen) {
                    if self.lookahead_fn_def_from_paren() {
                        return AstNode::Ident { span, name };
                    }
                    // `name(...)` (no space) is not call syntax: parentheses
                    // after a name delimit parameters in a definition. Calls are
                    // space-separated. A *spaced* `name (expr)` is a grouped
                    // argument and is handled by the call loop below.
                    if self.adjacent_to_previous() {
                        let span = self.span();
                        let msg = format!(
                            "`{}(...)` is not call syntax — calls use space-separated \
                             arguments (`{} arg1 arg2`); parentheses are for definitions",
                            name, name
                        );
                        self.error("E0005", span.clone(), msg);
                        // Consume the `(` to advance past the error
                        self.consume();
                        // Return a plain ident so parsing can continue
                        return AstNode::Ident { span, name };
                    }
                }
                // Space-separated domain-style call: `domain func arg`
                let mut args = Vec::new();
                while self.looks_like_arg() {
                    // In a control-flow header the brace belongs to the body.
                    if self.at(Token::LBrace) && !self.brace_arg_allowed {
                        break;
                    }
                    if let Some(items) = self.try_expansion(&args) {
                        return AstNode::DomainCall {
                            span: span.clone(),
                            callee: Box::new(AstNode::Ident {
                                span: span.clone(),
                                name,
                            }),
                            args,
                            items,
                        };
                    }
                    args.push(self.call_arg());
                }
                while self.at(Token::Comma) {
                    self.consume();
                    if self.at(Token::LBrace) && !self.brace_arg_allowed {
                        break;
                    }
                    if let Some(items) = self.try_expansion(&args) {
                        return AstNode::DomainCall {
                            span: span.clone(),
                            callee: Box::new(AstNode::Ident {
                                span: span.clone(),
                                name,
                            }),
                            args,
                            items,
                        };
                    }
                    // An explicit comma continues the list across a newline.
                    if self.starts_positional_arg() {
                        args.push(self.call_arg());
                    }
                }
                if let Some(items) = self.try_expansion(&args) {
                    return AstNode::DomainCall {
                        span: span.clone(),
                        callee: Box::new(AstNode::Ident {
                            span: span.clone(),
                            name,
                        }),
                        args,
                        items,
                    };
                }
                if args.is_empty() {
                    AstNode::Ident { span, name }
                } else {
                    AstNode::Call {
                        span: span.clone(),
                        callee: Box::new(AstNode::Ident { span, name }),
                        args,
                    }
                }
            }

            Token::LBracket => {
                let mut elements = Vec::new();
                while !self.at(Token::RBracket) && !self.at_eof() {
                    elements.push(self.expr());
                    if self.at(Token::Comma) || self.at(Token::Semi) {
                        self.consume();
                    }
                }
                self.expect(Token::RBracket);
                AstNode::ArrayLit { span, elements }
            }

            Token::LBrace => {
                // Lookahead: if the first meaningful token inside { is followed by `:`, it's a dict literal.
                // Otherwise, it's a block-lambda (zero-param closure).
                if self.lookahead_dict_literal() {
                    let mut entries = Vec::new();
                    while !self.at(Token::RBrace) && !self.at_eof() {
                        if self.at(Token::Semi) || self.at(Token::Comma) {
                            self.consume();
                            continue;
                        }
                        let key = self.consume().text.clone();
                        if self.at(Token::Colon) {
                            self.consume();
                            let value = self.expr();
                            entries.push((key, value));
                        } else {
                            entries.push((
                                key.clone(),
                                AstNode::Ident {
                                    span: span.clone(),
                                    name: key,
                                },
                            ));
                        }
                    }
                    self.expect(Token::RBrace);
                    AstNode::DictLit { span, entries }
                } else {
                    // Block-lambda: { stmt1; stmt2; ... }
                    let mut statements = Vec::new();
                    while !self.at(Token::RBrace) && !self.at_eof() {
                        if self.at(Token::Semi) {
                            self.consume();
                            continue;
                        }
                        statements.push(self.statement());
                    }
                    self.expect(Token::RBrace);
                    AstNode::Lambda {
                        span: span.clone(),
                        params: vec![],
                        body: Box::new(AstNode::Block { span, statements }),
                    }
                }
            }

            Token::LParen => {
                // Check for arrow lambda: ($x, $y) => expr
                // First, try to parse params: a list of $-identifiers separated by commas
                let save_pos = self.pos;
                let params = self.try_parse_lambda_params();
                if let Some(params) = params {
                    // Successfully parsed params and consumed RParen
                    // Now expect =>
                    if self.at(Token::Arrow) {
                        self.consume(); // consume =>
                        let body = Box::new(self.expr());
                        return AstNode::Lambda {
                            span: span.clone(),
                            params,
                            body,
                        };
                    } else {
                        // Not a lambda, rewind and parse as regular parenthesized expr
                        self.pos = save_pos;
                        let e = self.expr();
                        self.expect(Token::RParen);
                        return e;
                    }
                } else {
                    // Not lambda params, parse as regular parenthesized expr
                    self.pos = save_pos;
                    let start = self.pos;
                    let e = self.expr();
                    // `($f 1)` is a call of the value `$f`.
                    let e = self.attach_call_args(e, start);
                    self.expect(Token::RParen);
                    return e;
                }
            }

            Token::Pipe => {
                // Pipe-style lambda: |x, y| expr
                // Token already consumed, self.pos is at the first param
                let saved_pos = self.pos - 1; // back to the Pipe token
                // Reconstruct the parser state to call pipe_lambda
                self.pos = saved_pos;
                return self.pipe_lambda();
            }

            Token::If => {
                // `if` as an expression: $x = if cond { a } else { b }
                // Token already consumed, self.pos is past the If token
                self.pos -= 1; // back to the If token
                return self.if_expr();
            }

            Token::RArrow => {
                // Bare -> in expression context — likely a lambda without params
                let msg = "unexpected '->' in expression. Did you mean '(x) -> expr' for a lambda?"
                    .to_string();
                self.error("E0008", t.span.clone(), msg);
                AstNode::Nil { span }
            }

            _ => {
                let msg = format!("unexpected token {:?} ({})", t.token, t.text);
                self.error("E0002", t.span.clone(), msg);
                AstNode::Nil { span }
            }
        }
    }

    #[inline]
    pub(super) fn looks_like_arg(&self) -> bool {
        if self.has_newline_before_current() {
            return false;
        }
        self.starts_positional_arg()
    }

    /// Check if there's a newline between the previous token and the current token.
    #[inline]
    pub(super) fn has_newline_before_current(&self) -> bool {
        if self.pos == 0 {
            return false;
        }
        let prev = &self.tokens[self.pos - 1];
        let cur = match self.current() {
            Some(t) => t,
            None => return true,
        };
        let gap_start = prev.byte_offset + prev.text.len();
        let gap_end = cur.byte_offset;
        if gap_end > gap_start {
            let gap = &self.source[gap_start..gap_end.min(self.source.len())];
            gap.contains('\n')
        } else {
            false
        }
    }

    /// True when the current token starts exactly where the previous token ends
    /// (no spaces or newlines between them).
    #[inline]
    pub(super) fn adjacent_to_previous(&self) -> bool {
        let Some(prev) = self.pos.checked_sub(1).and_then(|i| self.tokens.get(i)) else {
            return false;
        };
        let Some(cur) = self.current() else {
            return false;
        };
        prev.byte_offset + prev.text.len() == cur.byte_offset
    }

    /// Like `looks_like_arg`, but excludes `{` after control-flow keywords
    /// (e.g. `if $x { ... }`). After a dotted path or $-ident call, `{` is a block-lambda arg.
    #[inline]
    pub(super) fn starts_positional_arg(&self) -> bool {
        match self.current().map(|t| &t.token) {
            None => false,
            Some(Token::Ident) => !matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Assign)
                    | Some(Token::Colon)
                    | Some(Token::CoalesceAssign)
                    | Some(Token::AddAssign)
                    | Some(Token::SubAssign)
                    | Some(Token::MulAssign)
                    | Some(Token::DivAssign)
                    | Some(Token::BitOrAssign)
                    | Some(Token::BitXorAssign)
                    | Some(Token::PowAssign)
            ),
            Some(Token::IdentPlain) => {
                // IdentPlain can be a positional arg (e.g., `allow file.read "/data/"`)
                // but NOT if it looks like a type definition (`Name =`), impl (`Name is`),
                // or bare function definition (`Name(`).
                let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
                // Skip if it's an assignment: `$x = value` or `Name = {`
                if matches!(next, Some(Token::Assign)) {
                    return false;
                }
                // `name: value` is a named argument.
                if matches!(next, Some(Token::Colon)) {
                    return true;
                }
                // Skip if it's `Name is Trait`
                if matches!(next, Some(Token::Is)) {
                    return false;
                }
                // Skip if it's `Name(` (bare function definition at statement level)
                // But allow it in expression context (like `allow file.read`)
                // The key: if followed by `(` and then `{` or `->`, it's a fn def
                if matches!(next, Some(Token::LParen)) {
                    // Check if this looks like a function definition
                    if self.lookahead_fn_def_at(self.pos) {
                        return false;
                    }
                }
                true
            }
            Some(Token::LBrace) => {
                // Allow both block-lambda { stmts } and dict literals { key: val } as args
                true
            }
            Some(t) => matches!(
                t,
                Token::Bool
                    | Token::Any
                    | Token::Str
                    | Token::I8
                    | Token::I16
                    | Token::I32
                    | Token::I64
                    | Token::U8
                    | Token::U16
                    | Token::U32
                    | Token::U64
                    | Token::F32
                    | Token::F64
                    | Token::Int
                    | Token::Float
                    | Token::IntHex
                    | Token::IntBin
                    | Token::IntOct
                    | Token::String
                    | Token::RawString
                    | Token::MultiLineString
                    | Token::True
                    | Token::False
                    | Token::None_
                    | Token::LBracket
                    | Token::LParen
                    | Token::BitNot
                    | Token::SpecialVar
            ),
        }
    }
}
