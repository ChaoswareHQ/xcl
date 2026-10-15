use crate::span::Span;
use crate::ast::{AstNode, BinaryOperator, UnaryOperator, Param, CallArg, PipelineStage, TypeKind, SumVariant, TypeField};
use crate::lexer::{Token, TokenWithText, extract_string};

struct Parser<'a> {
    tokens: &'a [TokenWithText],
    pos: usize,
    source: &'a str,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [TokenWithText], source: &'a str) -> Self {
        Self { tokens, pos: 0, source }
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
            Some(Token::Ident) if self.lookahead_assign() => self.assign(),
            Some(Token::IdentPlain) if self.lookahead_fn_def() => self.fn_def(),
            // Type definition: `Name = { ... }` or `Name = | Variant1 { ... } | Variant2 { ... }`
            Some(Token::IdentPlain) if self.lookahead_type_def() => self.type_def(),
            // Impl block: `Name is Trait { ... }`
            Some(Token::IdentPlain) if self.lookahead_impl_def() => self.impl_def(),
            // `defer { ... }` — keyword-like syntax for deferred closures
            Some(Token::IdentPlain) if self.lookahead_defer() => self.defer_expr(),
            _ => self.expr_stmt(),
        }
    }

    #[inline]
    fn lookahead_defer(&self) -> bool {
        // `defer` followed by `{`
        if self.current().map(|t| t.text != "defer").unwrap_or(true) {
            return false;
        }
        matches!(self.tokens.get(self.pos + 1).map(|t| &t.token), Some(Token::LBrace))
    }

    fn defer_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume(); // consume "defer"
        // Parse the block { ... }
        let statements = self.block();
        // Check if body references $it
        let uses_it = contains_ident(&statements, "it");
        // Wrap in: defer( Lambda { params: [], body: Block { statements } } )
        let lambda = AstNode::Lambda {
            span: span.clone(),
            params: vec![],
            body: Box::new(AstNode::Block { span: span.clone(), statements }),
            implicit_it: uses_it,
        };
        AstNode::Call {
            span: span.clone(),
            callee: Box::new(AstNode::Ident { span, name: "defer".to_string() }),
            args: vec![CallArg { value: lambda, name: None }],
        }
    }

    #[inline]
    fn lookahead_type_def(&self) -> bool {
        // Name = { or Name = |
        if !matches!(self.tokens.get(self.pos + 1).map(|t| &t.token), Some(Token::Assign)) {
            return false;
        }
        matches!(self.tokens.get(self.pos + 2).map(|t| &t.token),
            Some(Token::LBrace) | Some(Token::Pipe))
    }

    #[inline]
    fn lookahead_impl_def(&self) -> bool {
        // Name is Trait {
        if !matches!(self.tokens.get(self.pos + 1).map(|t| &t.token), Some(Token::Is)) {
            return false;
        }
        // Check for: Name is IdentPlain {
        if !matches!(self.tokens.get(self.pos + 2).map(|t| &t.token), Some(Token::IdentPlain)) {
            return false;
        }
        matches!(self.tokens.get(self.pos + 3).map(|t| &t.token), Some(Token::LBrace))
    }

    fn type_def(&mut self) -> AstNode {
        let span = self.span();
        let name = self.consume().text; // IdentPlain
        self.expect(Token::Assign);

        if self.at(Token::Pipe) {
            // Sum type: `Name = | Variant1 { ... } | Variant2 { ... }`
            self.consume(); // |
            let mut variants = Vec::new();
            while self.at(Token::IdentPlain) {
                let vname = self.consume().text;
                let mut fields = Vec::new();
                if self.at(Token::LBrace) {
                    self.consume(); // {
                    while !self.at(Token::RBrace) && !self.at(Token::Pipe) && !self.at_eof() {
                        if self.at(Token::Semi) { self.consume(); continue; }
                        let fname = self.consume().text;
                        self.expect(Token::Colon);
                        let fty = self.parse_type();
                        fields.push(TypeField { name: fname, ty: fty, mutable: false });
                        if self.at(Token::Semi) { self.consume(); }
                    }
                    self.expect(Token::RBrace);
                }
                variants.push(SumVariant { name: vname, fields });
                if self.at(Token::Pipe) { self.consume(); }
            }
            AstNode::TypeDef { span, name, kind: TypeKind::Sum { variants }, body: Vec::new() }
        } else {
            // Record or Trait: `Name = { ... }`
            self.expect(Token::LBrace);
            let mut body = Vec::new();
            while !self.at(Token::RBrace) && !self.at_eof() {
                if self.at(Token::Semi) { self.consume(); continue; }
                // Check if it's a method definition
                if self.at(Token::IdentPlain) && self.lookahead_fn_def() {
                    body.push(self.fn_def());
                } else {
                    // Field definition: `name: type` or `mut name: type`
                    let span = self.span();
                    let mut _mutable = false;
                    if self.at(Token::IdentPlain) && self.current().map(|t| t.text == "mut").unwrap_or(false) {
                        _mutable = true;
                        self.consume();
                    }
                    let fname = self.consume().text;
                    self.expect(Token::Colon);
                    let fty = self.parse_type();
                    // Store as a synthetic assign node for now; compiler will handle it
                    body.push(AstNode::Assign {
                        span,
                        name: format!("__field__{}__{}__", fname, fty),
                        value: Box::new(AstNode::Nil { span: Span::empty() }),
                        type_annotation: None,
                    });
                    if self.at(Token::Semi) { self.consume(); }
                }
            }
            self.expect(Token::RBrace);
            // Determine if trait (only methods) or record (has fields)
            let kind = if body.iter().all(|n| matches!(n, AstNode::FunctionDef { .. })) {
                TypeKind::Trait
            } else {
                TypeKind::Record
            };
            AstNode::TypeDef { span, name, kind, body }
        }
    }

    fn impl_def(&mut self) -> AstNode {
        let span = self.span();
        let type_name = self.consume().text; // IdentPlain
        self.expect(Token::Is);
        let trait_name = self.consume().text; // IdentPlain
        self.expect(Token::LBrace);
        let mut body = Vec::new();
        while !self.at(Token::RBrace) && !self.at_eof() {
            if self.at(Token::Semi) { self.consume(); continue; }
            if self.at(Token::IdentPlain) && self.lookahead_fn_def() {
                body.push(self.fn_def());
            } else {
                // Skip unknown content
                self.consume();
            }
        }
        self.expect(Token::RBrace);
        AstNode::ImplDef { span, type_name, trait_name, body }
    }

    #[inline]
    fn lookahead_dict_literal(&self) -> bool {
        // self.pos already points past the consumed `{`, so start here.
        // Find the first meaningful token. If it's followed by `:`, it's a dict literal.
        // Otherwise, it's a block-lambda.
        // `{ }` → empty block-lambda (returns nil)
        let mut i = self.pos;
        while i < self.tokens.len() {
            match self.tokens[i].token {
                Token::RBrace => return false, // empty `{ }` → block-lambda
                Token::Semi | Token::Comma => { i += 1; continue; }
                _ => {
                    // Found the first meaningful token — check if next is `:`
                    return matches!(self.tokens.get(i + 1).map(|t| &t.token), Some(Token::Colon));
                }
            }
        }
        false // reached EOF without `}` → block-lambda
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
        // Must be: name( ... ) { or name( ... ) ->
        // We scan forward past the parens to check for a block or return type.
        if !matches!(self.tokens.get(self.pos + 1).map(|t| &t.token), Some(Token::LParen)) {
            return false;
        }
        // Find the matching closing paren
        let mut depth = 1;
        let mut i = self.pos + 2;
        while i < self.tokens.len() && depth > 0 {
            match self.tokens[i].token {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            if depth > 0 { i += 1; }
        }
        if depth != 0 { return false; }
        // After the closing paren, expect { or ->
        matches!(self.tokens.get(i).map(|t| &t.token),
            Some(Token::LBrace) | Some(Token::RArrow))
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
        // $f $x call pattern: if expression is a plain $-ident (not FieldAccess)
        // and next token starts a positional arg, treat it as a call
        if let AstNode::Ident { .. } = e
            && self.starts_positional_arg()
        {
            let callee = e;
            let mut args = vec![self.call_arg()];
            while self.starts_positional_arg() {
                args.push(self.call_arg());
            }
            if self.at(Token::Semi) { self.consume(); }
            return AstNode::Call { span: self.span(), callee: Box::new(callee), args };
        }
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
                    // Check if the next token is a type name (IdentPlain) — type test
                    if self.at(Token::IdentPlain) {
                        let type_name = self.consume().text;
                        e = AstNode::TypeTest { span, value: Box::new(e), type_name };
                    } else {
                        // Fallback: equality check
                        let r = Box::new(self.primary());
                        e = AstNode::BinaryOp { span, left: Box::new(e), op: BinaryOperator::Eq, right: r };
                    }
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
            if self.at(Token::Comma) || self.at(Token::Semi) { self.consume(); }
        }
        self.expect(Token::RParen);
        self.expect(Token::Arrow);
        let body = Box::new(self.expr());
        AstNode::Lambda { span, params, body, implicit_it: false }
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
                    // If there's a newline between the previous token and this `(`,
                    // treat the `(` as starting a new statement, not a call postfix.
                    if self.pos > 0 {
                        let prev = &self.tokens[self.pos - 1];
                        let gap_start = prev.byte_offset + prev.text.len();
                        let gap_end = self.current().map(|t| t.byte_offset).unwrap_or(gap_start);
                        if gap_end > gap_start {
                            let gap = &self.source[gap_start..gap_end.min(self.source.len())];
                            if gap.contains('\n') {
                                break;
                            }
                        }
                    }
                    self.consume();
                    let mut args = Vec::new();
                    while !self.at(Token::RParen) && !self.at_eof() {
                        args.push(self.call_arg());
                        if self.at(Token::Comma) { self.consume(); }
                    }
                    self.expect(Token::RParen);
                    e = AstNode::Call { span: self.span(), callee: Box::new(e), args };
                    // After parenthesized call, check for additional space-separated args
                    // but only if they're on the same line (no newline between ) and next token)
                    if self.starts_positional_arg() && !self.has_newline_before_current() {
                        let AstNode::Call { callee, args: mut a, .. } = e else { unreachable!() };
                        while self.starts_positional_arg() && !self.has_newline_before_current() {
                            a.push(self.call_arg());
                        }
                        e = AstNode::Call { span: self.span(), callee, args: a };
                    }
                }
                _ => break,
            }
        }

        // Space-separated call after a bare dotted path.
        // Only apply when the previous token was an IdentPlain (not $-ident).
        // $-ident calls are handled by expr_stmt at the statement level.
        let prev_is_ident = self.pos > 0 && matches!(
            self.tokens[self.pos - 1].token,
            Token::IdentPlain
        );

        if prev_is_ident
            && matches!(e, AstNode::Ident { .. } | AstNode::FieldAccess { .. } | AstNode::Call { .. })
            && self.starts_positional_arg()
            && !self.has_newline_before_current() {
            // Skip $f.method $x when $x is also a field access ($x.field)
            // because $x.field is a separate arg, not a call to $f.method
            let skip = matches!(e, AstNode::FieldAccess { .. })
                && self.at(Token::Ident)
                && matches!(self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Dot) | Some(Token::LBracket));
            if skip {
                // Do nothing — $x.field will be consumed as a separate arg by the caller
            } else {
                // If e is already a Call (from parenthesized args), merge additional args
                let mut args: Vec<CallArg> = if let AstNode::Call { args, .. } = &e {
                    args.clone()
                } else {
                    Vec::new()
                };
                let callee: Box<AstNode> = if let AstNode::Call { callee, .. } = e {
                    callee
                } else {
                    Box::new(e)
                };
                while self.starts_positional_arg() {
                    args.push(self.call_arg());
                }
                while self.at(Token::Comma) {
                    self.consume();
                    if self.starts_positional_arg() {
                        args.push(self.call_arg());
                    }
                }
                e = AstNode::Call {
                    span: self.span(),
                    callee,
                    args,
                };
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
                AstNode::Ident { span, name }
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
                    if self.at(Token::Comma) || self.at(Token::Semi) { self.consume(); }
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
                } else {
                    // Block-lambda: { stmt1; stmt2; ... }
                    let mut statements = Vec::new();
                    while !self.at(Token::RBrace) && !self.at_eof() {
                        if self.at(Token::Semi) { self.consume(); continue; }
                        statements.push(self.statement());
                    }
                    self.expect(Token::RBrace);
                    // Check if body references $it
                    let uses_it = contains_ident(&statements, "it");
                    AstNode::Lambda {
                        span: span.clone(),
                        params: vec![],
                        body: Box::new(AstNode::Block { span, statements }),
                        implicit_it: uses_it,
                    }
                }
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
    fn looks_like_arg(&self) -> bool {
        // If there's a newline before the current token, it's not an arg
        if self.has_newline_before_current() {
            return false;
        }
        match self.current().map(|t| &t.token) {
            None => false,
            Some(Token::Ident) => {
                // $-prefixed identifiers are args if:
                // 1. Followed by = (named arg: $x = value)
                // 2. Followed by . (field access: $x.field)
                // 3. Followed by [ (index access: $x[0])
                // NOT if followed by another $-ident (that's a call: $f $x)
                let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
                matches!(next,
                    Some(Token::Assign) | Some(Token::CoalesceAssign)
                    | Some(Token::AddAssign) | Some(Token::SubAssign)
                    | Some(Token::MulAssign) | Some(Token::DivAssign)
                    | Some(Token::Dot) | Some(Token::LBracket))
            }
            Some(t) => matches!(t,
                Token::Fn
                | Token::Bool | Token::Any
                | Token::Int | Token::Float | Token::IntHex | Token::IntBin | Token::IntOct
                | Token::String | Token::RawString | Token::MultiLineString
                | Token::True | Token::False | Token::None_
                | Token::LBracket | Token::LBrace | Token::LParen),
        }
    }

    /// Check if there's a newline between the previous token and the current token.
    #[inline]
    fn has_newline_before_current(&self) -> bool {
        if self.pos == 0 { return false; }
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

    /// Like `looks_like_arg`, but excludes `{` after control-flow keywords
    /// (e.g. `if $x { ... }`). After a dotted path or $-ident call, `{` is a block-lambda arg.
    #[inline]
    fn starts_positional_arg(&self) -> bool {
        match self.current().map(|t| &t.token) {
            None => false,
            Some(Token::Ident) => {
                !matches!(self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Assign) | Some(Token::Semi) | Some(Token::Colon)
                    | Some(Token::CoalesceAssign) | Some(Token::AddAssign)
                    | Some(Token::SubAssign) | Some(Token::MulAssign) | Some(Token::DivAssign))
            }
            Some(t) => matches!(t,
                Token::Fn
                | Token::Bool | Token::Any
                | Token::Int | Token::Float | Token::IntHex | Token::IntBin | Token::IntOct
                | Token::String | Token::RawString | Token::MultiLineString
                | Token::True | Token::False | Token::None_
                | Token::LBracket | Token::LBrace | Token::LParen),
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

pub fn parse(tokens: &[TokenWithText], source: &str) -> Vec<AstNode> {
    Parser::new(tokens, source).program()
}

/// Recursively check if any AstNode in a list references an identifier with the given name.
fn contains_ident(nodes: &[AstNode], name: &str) -> bool {
    for node in nodes {
        if node_has_ident(node, name) { return true; }
    }
    false
}

fn node_has_ident(node: &AstNode, name: &str) -> bool {
    match node {
        AstNode::Ident { name: n, .. } if n == name => true,
        AstNode::Assign { value, .. } => node_has_ident(value, name),
        AstNode::Return { value, .. } => value.as_ref().map_or(false, |v| node_has_ident(v, name)),
        AstNode::If { condition, then_branch, else_branch, .. } => {
            node_has_ident(condition, name)
                || then_branch.iter().any(|n| node_has_ident(n, name))
                || else_branch.as_ref().map_or(false, |b| b.iter().any(|n| node_has_ident(n, name)))
        }
        AstNode::For { iterable, body, .. } => {
            node_has_ident(iterable, name) || body.iter().any(|n| node_has_ident(n, name))
        }
        AstNode::While { condition, body, .. } => {
            node_has_ident(condition, name) || body.iter().any(|n| node_has_ident(n, name))
        }
        AstNode::Lambda { body, .. } => node_has_ident(body, name),
        AstNode::Call { callee, args, .. } => {
            node_has_ident(callee, name) || args.iter().any(|a| node_has_ident(&a.value, name))
        }
        AstNode::BinaryOp { left, right, .. } => {
            node_has_ident(left, name) || node_has_ident(right, name)
        }
        AstNode::UnaryOp { operand, .. } => node_has_ident(operand, name),
        AstNode::ArrayLit { elements, .. } => elements.iter().any(|e| node_has_ident(e, name)),
        AstNode::DictLit { entries, .. } => entries.iter().any(|(_, v)| node_has_ident(v, name)),
        AstNode::Index { target, index, .. } => {
            node_has_ident(target, name) || node_has_ident(index, name)
        }
        AstNode::FieldAccess { target, .. } => node_has_ident(target, name),
        AstNode::Range { start, end, .. } => {
            start.as_ref().map_or(false, |s| node_has_ident(s, name))
                || end.as_ref().map_or(false, |e| node_has_ident(e, name))
        }
        AstNode::Block { statements, .. } => statements.iter().any(|n| node_has_ident(n, name)),
        AstNode::Pipeline { input, stages, .. } => {
            node_has_ident(input, name)
                || stages.iter().any(|s| s.arg.as_ref().map_or(false, |a| node_has_ident(a, name)))
        }
        AstNode::TypeTest { value, .. } => node_has_ident(value, name),
        // Literals and other nodes don't contain identifiers
        _ => false,
    }
}