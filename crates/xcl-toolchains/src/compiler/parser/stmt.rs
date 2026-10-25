//! Statements and declarations.
//!
//! Top-level items (`program`), assignments, control flow, functions, and
//! domain expansion. Declaration lookaheads live here too: they decide
//! whether the current token starts a definition rather than an expression.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn program(&mut self) -> Vec<AstNode> {
        let mut out = Vec::new();
        while !self.at_eof() {
            if self.error_cap_reached {
                break;
            }
            if self.at(Token::Semi) {
                self.consume();
                continue;
            }
            // Skip stray RBrace at program level — can happen if a block-lambda
            // consumed a closing brace that belonged to an outer scope.
            if self.at(Token::RBrace) {
                self.consume();
                continue;
            }
            let errors_before = self.errors.len();
            out.push(self.statement());
            // If the statement recorded errors, try to synchronise.
            if self.errors.len() > errors_before {
                self.sync_statement();
            }
        }
        out
    }

    pub(super) fn statement(&mut self) -> AstNode {
        match self.current().map(|t| &t.token) {
            Some(Token::Return) => self.ret(),
            Some(Token::Break) => {
                let s = self.span();
                self.consume();
                AstNode::Break { span: s }
            }
            Some(Token::Continue) => {
                let s = self.span();
                self.consume();
                AstNode::Continue { span: s }
            }
            Some(Token::If) => self.if_expr(),
            Some(Token::For) => self.for_expr(),
            Some(Token::While) => self.while_expr(),
            Some(Token::Ident) if self.lookahead_assign() => self.assign(),
            // Member write: `$obj.field = v`, `$arr[i] = v`, and the compound
            // forms. The operator is not directly after the name, so the plain
            // assignment lookahead above cannot see it.
            Some(Token::Ident) | Some(Token::SpecialVar) if self.lookahead_member_assign() => {
                self.member_assign()
            }
            Some(Token::IdentPlain) if self.lookahead_fn_def() => self.fn_def(),
            // Type definition: `Name = { ... }` or `Name = | Variant1 { ... } | Variant2 { ... }`
            Some(Token::IdentPlain) if self.lookahead_type_def() => self.type_def(),
            // Impl block: `Name is Trait { ... }`
            Some(Token::IdentPlain) if self.lookahead_impl_def() => self.impl_def(),
            // `defer { ... }` — keyword-like syntax for deferred closures
            Some(Token::IdentPlain) if self.lookahead_defer() => self.defer_expr(),
            // Member write on a plain identifier (`config.retries = 3`), after
            // the definition lookaheads so `Name = { ... }` keeps its meaning.
            Some(Token::IdentPlain) if self.lookahead_member_assign() => self.member_assign(),
            Some(Token::SpecialVar) | Some(Token::UnsupportedSpecialVar)
                if self.lookahead_assign() =>
            {
                let name = self.current().map(|t| t.text.as_str()).unwrap_or("");
                let span = self.span();
                let msg = format!("special variable `{}` is read-only", name);
                self.error("E0003", span.clone(), msg);
                // Consume the special var and the `=` to advance past the bad statement
                self.consume();
                if self.at(Token::Assign) {
                    self.consume();
                }
                // Return a nil node as a placeholder
                AstNode::Nil { span }
            }
            _ => self.expr_stmt(),
        }
    }

    #[inline]
    pub(super) fn lookahead_defer(&self) -> bool {
        // `defer` followed by `{`
        if self.current().map(|t| t.text != "defer").unwrap_or(true) {
            return false;
        }
        matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::LBrace)
        )
    }

    pub(super) fn defer_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume(); // consume "defer"
        // Parse the block { ... }
        let statements = self.block();
        // Wrap in: defer( Lambda { params: [], body: Block { statements } } )
        let lambda = AstNode::Lambda {
            span: span.clone(),
            params: vec![],
            body: Box::new(AstNode::Block {
                span: span.clone(),
                statements,
            }),
        };
        AstNode::Call {
            span: span.clone(),
            callee: Box::new(AstNode::Ident {
                span,
                name: "defer".to_string(),
            }),
            args: vec![CallArg {
                value: lambda,
                name: None,
            }],
        }
    }

    #[inline]
    pub(super) fn lookahead_type_def(&self) -> bool {
        // Name = { or Name = | ... or Name = Variant | ... or Name = Variant { ... } | ...
        if !matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Assign)
        ) {
            return false;
        }
        match self.tokens.get(self.pos + 2).map(|t| &t.token) {
            Some(Token::LBrace) | Some(Token::Pipe) => true,
            // `Name = IdentPlain |` — enum-style without leading pipe
            // `Name = IdentPlain {` — variant with fields
            Some(Token::IdentPlain) => {
                matches!(
                    self.tokens.get(self.pos + 3).map(|t| &t.token),
                    Some(Token::Pipe) | Some(Token::LBrace)
                )
            }
            _ => false,
        }
    }

    #[inline]
    pub(super) fn lookahead_impl_def(&self) -> bool {
        // Name is Trait {
        if !matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Is)
        ) {
            return false;
        }
        // Check for: Name is IdentPlain {
        if !matches!(
            self.tokens.get(self.pos + 2).map(|t| &t.token),
            Some(Token::IdentPlain)
        ) {
            return false;
        }
        matches!(
            self.tokens.get(self.pos + 3).map(|t| &t.token),
            Some(Token::LBrace)
        )
    }

    pub(super) fn type_def(&mut self) -> AstNode {
        let span = self.span();
        let name = self.consume().text; // IdentPlain
        self.expect(Token::Assign);

        if self.at(Token::Pipe) || self.at(Token::IdentPlain) {
            // Sum type: `Name = | Variant1 | Variant2` or `Name = Variant1 | Variant2`
            let mut variants = Vec::new();
            // If starts with `|`, consume it first (old syntax)
            if self.at(Token::Pipe) {
                self.consume();
            }
            while self.at(Token::IdentPlain) {
                // Check if this IdentPlain is followed by `=` — that means it's a new
                // assignment statement, not a variant name. Stop parsing variants.
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Assign)
                ) {
                    break;
                }
                let vname = self.consume().text;
                let mut fields = Vec::new();
                if self.at(Token::LBrace) {
                    self.consume(); // {
                    while !self.at(Token::RBrace) && !self.at(Token::Pipe) && !self.at_eof() {
                        if self.at(Token::Semi) {
                            self.consume();
                            continue;
                        }
                        let fname = self.consume().text;
                        self.expect(Token::Colon);
                        let fty = self.parse_type();
                        fields.push(TypeField {
                            name: fname,
                            ty: fty,
                            mutable: false,
                        });
                        if self.at(Token::Semi) {
                            self.consume();
                        }
                    }
                    self.expect(Token::RBrace);
                }
                variants.push(SumVariant {
                    name: vname,
                    fields,
                });
                if self.at(Token::Pipe) {
                    self.consume();
                }
            }
            let variant_names: HashSet<String> = variants.iter().map(|v| v.name.clone()).collect();
            self.sum_types.insert(name.clone(), variant_names);
            AstNode::TypeDef {
                span,
                name,
                kind: TypeKind::Sum { variants },
                body: Vec::new(),
            }
        } else {
            // Record or Trait: `Name = { ... }`
            self.expect(Token::LBrace);
            let mut body = Vec::new();
            while !self.at(Token::RBrace) && !self.at_eof() {
                if self.at(Token::Semi) || self.at(Token::Comma) {
                    self.consume();
                    continue;
                }
                // Check if it's a method definition
                if self.at(Token::IdentPlain) && self.lookahead_fn_def() {
                    body.push(self.fn_def());
                } else {
                    // Field definition: `name: type` or `mut name: type`
                    let span = self.span();
                    let mut _mutable = false;
                    if self.at(Token::IdentPlain)
                        && self.current().map(|t| t.text == "mut").unwrap_or(false)
                    {
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
                        value: Box::new(AstNode::Nil {
                            span: Span::empty(),
                        }),
                        type_annotation: None,
                    });
                    if self.at(Token::Semi) {
                        self.consume();
                    }
                }
            }
            self.expect(Token::RBrace);
            // Determine if trait (only methods) or record (has fields)
            let kind = if body
                .iter()
                .all(|n| matches!(n, AstNode::FunctionDef { .. }))
            {
                TypeKind::Trait
            } else {
                TypeKind::Record
            };
            if matches!(kind, TypeKind::Record) {
                self.record_types.insert(name.clone());
            }
            AstNode::TypeDef {
                span,
                name,
                kind,
                body,
            }
        }
    }

    pub(super) fn impl_def(&mut self) -> AstNode {
        let span = self.span();
        let type_name = self.consume().text; // IdentPlain
        self.expect(Token::Is);
        let trait_name = self.consume().text; // IdentPlain
        self.expect(Token::LBrace);
        let mut body = Vec::new();
        while !self.at(Token::RBrace) && !self.at_eof() {
            if self.at(Token::Semi) {
                self.consume();
                continue;
            }
            if self.at(Token::IdentPlain) && self.lookahead_fn_def() {
                body.push(self.fn_def());
            } else {
                // Skip unknown content
                self.consume();
            }
        }
        self.expect(Token::RBrace);
        AstNode::ImplDef {
            span,
            type_name,
            trait_name,
            body,
        }
    }

    #[inline]
    pub(super) fn lookahead_dict_literal(&self) -> bool {
        // self.pos already points past the consumed `{`, so start here.
        self.lookahead_dict_at(self.pos)
    }

    /// Does the brace group starting at `start` look like `{ key: value, ... }`?
    pub(super) fn lookahead_dict_at(&self, start: usize) -> bool {
        let mut i = start;
        while i < self.tokens.len() {
            match self.tokens[i].token {
                Token::RBrace => return false, // empty `{ }` → block-lambda
                Token::Semi | Token::Comma => {
                    i += 1;
                    continue;
                }
                _ => {
                    // Found the first meaningful token — check if next is `:`
                    return matches!(self.tokens.get(i + 1).map(|t| &t.token), Some(Token::Colon));
                }
            }
        }
        false // reached EOF without `}` → block-lambda
    }

    /// Parse a brace-expanded argument list:
    /// `{ expr, name: value; expr; ... }`.
    pub(super) fn expansion_block(&mut self) -> Vec<DomainItem> {
        self.expect(Token::LBrace);
        let mut items = Vec::new();
        while !self.at(Token::RBrace) && !self.at_eof() {
            if self.at(Token::Semi) {
                self.consume();
                continue;
            }
            let before = self.pos;
            let value = self.expr();
            let mut named = Vec::new();
            while self.at(Token::Comma) {
                let is_named = matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::IdentPlain)
                ) && matches!(
                    self.tokens.get(self.pos + 2).map(|t| &t.token),
                    Some(Token::Colon)
                );
                if !is_named {
                    break;
                }
                self.consume(); // ,
                let name = self.consume().text.clone();
                self.consume(); // :
                self.arg_depth += 1;
                let item_value = self.expr();
                self.arg_depth -= 1;
                named.push((name, item_value));
            }
            items.push(DomainItem { value, named });
            if self.at(Token::Semi) {
                self.consume();
            }
            // Never spin on a token `expr` refused to consume.
            if self.pos == before {
                self.consume();
            }
        }
        self.expect(Token::RBrace);
        items
    }

    /// Recognises the expansion form: a brace block with no positional argument
    /// before it. `provider "ctx" { ... }` has a positional argument and stays a
    /// domain block-lambda; `print { ... }` expands.
    pub(super) fn try_expansion(&mut self, args: &[CallArg]) -> Option<Vec<DomainItem>> {
        // A brace in a control-flow header is the body, never an expansion.
        if !self.brace_arg_allowed || !self.at(Token::LBrace) {
            return None;
        }
        if !args.iter().all(|a| a.name.is_some()) {
            return None;
        }
        if self.lookahead_dict_at(self.pos + 1) {
            return None;
        }
        Some(self.expansion_block())
    }

    #[inline]
    pub(super) fn lookahead_assign(&self) -> bool {
        matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Assign)
                | Some(Token::AddAssign)
                | Some(Token::SubAssign)
                | Some(Token::MulAssign)
                | Some(Token::DivAssign)
                | Some(Token::BitOrAssign)
                | Some(Token::BitXorAssign)
                | Some(Token::PowAssign)
                | Some(Token::CoalesceAssign)
                | Some(Token::Colon)
        )
    }

    /// Returns the compound operator at the current token, if any.
    #[inline]
    pub(super) fn compound_operator(&self) -> Option<BinaryOperator> {
        match self.current().map(|t| &t.token) {
            Some(Token::AddAssign) => Some(BinaryOperator::Add),
            Some(Token::SubAssign) => Some(BinaryOperator::Sub),
            Some(Token::MulAssign) => Some(BinaryOperator::Mul),
            Some(Token::DivAssign) => Some(BinaryOperator::Div),
            Some(Token::BitOrAssign) => Some(BinaryOperator::BitOr),
            Some(Token::BitXorAssign) => Some(BinaryOperator::BitXor),
            Some(Token::PowAssign) => Some(BinaryOperator::Pow),
            Some(Token::CoalesceAssign) => Some(BinaryOperator::Coalesce),
            _ => None,
        }
    }

    #[inline]
    fn is_assign_operator(&self) -> bool {
        matches!(
            self.current().map(|t| &t.token),
            Some(Token::Assign)
                | Some(Token::AddAssign)
                | Some(Token::SubAssign)
                | Some(Token::MulAssign)
                | Some(Token::DivAssign)
                | Some(Token::BitOrAssign)
                | Some(Token::BitXorAssign)
                | Some(Token::PowAssign)
                | Some(Token::CoalesceAssign)
        )
    }

    /// Lookahead for a write through a member chain: `$obj.field = v`,
    /// `$arr[i] = v`, `$obj.a[i].b += v`.
    ///
    /// Scans only the target chain (`.` and balanced `[...]`) and then requires
    /// an assignment operator. Anything else — a comparison, a call with
    /// arguments, an operator — is left for the expression parser.
    pub(super) fn lookahead_member_assign(&self) -> bool {
        let mut i = self.pos;
        match self.tokens.get(i).map(|t| &t.token) {
            Some(Token::Ident) | Some(Token::IdentPlain) | Some(Token::SpecialVar) => i += 1,
            _ => return false,
        }

        let mut saw_member = false;
        loop {
            match self.tokens.get(i).map(|t| &t.token) {
                Some(Token::Dot) | Some(Token::OptionalDot) => {
                    match self.tokens.get(i + 1).map(|t| &t.token) {
                        // Field names lex as `IdentPlain`; `$`-prefixed ones are
                        // accepted too so `$obj.$field` is a clear error later
                        // rather than a confusing one here.
                        Some(Token::IdentPlain) | Some(Token::Ident) => {}
                        _ => return false,
                    }
                    saw_member = true;
                    i += 2;
                }
                Some(Token::LBracket) => {
                    let mut depth = 0usize;
                    loop {
                        match self.tokens.get(i).map(|t| &t.token) {
                            Some(Token::LBracket) | Some(Token::LParen) | Some(Token::LBrace) => {
                                depth += 1;
                            }
                            Some(Token::RBracket) | Some(Token::RParen) | Some(Token::RBrace) => {
                                depth -= 1;
                                if depth == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            // A top-level comma makes this an array literal used
                            // as a call argument, not an index (`$a[1, 2]`).
                            Some(Token::Comma) if depth == 1 => return false,
                            Some(Token::Semi) | None => return false,
                            _ => {}
                        }
                        i += 1;
                    }
                    saw_member = true;
                }
                _ => break,
            }
        }

        saw_member
            && matches!(
                self.tokens.get(i).map(|t| &t.token),
                Some(Token::Assign)
                    | Some(Token::AddAssign)
                    | Some(Token::SubAssign)
                    | Some(Token::MulAssign)
                    | Some(Token::DivAssign)
                    | Some(Token::BitOrAssign)
                    | Some(Token::BitXorAssign)
                    | Some(Token::PowAssign)
                    | Some(Token::CoalesceAssign)
            )
    }

    #[inline]
    pub(super) fn lookahead_fn_def(&self) -> bool {
        // Must be: name( ... ) { or name( ... ) ->
        // We scan forward past the parens to check for a block or return type.
        if !matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::LParen)
        ) {
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
            i += 1;
        }
        if depth != 0 {
            return false;
        }
        // After the closing paren, expect { or ->
        matches!(
            self.tokens.get(i).map(|t| &t.token),
            Some(Token::LBrace) | Some(Token::RArrow)
        )
    }

    pub(super) fn assign(&mut self) -> AstNode {
        let span = self.span();
        let name = self.consume().text.trim_start_matches('$').to_string();
        let type_annotation = if self.at(Token::Colon) {
            self.consume();
            Some(self.parse_type())
        } else {
            None
        };

        let compound = self.compound_operator();

        if let Some(op) = compound {
            self.consume();
            let value = Box::new(self.expr());
            return AstNode::Assign {
                span: span.clone(),
                name: name.clone(),
                value: Box::new(AstNode::BinaryOp {
                    span: span.clone(),
                    left: Box::new(AstNode::Ident {
                        span: span.clone(),
                        name,
                    }),
                    op,
                    right: value,
                }),
                type_annotation: None,
            };
        }

        self.expect(Token::Assign);
        let start_pos = self.pos;
        let value = self.expr();
        let value = self.attach_call_args(value, start_pos);
        let value = Box::new(self.bare_call_to_zero_arg(value));
        AstNode::Assign {
            span,
            name,
            value,
            type_annotation,
        }
    }

    /// Parses a write through a member chain: `$obj.field = v`, `$arr[i] = v`,
    /// `$obj.a[i].b += v`.
    ///
    /// The target is parsed with the normal postfix chain, so nested writes and
    /// computed indices keep their usual meaning. A compound operator becomes a
    /// field on the node instead of a duplicated read expression: the compiler
    /// can then evaluate the target exactly once.
    pub(super) fn member_assign(&mut self) -> AstNode {
        let span = self.span();
        let target = self.postfix();
        if !self.is_assign_operator() {
            // The lookahead should have caught this; recovering here keeps a
            // malformed target from consuming the following statement.
            let msg = "expected an assignment operator after the target".to_string();
            self.error("E0009", span, msg);
            return target;
        }
        let op = self.compound_operator();
        self.consume();

        let start_pos = self.pos;
        let value = self.expr();
        let value = self.attach_call_args(value, start_pos);
        let value = Box::new(self.bare_call_to_zero_arg(value));

        match target {
            AstNode::FieldAccess {
                target: object,
                field,
                ..
            } => AstNode::SetField {
                span,
                target: object,
                field,
                value,
                op,
            },
            AstNode::Index {
                target: object,
                index,
                ..
            } => AstNode::SetIndex {
                span,
                target: object,
                index,
                value,
                op,
            },
            other => {
                let msg =
                    "assignment target must be a `$variable`, a field, or an index".to_string();
                self.error("E0009", span, msg);
                // The value expression was already parsed (and is dropped here),
                // so the following statement still starts on a clean token.
                other
            }
        }
    }

    /// If an expression is a bare FieldAccess on a $-variable or a domain function
    /// (starting with IdentPlain), convert it to a zero-arg Call.
    /// This enables scripting-style syntax: `$r.is_ok` and `cloudflare.config`.
    pub(super) fn bare_call_to_zero_arg(&self, e: AstNode) -> AstNode {
        if let AstNode::FieldAccess { target, .. } = &e {
            if let AstNode::Ident { .. } = target.as_ref() {
                let span = self.span();
                return AstNode::Call {
                    span,
                    callee: Box::new(e),
                    args: vec![],
                };
            }
        }
        e
    }

    /// Attaches trailing positional arguments to a `$`-variable callee parsed
    /// in an expression position: `$f 1 2` becomes `Call($f, [1, 2])`.
    ///
    /// Only applies when the expression starts with a `$`-variable and the next
    /// token starts an argument on the same line; `$a + $b`, `$a` at end of
    /// line, and plain identifiers are left alone.
    pub(super) fn attach_call_args(&mut self, callee: AstNode, start_pos: usize) -> AstNode {
        let is_dollar_ident = matches!(
            self.tokens.get(start_pos).map(|t| &t.token),
            Some(Token::Ident)
        );
        if !is_dollar_ident
            || !matches!(&callee, AstNode::Ident { .. })
            || !self.starts_positional_arg()
            || self.has_newline_before_current()
        {
            return callee;
        }
        let span = self.span();
        let mut args = vec![self.call_arg()];
        while self.starts_positional_arg() && !self.has_newline_before_current() {
            args.push(self.call_arg());
        }
        AstNode::Call {
            span,
            callee: Box::new(callee),
            args,
        }
    }

    pub(super) fn ret(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let value = if !self.at_eof() && !self.at(Token::Semi) && !self.at(Token::RBrace) {
            // Don't parse if the next token starts a function definition (nested fn)
            // because expr() would try to parse it as a call
            let is_fn_def = self.at(Token::IdentPlain) && self.lookahead_fn_def();
            if is_fn_def {
                None
            } else {
                let start_pos = self.pos;
                let e = self.expr();
                let e = self.attach_call_args(e, start_pos);
                Some(Box::new(self.bare_call_to_zero_arg(e)))
            }
        } else {
            None
        };
        AstNode::Return { span, value }
    }

    pub(super) fn if_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let allowed = self.brace_arg_allowed;
        self.brace_arg_allowed = false;
        let start = self.pos;
        let condition = self.expr();
        self.brace_arg_allowed = allowed;
        let condition = self.condition_call(condition, start);
        let then_branch = self.block();
        let else_branch = self.else_branch();
        AstNode::If {
            span,
            condition: Box::new(condition),
            then_branch,
            else_branch,
        }
    }

    pub(super) fn else_branch(&mut self) -> Option<Vec<AstNode>> {
        if self.at(Token::Else) {
            self.consume();
            if self.at(Token::If) {
                Some(vec![self.if_expr()])
            } else {
                Some(self.block())
            }
        } else {
            None
        }
    }

    pub(super) fn for_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let variable = self.consume().text.trim_start_matches('$').to_string();
        self.expect(Token::In);
        let allowed = self.brace_arg_allowed;
        self.brace_arg_allowed = false;
        let start = self.pos;
        let iterable = self.expr();
        self.brace_arg_allowed = allowed;
        let iterable = Box::new(self.condition_call(iterable, start));
        let body = self.block();
        AstNode::For {
            span,
            variable,
            iterable,
            body,
        }
    }

    pub(super) fn while_expr(&mut self) -> AstNode {
        let span = self.span();
        self.consume();
        let allowed = self.brace_arg_allowed;
        self.brace_arg_allowed = false;
        let start = self.pos;
        let condition = self.expr();
        self.brace_arg_allowed = allowed;
        let condition = Box::new(self.condition_call(condition, start));
        let body = self.block();
        AstNode::While {
            span,
            condition,
            body,
        }
    }

    /// Resolves an `if`/`while` condition or `for` iterable.
    ///
    /// A bare plain identifier in this position is a zero-argument call
    /// (`if ready { }`), while `$variables` stay value reads. A bare field
    /// access on a `$`-variable is a zero-argument method call (`$r.is_ok`).
    pub(super) fn condition_call(&self, e: AstNode, start_pos: usize) -> AstNode {
        let plain_at = |i: usize| {
            matches!(
                self.tokens.get(i).map(|t| &t.token),
                Some(Token::IdentPlain)
            )
        };
        match e {
            AstNode::Ident { .. } if plain_at(start_pos) => AstNode::Call {
                span: self.span(),
                callee: Box::new(e),
                args: Vec::new(),
            },
            AstNode::UnaryOp { span, op, operand }
                if matches!(op, UnaryOperator::Not)
                    && plain_at(start_pos + 1)
                    && matches!(operand.as_ref(), AstNode::Ident { .. }) =>
            {
                AstNode::UnaryOp {
                    span,
                    op,
                    operand: Box::new(AstNode::Call {
                        span: self.span(),
                        callee: operand,
                        args: Vec::new(),
                    }),
                }
            }
            other => self.bare_call_to_zero_arg(other),
        }
    }

    /// Parse `name(params) { body }` — function definition without `fn` keyword.
    pub(super) fn fn_def(&mut self) -> AstNode {
        let span = self.span();
        let name = self.consume().text.clone();
        self.expect(Token::LParen);
        let mut params = Vec::new();
        while !self.at(Token::RParen) && !self.at_eof() {
            params.push(self.param());
            if self.at(Token::Comma) {
                self.consume();
            }
        }
        self.expect(Token::RParen);
        let return_type = if self.at(Token::RArrow) {
            self.consume();
            Some(self.parse_type())
        } else {
            None
        };
        let body = self.block();
        AstNode::FunctionDef {
            span,
            name,
            params,
            return_type,
            body,
        }
    }

    pub(super) fn param(&mut self) -> Param {
        let span = self.span();
        // Parameters must use $ prefix: $x, $name, etc.
        let t = self.consume();
        if !matches!(&t.token, Token::Ident) {
            let msg = format!(
                "parameter must use $ prefix, got '{}' (use ${} instead)",
                t.text, t.text
            );
            self.error_with_help(
                "E0004",
                t.span.clone(),
                msg,
                format!("prefix the parameter with $: ${}", t.text),
            );
            // Return a dummy param so parsing can continue
            return Param {
                name: t.text.clone(),
                type_annotation: None,
                default_value: None,
                span: t.span,
            };
        }
        let name = t.text.trim_start_matches('$').to_string();
        let type_annotation = if self.at(Token::Colon) {
            self.consume();
            Some(self.parse_type())
        } else {
            None
        };
        let default_value = if self.at(Token::Assign) {
            self.consume();
            Some(Box::new(self.expr()))
        } else {
            None
        };
        Param {
            name,
            type_annotation,
            default_value,
            span,
        }
    }

    pub(super) fn block(&mut self) -> Vec<AstNode> {
        // Nested blocks recurse through `statement`, so they need the same
        // headroom check as expressions.
        if !self.enter_recursion() {
            self.skip_to_matching_brace();
            return Vec::new();
        }
        let out = self.block_inner();
        self.exit_recursion();
        out
    }

    pub(super) fn block_inner(&mut self) -> Vec<AstNode> {
        self.expect(Token::LBrace);
        if self.error_cap_reached {
            // Already at the error cap — drain to the matching brace so the
            // caller does not try to parse this block's contents.
            self.skip_to_matching_brace();
            return Vec::new();
        }
        let mut out = Vec::new();
        while !self.at(Token::RBrace) && !self.at_eof() {
            if self.error_cap_reached {
                break;
            }
            if self.at(Token::Semi) {
                self.consume();
                continue;
            }
            let errors_before = self.errors.len();
            out.push(self.statement());
            // If the statement recorded errors, try to synchronise.
            if self.errors.len() > errors_before {
                self.sync_statement();
            }
        }
        self.expect(Token::RBrace);
        out
    }

    /// Consumes tokens until the `{` that opened the current block is closed.
    ///
    /// Used when parsing is aborted mid-block: without this, the remainder of
    /// the block would be parsed as if it were at the enclosing level, producing
    /// a cascade of secondary errors.
    pub(super) fn skip_to_matching_brace(&mut self) {
        let mut depth = 1usize;
        while !self.at_eof() && depth > 0 {
            match self.current().map(|t| &t.token) {
                Some(Token::LBrace) => depth += 1,
                Some(Token::RBrace) => depth -= 1,
                _ => {}
            }
            self.consume();
        }
    }

    pub(super) fn expr_stmt(&mut self) -> AstNode {
        // The first token of the statement's expression, needed to tell a plain
        // identifier (`fs.cwd`, `work`) from a `$`-variable (`$m.key`).
        let start_pos = self.pos;
        let e = self.expr();
        // $f $x call pattern: if expression is a plain $-ident (not FieldAccess)
        // and next token starts a positional arg, treat it as a call
        if let AstNode::Ident { .. } = &e
            && self.starts_positional_arg()
            && !self.has_newline_before_current()
        {
            let callee = e;
            let mut args = vec![self.call_arg()];
            while self.starts_positional_arg() && !self.has_newline_before_current() {
                args.push(self.call_arg());
            }
            if self.at(Token::Semi) {
                self.consume();
            }
            return AstNode::Call {
                span: self.span(),
                callee: Box::new(callee),
                args,
            };
        }
        // A bare `$f` at a statement boundary calls the function value it holds
        // (the VM leaves the value alone when it is not a function, so `$x` as
        // the last statement still yields the program result).
        let first_token_is_dollar_ident = matches!(
            self.tokens.get(start_pos).map(|t| &t.token),
            Some(Token::Ident)
        );
        if first_token_is_dollar_ident
            && matches!(&e, AstNode::Ident { .. })
            && !self.starts_positional_arg()
            && (self.at_eof()
                || self.at(Token::Semi)
                || self.at(Token::RBrace)
                || self.has_newline_before_current())
        {
            if self.at(Token::Semi) {
                self.consume();
            }
            return AstNode::Call {
                span: self.span(),
                callee: Box::new(e),
                args: Vec::new(),
            };
        }
        // Bare IdentPlain or FieldAccess starting with IdentPlain at a statement
        // boundary is treated as a zero-argument call: `cloudflare.config` ==
        // `cloudflare.config ()`. Only applies to domain functions (IdentPlain),
        // NOT to $-variables (Ident) like `$m.key`.
        // The expression must have NO Call nodes inside (e.g., `cloud.config ().region`
        // is NOT a bare call because it contains a Call).
        fn contains_call(node: &AstNode) -> bool {
            match node {
                AstNode::Call { .. } => true,
                AstNode::FieldAccess { target, .. } => contains_call(target),
                AstNode::Index { target, index, .. } => {
                    contains_call(target) || contains_call(index)
                }
                AstNode::Ident { .. } => false,
                _ => true,
            }
        }
        // The statement's leading token tells us whether the expression is a plain
        // (domain/function) name or a `$`-variable reference.
        let first_token_is_ident_plain = matches!(
            self.tokens.get(start_pos).map(|t| &t.token),
            Some(Token::IdentPlain)
        );
        // Arguments must be on the same line as the callee; a newline means the
        // next identifier starts a new statement, not an argument list.
        let takes_args = self.starts_positional_arg() && !self.has_newline_before_current();
        // Bare domain function call: `cloudflare.config` at statement boundary
        if matches!(&e, AstNode::Ident { .. } | AstNode::FieldAccess { .. })
            && first_token_is_ident_plain
            && !contains_call(&e)
            && !takes_args
            && (self.at_eof()
                || self.at(Token::Semi)
                || self.at(Token::RBrace)
                || self.has_newline_before_current())
        {
            if self.at(Token::Semi) {
                self.consume();
            }
            return AstNode::Call {
                span: self.span(),
                callee: Box::new(e),
                args: vec![],
            };
        }
        // Bare method call on $-variable: `$r.is_ok` at statement boundary
        let first_token_is_dollar_ident = matches!(
            self.tokens.get(start_pos).map(|t| &t.token),
            Some(Token::Ident)
        );
        if let AstNode::FieldAccess { target, .. } = &e {
            if let AstNode::Ident { .. } = target.as_ref() {
                if first_token_is_dollar_ident
                    && !contains_call(&e)
                    && !takes_args
                    && (self.at_eof()
                        || self.at(Token::Semi)
                        || self.at(Token::RBrace)
                        || self.has_newline_before_current())
                {
                    if self.at(Token::Semi) {
                        self.consume();
                    }
                    return AstNode::Call {
                        span: self.span(),
                        callee: Box::new(e),
                        args: vec![],
                    };
                }
            }
        }
        if self.at(Token::Semi) {
            self.consume();
        }
        e
    }

    /// Check if position looks like a bare function definition: `Name( ... ) {` or `Name( ... ) ->`
    pub(super) fn lookahead_fn_def_at(&self, pos: usize) -> bool {
        if !matches!(
            self.tokens.get(pos).map(|t| &t.token),
            Some(Token::IdentPlain)
        ) {
            return false;
        }
        if !matches!(
            self.tokens.get(pos + 1).map(|t| &t.token),
            Some(Token::LParen)
        ) {
            return false;
        }
        // Find matching closing paren
        let mut depth = 1;
        let mut i = pos + 2;
        while i < self.tokens.len() && depth > 0 {
            match self.tokens[i].token {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                i += 1;
            }
        }
        if depth != 0 {
            return false;
        }
        // After closing paren, expect { or ->
        matches!(
            self.tokens.get(i).map(|t| &t.token),
            Some(Token::LBrace) | Some(Token::RArrow)
        )
    }

    /// Check if current position (at LParen) looks like a function definition.
    /// Used by primary() to avoid parsing `name()` as a call when it's a fn def.
    pub(super) fn lookahead_fn_def_from_paren(&self) -> bool {
        // self.pos points at LParen. Scan for matching RParen.
        let mut depth = 1;
        let mut i = self.pos + 1;
        while i < self.tokens.len() && depth > 0 {
            match self.tokens[i].token {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                i += 1;
            }
        }
        if depth != 0 {
            return false;
        }
        // After closing paren, expect { or ->
        matches!(
            self.tokens.get(i).map(|t| &t.token),
            Some(Token::LBrace) | Some(Token::RArrow)
        )
    }
}
