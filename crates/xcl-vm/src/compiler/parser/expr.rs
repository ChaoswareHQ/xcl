//! The expression grammar.
//!
//! One function per precedence level, from `expr` (lowest) down through the
//! binary levels to `unary`, `postfix`, and the operators that take special
//! shape: coalescing, comparison chains, ranges, lambdas, and pipelines.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn expr(&mut self) -> AstNode {
        if !self.enter_recursion() {
            // Recursion limit reached — consume one token to make progress.
            let span = self.span();
            self.consume();
            return AstNode::Nil { span };
        }
        let mut e = self.coalesce();
        // Only consume `|>` when not inside a pipeline stage's lambda body
        while self.at(Token::Pipeline) && self.pipeline_depth == 0 {
            self.consume();
            e = self.pipeline_stage(e);
        }
        if self.at(Token::If) && !self.has_newline_before_current() {
            let span = self.span();
            self.consume(); // if
            let condition = self.expr();
            self.expect(Token::Else);
            let else_expr = self.expr();
            e = AstNode::If {
                span,
                condition: Box::new(condition),
                then_branch: vec![e],
                else_branch: Some(vec![else_expr]),
            };
        }
        self.exit_recursion();
        e
    }

    pub(super) fn coalesce(&mut self) -> AstNode {
        let mut e = self.or_expr();
        while self.at(Token::Coalesce) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.coalesce());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::Coalesce,
                right: r,
            };
        }
        e
    }

    pub(super) fn or_expr(&mut self) -> AstNode {
        let mut e = self.and_expr();
        while self.at(Token::Or) || self.at(Token::OrOp) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.and_expr());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::Or,
                right: r,
            };
        }
        e
    }

    pub(super) fn and_expr(&mut self) -> AstNode {
        let mut e = self.equality();
        while self.at(Token::And) || self.at(Token::AndOp) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.equality());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::And,
                right: r,
            };
        }
        e
    }

    pub(super) fn equality(&mut self) -> AstNode {
        let mut e = self.bit_or();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Eq) => BinaryOperator::Eq,
                Some(Token::NotEq) => BinaryOperator::NotEq,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.bit_or());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op,
                right: r,
            };
        }
        e
    }

    pub(super) fn bit_or(&mut self) -> AstNode {
        let mut e = self.bit_xor();
        while self.at(Token::Pipe) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.bit_xor());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::BitOr,
                right: r,
            };
        }
        e
    }

    pub(super) fn bit_xor(&mut self) -> AstNode {
        let mut e = self.bitwise_and();
        while self.at(Token::BitXor) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.bitwise_and());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::BitXor,
                right: r,
            };
        }
        e
    }

    pub(super) fn bitwise_and(&mut self) -> AstNode {
        let mut e = self.comparison();
        loop {
            if !self.at(Token::BitAnd) {
                break;
            }
            let span = self.span();
            self.consume();
            let r = Box::new(self.comparison());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::BitAnd,
                right: r,
            };
        }
        e
    }

    pub(super) fn comparison(&mut self) -> AstNode {
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
                    // Check if the next token is a type name — type test
                    // Accepts IdentPlain (custom types) and primitive type keywords
                    let is_type_keyword = matches!(
                        self.current().map(|t| &t.token),
                        Some(
                            Token::Str
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
                                | Token::Bool
                                | Token::None_
                                | Token::Any
                        )
                    );
                    if self.at(Token::IdentPlain) || is_type_keyword {
                        let type_name = self.parse_type();
                        e = AstNode::TypeTest {
                            span,
                            value: Box::new(e),
                            type_name,
                        };
                    } else if self.looks_like_expr_start() {
                        // Fallback: equality check
                        let r = Box::new(self.primary());
                        e = AstNode::BinaryOp {
                            span,
                            left: Box::new(e),
                            op: BinaryOperator::Eq,
                            right: r,
                        };
                    }
                    // If not a valid expr start (e.g., `}`), treat `is` as part of the
                    // left-hand side identifier and don't consume it as an operator.
                    continue;
                }
                Some(Token::As) => {
                    let span = self.span();
                    self.consume();
                    // Parse as type cast: `$x as i32`
                    let type_name = self.parse_type();
                    e = AstNode::TypeCast {
                        span,
                        value: Box::new(e),
                        type_name,
                    };
                    continue;
                }
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.range_expr());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op,
                right: r,
            };
        }
        e
    }

    pub(super) fn range_expr(&mut self) -> AstNode {
        let start = self.additive();
        if self.at(Token::Range) {
            let span = self.span();
            self.consume();
            let end = if self.at_eof() || !self.looks_like_expr_start() {
                None
            } else {
                Some(Box::new(self.additive()))
            };
            return AstNode::Range {
                span,
                start: Some(Box::new(start)),
                end,
            };
        }
        start
    }

    #[inline]
    pub(super) fn looks_like_expr_start(&self) -> bool {
        matches!(
            self.current().map(|t| &t.token),
            Some(Token::Int)
                | Some(Token::Float)
                | Some(Token::IntHex)
                | Some(Token::IntBin)
                | Some(Token::IntOct)
                | Some(Token::Ident)
                | Some(Token::IdentPlain)
                | Some(Token::Str)
                | Some(Token::I8)
                | Some(Token::I16)
                | Some(Token::I32)
                | Some(Token::I64)
                | Some(Token::U8)
                | Some(Token::U16)
                | Some(Token::U32)
                | Some(Token::U64)
                | Some(Token::F32)
                | Some(Token::F64)
                | Some(Token::Sub)
                | Some(Token::Not)
                | Some(Token::Bang)
                | Some(Token::BitNot)
                | Some(Token::LParen)
                | Some(Token::LBracket)
                | Some(Token::Pipe)
        )
    }

    pub(super) fn additive(&mut self) -> AstNode {
        let mut e = self.shift();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Add) => BinaryOperator::Add,
                Some(Token::Sub) => BinaryOperator::Sub,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.shift());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op,
                right: r,
            };
        }
        e
    }

    pub(super) fn shift(&mut self) -> AstNode {
        let mut e = self.multiplicative();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Shl) => BinaryOperator::Shl,
                Some(Token::Shr) => BinaryOperator::Shr,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.multiplicative());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op,
                right: r,
            };
        }
        e
    }

    pub(super) fn multiplicative(&mut self) -> AstNode {
        let mut e = self.power();
        loop {
            let op = match self.current().map(|t| &t.token) {
                Some(Token::Mul) => BinaryOperator::Mul,
                Some(Token::Div) => BinaryOperator::Div,
                Some(Token::Mod) => BinaryOperator::Mod,
                _ => break,
            };
            let span = self.span();
            self.consume();
            let r = Box::new(self.power());
            e = AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op,
                right: r,
            };
        }
        e
    }

    pub(super) fn power(&mut self) -> AstNode {
        let e = self.unary();
        if self.at(Token::Pow) {
            let span = self.span();
            self.consume();
            let r = Box::new(self.power());
            AstNode::BinaryOp {
                span,
                left: Box::new(e),
                op: BinaryOperator::Pow,
                right: r,
            }
        } else {
            e
        }
    }

    pub(super) fn unary(&mut self) -> AstNode {
        let span = self.span();
        if self.at(Token::Sub) {
            self.consume();
            return AstNode::UnaryOp {
                span,
                op: UnaryOperator::Neg,
                operand: Box::new(self.unary()),
            };
        }
        if self.at(Token::Not) {
            self.consume();
            return AstNode::UnaryOp {
                span,
                op: UnaryOperator::Not,
                operand: Box::new(self.unary()),
            };
        }
        if self.at(Token::BitNot) {
            let span = self.span();
            self.consume();
            return AstNode::UnaryOp {
                span,
                op: UnaryOperator::BitNot,
                operand: Box::new(self.unary()),
            };
        }
        if self.at(Token::Bang) {
            self.consume();
            return self.unary();
        }
        self.postfix()
    }

    /// Lookahead: does [ look like an index ($arr[0]) or an array literal ([1, 2, 3])?
    /// An index has a single expression followed by ]. An array has commas.
    pub(super) fn lookahead_index(&self) -> bool {
        // self.pos points at LBracket. Scan from pos+1.
        let mut i = self.pos + 1;
        let mut depth = 1;
        while i < self.tokens.len() && depth > 0 {
            match self.tokens[i].token {
                Token::LBracket | Token::LParen | Token::LBrace => depth += 1,
                Token::RBracket | Token::RParen | Token::RBrace => depth -= 1,
                Token::Comma if depth == 1 => return false, // comma at top level = array literal
                _ => {}
            }
            if depth > 0 {
                i += 1;
            }
        }
        true // no comma found = index expression
    }

    /// Try to parse lambda parameters: a list of $-identifiers separated by commas, ending with RParen.
    /// Returns Some(params) if successful and consumes the RParen, otherwise None without consuming tokens.
    pub(super) fn try_parse_lambda_params(&mut self) -> Option<Vec<String>> {
        // self.pos points to the first token after LParen
        let mut params = Vec::new();
        let mut pos = self.pos;

        if pos >= self.tokens.len() {
            return None;
        }

        // Parse first param
        match &self.tokens[pos].token {
            Token::Ident => {
                let name = self.tokens[pos].text.trim_start_matches('$').to_string();
                params.push(name);
                pos += 1;
            }
            Token::RParen => {
                // Empty params: () => expr
                self.pos = pos + 1; // consume RParen
                return Some(params);
            }
            _ => return None,
        }

        // Parse remaining params
        loop {
            if pos >= self.tokens.len() {
                return None;
            }
            match &self.tokens[pos].token {
                Token::Comma | Token::Semi => {
                    pos += 1;
                    if pos >= self.tokens.len() {
                        return None;
                    }
                    // Check for next param
                    match &self.tokens[pos].token {
                        Token::Ident => {
                            let name = self.tokens[pos].text.trim_start_matches('$').to_string();
                            params.push(name);
                            pos += 1;
                        }
                        Token::RParen => {
                            // Trailing comma: ($x,) => expr
                            self.pos = pos + 1;
                            return Some(params);
                        }
                        _ => return None,
                    }
                }
                Token::RParen => {
                    self.pos = pos + 1; // consume RParen
                    return Some(params);
                }
                _ => return None,
            }
        }
    }

    /// Parse pipe-style lambda: |$x, $y| expr — params must use $ prefix
    pub(super) fn pipe_lambda(&mut self) -> AstNode {
        let span = self.span();
        self.consume(); // consume first |
        let mut params = Vec::new();
        while !self.at(Token::Pipe) && !self.at_eof() {
            // Pipe lambda params must use $ prefix: |$x| $x * 2
            let t = self.consume();
            if !matches!(&t.token, Token::Ident) {
                let msg = format!(
                    "pipe lambda parameter must use $ prefix, got '{}' (use ${} instead)",
                    t.text, t.text
                );
                self.error_with_help(
                    "E0004",
                    t.span.clone(),
                    msg,
                    format!("prefix the parameter with $: ${}", t.text),
                );
                // Still add the name so parsing continues
                params.push(t.text.clone());
            } else {
                let name = t.text.trim_start_matches('$').to_string();
                params.push(name);
            }
            if self.at(Token::Comma) || self.at(Token::Semi) {
                self.consume();
            }
        }
        self.expect(Token::Pipe); // consume closing |
        let body = Box::new(self.expr());
        AstNode::Lambda { span, params, body }
    }

    pub(super) fn postfix(&mut self) -> AstNode {
        // The first token of this expression. It cannot be recovered by walking
        // backwards afterwards: the last token of the primary is the one before
        // the scan starts, so the scan lands on the previous statement.
        let start_pos = self.pos;
        let mut e = self.primary();
        // Track whether this expression starts with a domain function (IdentPlain)
        // Domain functions like `collections.map` should not have [ ] index operations
        // because [ ] after them is a call arg (array literal), not an index.
        let starts_with_ident_plain = matches!(
            self.tokens.get(start_pos).map(|t| &t.token),
            Some(Token::IdentPlain)
        );

        loop {
            match self.current().map(|t| &t.token) {
                Some(Token::Dot) | Some(Token::OptionalDot) => {
                    self.consume();
                    let field = self.consume().text.clone();
                    e = AstNode::FieldAccess {
                        span: self.span(),
                        target: Box::new(e),
                        field,
                    };
                }
                Some(Token::LParen) if !starts_with_ident_plain && self.adjacent_to_previous() => {
                    // `$f(...)` / `f(...)` are not call syntax. The only
                    // parenthesized form that follows a callee is a *spaced*
                    // grouped argument (`f (x)`), which the call loop below
                    // handles.
                    let span = self.span();
                    let msg = "`(...)` must be separated from the callee by a space — \
                     calls use space-separated arguments (`f arg1 arg2`); \
                     `f (expr)` passes one grouped expression"
                        .to_string();
                    self.error("E0005", span.clone(), msg);
                    // Consume the `(` to advance past the error
                    self.consume();
                    break;
                }
                Some(Token::LBracket) => {
                    // Lookahead: if [ is followed by ] or by expr,], it's an index.
                    // If [ is followed by expr, expr, ... ], it's an array literal (call arg).
                    // For domain functions (IdentPlain), [ ] is always a call arg.
                    if starts_with_ident_plain || !self.lookahead_index() {
                        // Skip — let call detection handle it as a call arg
                        break;
                    } else {
                        self.consume();
                        let idx = self.expr();
                        self.expect(Token::RBracket);
                        e = AstNode::Index {
                            span: self.span(),
                            target: Box::new(e),
                            index: Box::new(idx),
                        };
                    }
                }
                _ => break,
            }
        }

        // Space-separated call after a bare dotted path or a `$`-value.
        // `prev_is_ident` covers plain/dotted names; `dollar_callee` covers
        // `$f 1` so the call is built here, with normal operator precedence.
        let prev_is_ident = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .map(|t| t.token == Token::IdentPlain)
            .unwrap_or(false);
        let dollar_callee = self.arg_depth == 0
            && !self.at(Token::LBrace)
            && matches!(
                self.tokens.get(start_pos).map(|t| &t.token),
                Some(Token::Ident)
            )
            && matches!(e, AstNode::Ident { .. });
        // `f (a) b c` is a three-argument call. The first argument is
        // parenthesized, so the previous token is `)`, not an identifier; a call
        // produced by parenthesized arguments must still accept the remaining
        // space-separated ones. Without this, `hanoi ($n - 1) $from $via $to`
        // compiles as `hanoi($n - 1)` followed by a discarded call.
        //
        // This only applies to a call that *ends the current expression*. Inside
        // a call argument (`g (h 1) 2`), the trailing `2` belongs to the outer
        // call, not to `h`; merging it here turned `unwrap_or (err 9) 3` into
        // `unwrap_or(err(9, 3))`.
        let call_from_parens = self.arg_depth == 0 && matches!(e, AstNode::Call { .. });

        if (prev_is_ident || dollar_callee || call_from_parens)
            && (!self.at(Token::LBrace) || self.brace_arg_allowed)
            && matches!(
                e,
                AstNode::Ident { .. } | AstNode::FieldAccess { .. } | AstNode::Call { .. }
            )
            && self.starts_positional_arg()
            && !self.has_newline_before_current()
        {
            // Skip $f.method $x when $x is also a field access ($x.field)
            // because $x.field is a separate arg, not a call to $f.method
            let skip = matches!(e, AstNode::FieldAccess { .. })
                && self.at(Token::Ident)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Dot) | Some(Token::LBracket)
                );
            // Skip { after FieldAccess on a $-variable when it's a block-lambda (not a dict).
            // This prevents `if $x.field { }` from being misparsed as a call
            // `if($x.field, { })`. Dict literals and domain functions (env.override, etc.)
            // are allowed because their FieldAccess target starts with a plain identifier.
            // Check the token stream: if the expression started with Token::Ident ($-var),
            // skip the brace. If it started with Token::IdentPlain (domain function), allow it.
            let skip_brace = if let AstNode::FieldAccess { target, .. } = &e {
                if let AstNode::Ident { .. } = target.as_ref() {
                    // Find the token that produced this ident - check if it was $-prefixed
                    // by looking at the token just before the dot in the token stream
                    let dot_pos = self.tokens.iter().rposition(|t| {
                        matches!(t.token, Token::Dot | Token::OptionalDot)
                            && t.byte_offset
                                < self
                                    .tokens
                                    .get(self.pos - 1)
                                    .map(|t| t.byte_offset)
                                    .unwrap_or(0)
                    });
                    if let Some(dp) = dot_pos {
                        let before_dot = dp.saturating_sub(1);
                        let is_dollar_var = matches!(
                            self.tokens.get(before_dot).map(|t| &t.token),
                            Some(Token::Ident)
                        );
                        is_dollar_var && self.at(Token::LBrace) && !self.lookahead_dict_literal()
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if skip || skip_brace {
                // Do nothing — leave the token for the caller
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
                while self.starts_positional_arg() && !self.has_newline_before_current() {
                    // In a control-flow header the brace is the body, not an
                    // argument.
                    if self.at(Token::LBrace) && !self.brace_arg_allowed {
                        break;
                    }
                    if let Some(items) = self.try_expansion(&args) {
                        return AstNode::DomainCall {
                            span: self.span(),
                            callee,
                            args,
                            items,
                        };
                    }
                    args.push(self.call_arg());
                }
                while self.at(Token::Comma) {
                    self.consume();
                    // An explicit comma continues the argument list across a
                    // newline: `f a: 1,\n  b: 2`.
                    if let Some(items) = self.try_expansion(&args) {
                        return AstNode::DomainCall {
                            span: self.span(),
                            callee,
                            args,
                            items,
                        };
                    }
                    if self.starts_positional_arg() {
                        args.push(self.call_arg());
                    }
                }
                if let Some(items) = self.try_expansion(&args) {
                    return AstNode::DomainCall {
                        span: self.span(),
                        callee,
                        args,
                        items,
                    };
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

    pub(super) fn call_arg(&mut self) -> CallArg {
        // A parenthesized expression directly after a callee (`f(x)`) is not a
        // call. With a space (`f (x)`) it is one grouped argument.
        if self.at(Token::LParen) && self.adjacent_to_previous() {
            let span = self.span();
            let msg = "`(...)` must be separated from the callee by a space — \
             calls use space-separated arguments (`f arg1 arg2`); \
             `f (expr)` passes one grouped expression"
                .to_string();
            self.error("E0005", span.clone(), msg);
            // Consume the `(` to advance past the error
            self.consume();
            // Return a nil placeholder arg
            return CallArg {
                value: AstNode::Nil { span },
                name: None,
            };
        }
        if self.at(Token::IdentPlain) {
            let next = self.pos + 1;
            if matches!(self.tokens.get(next).map(|t| &t.token), Some(Token::Colon)) {
                let name = self.consume().text.clone();
                self.consume();
                // Named args parse full expressions (they're unambiguous)
                self.arg_depth += 1;
                let value = self.expr();
                self.arg_depth -= 1;
                return CallArg {
                    value,
                    name: Some(name),
                };
            }
        }
        // Positional args parse at unary level so binary operators (>, <, ==, +, -)
        // are handled by the precedence climbing above postfix.
        // This means `math.sqrt 16 > 3` parses as `(math.sqrt 16) > 3`, not `math.sqrt (16 > 3)`.
        // Use parentheses for multi-operator args: `math.add (1 + 2)`.
        //
        // `arg_depth` stops a parenthesized call in this position from absorbing
        // the caller's remaining space-separated arguments (see `postfix`).
        self.arg_depth += 1;
        let value = self.unary();
        self.arg_depth -= 1;
        CallArg { value, name: None }
    }

    pub(super) fn pipeline_stage(&mut self, input: AstNode) -> AstNode {
        let span = self.span();
        // Parse the pipeline stage name, which can be a dotted path like `collections.filter`
        let mut name = self.consume().text.clone();
        while self.at(Token::Dot) {
            self.consume(); // consume Dot
            let field = self.consume().text.clone();
            name = format!("{}.{}", name, field);
        }
        // Increment pipeline depth so lambda bodies don't consume the next `|>` stage.
        self.pipeline_depth += 1;
        let arg = if self.looks_like_arg() {
            Some(Box::new(self.expr()))
        } else {
            None
        };
        self.pipeline_depth -= 1;
        AstNode::Pipeline {
            span,
            input: Box::new(input),
            stages: vec![PipelineStage { name, arg }],
        }
    }
}
