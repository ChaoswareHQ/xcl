use crate::span::Span;
use crate::ast::{AstNode, BinaryOperator, UnaryOperator, Param, CallArg, PipelineStage};
use crate::lexer::{Token, TokenWithText, extract_string};

struct Parser<'a> {
    tokens: &'a [TokenWithText],
    pos: usize,
    file: &'a str,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [TokenWithText], file: &'a str) -> Self {
        Self { tokens, pos: 0, file }
    }

    #[inline] fn current(&self) -> Option<&TokenWithText> { self.tokens.get(self.pos) }
    #[inline] fn at_eof(&self) -> bool { self.pos >= self.tokens.len() }

    fn span(&self) -> Span {
        self.current().map(|t| t.span.clone()).unwrap_or_else(Span::empty)
    }

    fn consume(&mut self) -> TokenWithText {
        let t = self.current().cloned().unwrap_or_else(|| TokenWithText {
            token: Token::RParen, text: String::new(),
            span: Span::empty(),
            byte_offset: 0,
        });
        self.pos += 1;
        t
    }

    fn expect(&mut self, want: Token) {
        let t = self.consume();
        if t.token != want {
            panic!("XCL {}:{}: expected {:?}, got {:?} ({})",
                   t.span.file, t.span.line, want, t.token, t.text);
        }
    }

    #[inline] fn at(&self, tok: Token) -> bool {
        self.current().map(|t| t.token == tok).unwrap_or(false)
    }

    fn program(&mut self) -> Vec<AstNode> {
        let mut out = Vec::new();
        while !self.at_eof() {
            if self.at(Token::Semi) { self.consume(); continue; }
            out.push(self.statement());
        }
        out
    }

    fn statement(&mut self) -> AstNode {
        match self.current().map(|t| &t.token) {
            Some(Token::Return) => self.ret(),
            Some(Token::Break) => { let s = self.span(); self.consume(); AstNode::Break { span: s } }
            Some(Token::Continue) => { let s = self.span(); self.consume(); AstNode::Continue { span: s } }
            Some(Token::If) => self.if_expr(),
            Some(Token::For) => self.for_expr(),
            Some(Token::While) => self.while_expr(),
            Some(Token::Try) => self.try_expr(),
            Some(Token::Ident) if self.lookahead_assign() => self.assign(),
            Some(Token::IdentPlain) if self.lookahead_fn_def() => self.fn_def(),
            Some(Token::IdentPlain) if self.lookahead_throw() => self.throw_expr(),
            _ => self.expr_stmt(),
        }
    }

    #[inline]
    fn lookahead_assign(&self) -> bool {
        matches!(self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Assign) | Some(Token::AddAssign) | Some(Token::SubAssign)
            | Some(Token::MulAssign) | Some(Token::DivAssign) | Some(Token::CoalesceAssign)
            | Some(Token::Colon))
    }

    #[inline]
    fn lookahead_fn_def(&self) -> bool {
        if !matches!(self.tokens.get(self.pos + 1).map(|t| &t.token), Some(Token::LParen)) { return false; }
        matches!(self.tokens.get(self.pos + 2).map(|t| &t.token), Some(Token::Ident) | Some(Token::RParen))
    }

    #[inline]
    fn lookahead_throw(&self) -> bool {
        self.current().map(|t| t.text == "throw").unwrap_or(false)
    }

    fn assign(&mut self) -> AstNode {
        let span = self.span();
        let name = self.consume().text.trim_start_matches('$').to_string();
        let type_annotation = if self.at(Token::Colon) {
            self.consume();
            Some(self.parse_type())
        } else { None };

        let compound = match self.current().map(|t| &t.token) {
            Some(Token::AddAssign) => Some(BinaryOperator::Add),
            Some(Token::SubAssign) => Some(BinaryOperator::Sub),
            Some(Token::MulAssign) => Some(BinaryOperator::Mul),
            Some(Token::DivAssign) => Some(BinaryOperator::Div),
            _ => None,
        };

        if let Some(op) = compound {
            self.consume();
            let value = Box::new(self.expr());
            return AstNode::Assign {
                span: span.clone(),
                name: name.clone(),
                value: Box::new(AstNode::BinaryOp {
                    span: span.clone(),
                    left: Box::new(AstNode::Ident { span: span.clone(), name }),
                    op,
                    right: value,
                }),
                type_annotation: None,
            };
        }

        if self.at(Token::CoalesceAssign) {
            self.consume();
            let value = Box::new(self.expr());
            return AstNode::Assign {
                span: span.clone(),
                name: name.clone(),
                value: Box::new(AstNode::BinaryOp {
                    span: span.clone(),
                    left: Box::new(AstNode::Ident { span: span.clone(), name }),
                    op: BinaryOperator::Coalesce,
                    right: value,
                }),
                type_annotation: None,
            };
        }

        self.expect(Token::Assign);
        let value = Box::new(self.expr());
        AstNode::Assign { span, name, value, type_annotation }
    }

    fn ret(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let value = if !self.at_eof() && !self.at(Token::Semi) && !self.at(Token::RBrace) {
            Some(Box::new(self.expr()))
        } else { None };
        AstNode::Return { span, value }
    }

    fn throw_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let value = Box::new(self.expr());
        AstNode::Throw { span, value }
    }

    fn if_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let condition = Box::new(self.expr());
        let then_branch = self.block();
        let else_branch = self.else_branch();
        AstNode::If { span, condition, then_branch, else_branch }
    }

    fn else_branch(&mut self) -> Option<Vec<AstNode>> {
        if self.at(Token::Else) {
            self.consume();
            if self.at(Token::If) { Some(vec![self.if_expr()]) }
            else { Some(self.block()) }
        } else { None }
    }

    fn for_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let variable = self.consume().text.trim_start_matches('$').to_string();
        self.expect(Token::In);
        let iterable = Box::new(self.expr());
        let body = self.block();
        AstNode::For { span, variable, iterable, body }
    }

    fn while_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let condition = Box::new(self.expr());
        let body = self.block();
        AstNode::While { span, condition, body }
    }

    fn try_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let body = self.block();
        let (catch_var, catch_body) = if self.at(Token::Catch) {
            self.consume();
            let var_tok = self.consume();
            let v = if var_tok.token == Token::Ident {
                Some(var_tok.text.trim_start_matches('$').to_string())
            } else { None };
            (v, Some(self.block()))
        } else { (None, None) };
        AstNode::Try { span, body, catch_var, catch_body }
    }

    fn fn_def(&mut self) -> AstNode {
        let span = self.span();
        let name = self.consume().text.clone();
        self.expect(Token::LParen);
        let mut params = Vec::new();
        while !self.at(Token::RParen) && !self.at_eof() {
            params.push(self.param());
            if self.at(Token::Comma) { self.consume(); }
        }
        self.expect(Token::RParen);
        let return_type = if self.at(Token::RArrow) {
            self.consume();
            Some(self.parse_type())
        } else { None };
        let body = self.block();
        AstNode::FunctionDef { span, name, params, return_type, body }
    }

    fn param(&mut self) -> Param {
        let span = self.span();
        let name = self.consume().text.trim_start_matches('$').to_string();
        let type_annotation = if self.at(Token::Colon) {
            self.consume();
            Some(self.parse_type())
        } else { None };
        let default_value = if self.at(Token::Assign) {
            self.consume();
            Some(Box::new(self.expr()))
        } else { None };
        Param { name, type_annotation, default_value, span }
    }

    fn parse_type(&mut self) -> String {
        let base = self.consume().text.clone();
        if self.at(Token::Lt) {
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
                if self.at(Token::Comma) { self.consume(); }
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

    fn block(&mut self) -> Vec<AstNode> {
        self.expect(Token::LBrace);
        let mut out = Vec::new();
        while !self.at(Token::RBrace) && !self.at_eof() {
            if self.at(Token::Semi) { self.consume(); continue; }
            out.push(self.statement());
        }
        self.expect(Token::RBrace);
        out
    }

    fn expr_stmt(&mut self) -> AstNode {
        let e = self.expr();
        if self.at(Token::Semi) { self.consume(); }
        e
    }

    fn expr(&mut self) -> AstNode {
        let mut e = self.coalesce();
        while self.at(Token::Pipeline) {
            self.consume();
            e = self.pipeline_stage(e);
        }
        e
    }

    fn coalesce(&mut self) -> AstNode {
        let mut e = self.or_expr();
        while self.at(Token::Coalesce) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.coalesce());
            e = AstNode::BinaryOp { span, left: Box::new(e), op: BinaryOperator::Coalesce, right: r };
        }
        e
    }

    fn or_expr(&mut self) -> AstNode {
        let mut e = self.and_expr();
        while self.at(Token::Or) || self.at(Token::OrOp) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.and_expr());
            e = AstNode::BinaryOp { span, left: Box::new(e), op: BinaryOperator::Or, right: r };
        }
        e
    }

    fn and_expr(&mut self) -> AstNode {
        let mut e = self.equality();
        while self.at(Token::And) || self.at(Token::AndOp) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.equality());
            e = AstNode::BinaryOp { span, left: Box::new(e), op: BinaryOperator::And, right: r };
        }
        e
    }

    fn equality(&mut self) -> AstNode {
        let mut e = self.comparison();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Eq) => BinaryOperator::Eq,
                Some(Token::NotEq) => BinaryOperator::NotEq,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.comparison());
            e = AstNode::BinaryOp { span, left: Box::new(e), op, right: r };
        }
        e
    }

    fn comparison(&mut self) -> AstNode {
        let mut e = self.range_expr();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Lt) => BinaryOperator::Lt,
                Some(Token::Gt) => BinaryOperator::Gt,
                Some(Token::Le) => BinaryOperator::Le,
                Some(Token::Ge) => BinaryOperator::Ge,
                Some(Token::In) => BinaryOperator::In,
                Some(Token::Is) => {
                    let span = self.span();
                    self.consume();
                    let r = Box::new(self.primary());
                    e = AstNode::BinaryOp { span, left: Box::new(e), op: BinaryOperator::Eq, right: r };
                    continue;
                }
                Some(Token::As) => {
                    let span = self.span();
                    self.consume();
                    let r = Box::new(self.primary());
                    e = AstNode::BinaryOp { span, left: Box::new(e), op: BinaryOperator::Eq, right: r };
                    continue;
                }
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.range_expr());
            e = AstNode::BinaryOp { span, left: Box::new(e), op, right: r };
        }
        e
    }

    fn range_expr(&mut self) -> AstNode {
        let start = self.additive();
        if self.at(Token::Range) {
            let span = self.span();
            self.consume();
            let end = if self.at_eof() || !self.looks_like_expr_start() {
                None
            } else {
                Some(Box::new(self.additive()))
            };
            return AstNode::Range { span, start: Some(Box::new(start)), end };
        }
        start
    }

    #[inline]
    fn looks_like_expr_start(&self) -> bool {
        matches!(self.current().map(|t| &t.token),
            Some(Token::Int) | Some(Token::Float) | Some(Token::IntHex)
            | Some(Token::IntBin) | Some(Token::IntOct) | Some(Token::Ident)
            | Some(Token::IdentPlain) | Some(Token::Sub) | Some(Token::Not)
            | Some(Token::Bang) | Some(Token::LParen) | Some(Token::LBracket))
    }

    fn additive(&mut self) -> AstNode {
        let mut e = self.multiplicative();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Add) => BinaryOperator::Add,
                Some(Token::Sub) => BinaryOperator::Sub,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.multiplicative());
            e = AstNode::BinaryOp { span, left: Box::new(e), op, right: r };
        }
        e
    }

    fn multiplicative(&mut self) -> AstNode {
        let mut e = self.unary();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Mul) => BinaryOperator::Mul,
                Some(Token::Div) => BinaryOperator::Div,
                Some(Token::Mod) => BinaryOperator::Mod,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.unary());
            e = AstNode::BinaryOp { span, left: Box::new(e), op, right: r };
        }
        e
    }

    fn unary(&mut self) -> AstNode {
        let span = self.span();
        if self.at(Token::Sub) {
            self.consume();
            return AstNode::UnaryOp { span, op: UnaryOperator::Neg, operand: Box::new(self.unary()) };
        }
        if self.at(Token::Not) {
            self.consume();
            return AstNode::UnaryOp { span, op: UnaryOperator::Not, operand: Box::new(self.unary()) };
        }
        if self.at(Token::Bang) {
            self.consume();
            return self.unary();
        }
        if self.at(Token::LParen) && self.lookahead_lambda() {
            return self.lambda();
        }
        self.postfix()
    }

    fn lookahead_lambda(&self) -> bool {
        let mut depth = 1;
        let mut pos = self.pos + 1;
        while pos < self.tokens.len() && depth > 0 {
            match self.tokens[pos].token {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                return self.tokens.get(pos + 1).map(|t| t.token == Token::Arrow).unwrap_or(false);
            }
            pos += 1;
        }
        false
    }

    fn lambda(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let mut params = Vec::new();
        while !self.at(Token::RParen) && !self.at_eof() {
            let name = self.consume().text.trim_start_matches('$').to_string();
            params.push(name);
            if self.at(Token::Comma) { self.consume(); }
        }
        self.expect(Token::RParen);
        self.expect(Token::Arrow);
        let body = Box::new(self.expr());
        AstNode::Lambda { span, params, body }
    }

    fn postfix(&mut self) -> AstNode {
        let mut e = self.primary();
        loop {
            match self.current().map(|t| &t.token) {
                Some(Token::Dot) | Some(Token::OptionalDot) => {
                    self.consume();
                    let field = self.consume().text.clone();
                    e = AstNode::FieldAccess { span: self.span(), target: Box::new(e), field };
                }
                Some(Token::LBracket) => {
                    self.consume();
                    let idx = self.expr();
                    self.expect(Token::RBracket);
                    e = AstNode::Index { span: self.span(), target: Box::new(e), index: Box::new(idx) };
                }
                Some(Token::LParen) => {
                    self.consume();
                    let mut args = Vec::new();
                    while !self.at(Token::RParen) && !self.at_eof() {
                        args.push(self.call_arg());
                        if self.at(Token::Comma) { self.consume(); }
                    }
                    self.expect(Token::RParen);
                    e = AstNode::Call { span: self.span(), callee: Box::new(e), args };
                }
                _ => break,
            }
        }
        e
    }

    fn call_arg(&mut self) -> CallArg {
        if self.at(Token::IdentPlain) {
            let next = self.pos + 1;
            if matches!(self.tokens.get(next).map(|t| &t.token), Some(Token::Assign)) {
                let name = self.consume().text.clone();
                self.consume();
                let value = self.expr();
                return CallArg { value, name: Some(name) };
            }
        }
        CallArg { value: self.expr(), name: None }
    }

    fn primary(&mut self) -> AstNode {
        let t = self.consume();
        let span = t.span.clone();
        match t.token {
            Token::Int => AstNode::Int { span, value: t.text.trim().parse().unwrap_or(0) },
            Token::Float => AstNode::Float { span, value: t.text.trim().parse().unwrap_or(0.0) },
            Token::IntHex => {
                let hex = t.text.trim_start_matches("0x").trim_start_matches("0X");
                AstNode::Int { span, value: i64::from_str_radix(hex, 16).unwrap_or(0) }
            }
            Token::IntBin => {
                let bin = t.text.trim_start_matches("0b").trim_start_matches("0B");
                AstNode::Int { span, value: i64::from_str_radix(bin, 2).unwrap_or(0) }
            }
            Token::IntOct => {
                let oct = t.text.trim_start_matches("0o");
                AstNode::Int { span, value: i64::from_str_radix(oct, 8).unwrap_or(0) }
            }
            Token::String | Token::RawString | Token::MultiLineString => {
                AstNode::Str { span, value: extract_string(&t.text) }
            }
            Token::True => AstNode::Bool { span, value: true },
            Token::False => AstNode::Bool { span, value: false },
            Token::None_ => AstNode::Nil { span },

            Token::Ident => {
                let name = t.text.trim_start_matches('$').to_string();
                let mut args = Vec::new();
                while self.looks_like_literal_arg() {
                    args.push(CallArg { value: self.expr(), name: None });
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

            Token::IdentPlain => {
                let name = t.text.clone();
                let mut args = Vec::new();
                while self.looks_like_arg() {
                    args.push(self.call_arg());
                }
                while self.at(Token::Comma) {
                    self.consume();
                    if self.looks_like_arg() { args.push(self.call_arg()); }
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
                    if self.at(Token::Comma) { self.consume(); }
                }
                self.expect(Token::RBracket);
                AstNode::ArrayLit { span, elements }
            }

            Token::LBrace => {
                let mut entries = Vec::new();
                while !self.at(Token::RBrace) && !self.at_eof() {
                    if self.at(Token::Semi) || self.at(Token::Comma) { self.consume(); continue; }
                    let key = self.consume().text.clone();
                    if self.at(Token::Colon) {
                        self.consume();
                        let value = self.expr();
                        entries.push((key, value));
                    } else {
                        entries.push((key.clone(), AstNode::Ident { span: span.clone(), name: key }));
                    }
                }
                self.expect(Token::RBrace);
                AstNode::DictLit { span, entries }
            }

            Token::LParen => {
                let e = self.expr();
                self.expect(Token::RParen);
                e
            }

            _ => panic!("XCL {}:{}: unexpected token {:?} ({})",
                        t.span.file, t.span.line, t.token, t.text),
        }
    }

    #[inline]
    fn looks_like_literal_arg(&self) -> bool {
        matches!(self.current().map(|t| &t.token),
            Some(Token::Int) | Some(Token::Float) | Some(Token::String)
            | Some(Token::RawString) | Some(Token::MultiLineString)
            | Some(Token::True) | Some(Token::False) | Some(Token::None_))
    }

    #[inline]
    fn looks_like_arg(&self) -> bool {
        match self.current().map(|t| &t.token) {
            None => false,
            Some(Token::Ident) => {
                !matches!(self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Assign) | Some(Token::Semi) | Some(Token::Colon)
                    | Some(Token::CoalesceAssign) | Some(Token::AddAssign)
                    | Some(Token::SubAssign) | Some(Token::MulAssign) | Some(Token::DivAssign))
            }
            Some(t) => matches!(t,
                Token::Arr | Token::Str | Token::Fn | Token::Dict
                | Token::Bool | Token::Any
                | Token::Int | Token::Float | Token::IntHex | Token::IntBin | Token::IntOct
                | Token::String | Token::RawString | Token::MultiLineString
                | Token::True | Token::False | Token::None_
                | Token::LBracket | Token::LBrace
                | Token::Sub | Token::Not | Token::Bang),
        }
    }

    fn pipeline_stage(&mut self, input: AstNode) -> AstNode {
        let span = self.span();
        let name = self.consume().text.clone();
        let arg = if self.looks_like_arg() {
            Some(Box::new(self.expr()))
        } else { None };
        AstNode::Pipeline {
            span,
            input: Box::new(input),
            stages: vec![PipelineStage { name, arg }],
        }
    }
}

pub fn parse(tokens: &[TokenWithText], file: &str) -> Vec<AstNode> {
    Parser::new(tokens, file).program()
}