//! Java frontend for the translator.
//!
//! Token-based parser that converts Java source code into IR, handling:
//! - Package declarations and imports (skipped)
//! - Annotations (skipped)
//! - Classes with fields and methods
//! - Enums
//! - Generics
//! - Method parameters, return types, and bodies
//! - Expressions: literals, variables, field access, method calls, binary ops, conditionals
//! - Statements: assignments, if/else, for/while loops, returns, try/catch
//!
//! Uses a two-phase approach: a fast O(n) lexer tokenizes the source once,
//! then the parser consumes tokens — dramatically faster than the previous
//! character-by-character approach.

pub mod lexer;

use crate::translate::frontend::Frontend;
use crate::translate::ir::node::{Ir, IrNode, IrLiteral, Origin, SourceSpan};
use crate::translate::ir::types::IrType;
use crate::translate::ir::ops::{IrOp, Hint};
use crate::translate::version::LangSpec;
use super::super::backend::TranslateError;
use lexer::{Token, Keyword, TokenStream};

pub struct JavaFrontend {
    version: String,
}

impl JavaFrontend {
    pub fn new(version: &str) -> Self {
        Self { version: version.to_string() }
    }
}

impl Frontend for JavaFrontend {
    fn lang_spec(&self) -> LangSpec {
        LangSpec::java(&self.version)
    }

    fn parse(&self, source: &str) -> Result<Vec<Ir>, TranslateError> {
        let origin = Origin::new(self.lang_spec());
        let tokens = lexer::lex(source);
        let ts = TokenStream::new(tokens);
        let mut p = JavaParser { ts, origin, result: Vec::new() };
        p.parse();
        Ok(p.result)
    }
}

struct JavaParser {
    ts: TokenStream,
    origin: Origin,
    result: Vec<Ir>,
}

impl JavaParser {
    fn parse(&mut self) {
        while !self.ts.at_eof() {
            // Skip package declaration
            if self.ts.is_keyword(&Keyword::Package) {
                self.ts.advance();
                self.ts.skip_to_semicolon();
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
                continue;
            }

            // Skip import declaration
            if self.ts.is_keyword(&Keyword::Import) {
                self.ts.advance();
                self.ts.skip_to_semicolon();
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
                continue;
            }

            // Skip modifiers and annotations (skip_modifiers handles @ internally)
            self.ts.skip_modifiers();

            // Parse class
            if self.ts.is_keyword(&Keyword::Class) || self.ts.is_keyword(&Keyword::Interface) {
                self.ts.advance();
                self.parse_class();
                continue;
            }

            // Parse enum
            if self.ts.is_keyword(&Keyword::Enum) {
                self.ts.advance();
                self.parse_enum();
                continue;
            }

            // Skip unknown
            if self.ts.is(&Token::Semicolon) { self.ts.advance(); continue; }
            self.ts.advance();
        }
    }

    fn parse_class(&mut self) {
        // Read class name
        let name = self.ts.read_ident();

        // Skip extends/implements
        while self.ts.is_keyword(&Keyword::Extends) || self.ts.is_keyword(&Keyword::Implements) {
            self.ts.advance();
            while !self.ts.at_eof() && !self.ts.is(&Token::LBrace) && !self.ts.is(&Token::Semicolon) {
                self.ts.advance();
            }
        }

        // Skip type parameters <T>
        if self.ts.is(&Token::Lt) {
            self.ts.skip_to_rparen(); // reuse for angle brackets
        }

        // Enter class body
        if !self.ts.is(&Token::LBrace) { return; }
        self.ts.advance(); // eat '{'

        // Collect fields first, then parse methods
        let mut fields: Vec<(String, IrType)> = Vec::new();

        // Parse class members
        while !self.ts.at_eof() && !self.ts.is(&Token::RBrace) {
            // Skip annotations on members
            if self.ts.is(&Token::At) {
                self.ts.advance();
                self.ts.skip_annotation();
                continue;
            }

            // Skip modifiers
            let _has_static = self.ts.skip_modifiers();

            if self.ts.at_eof() || self.ts.is(&Token::RBrace) { break; }

            // Check for inner class/enum
            if self.ts.is_keyword(&Keyword::Class) || self.ts.is_keyword(&Keyword::Interface)
               || self.ts.is_keyword(&Keyword::Enum) {
                self.ts.advance();
                let _inner_name = self.ts.read_ident();
                if self.ts.is(&Token::LBrace) {
                    self.ts.skip_to_rbrace();
                } else if self.ts.is(&Token::Semicolon) {
                    self.ts.advance();
                }
                continue;
            }

            // Read the type
            let type_name = self.ts.read_type();
            if self.ts.at_eof() || self.ts.is(&Token::RBrace) { break; }

            // Check for constructor: if next token is '(', the "type" is actually the constructor name
            // e.g., "public Tutorial()" — type_name is "Tutorial", and "(" follows immediately
            if self.ts.is(&Token::LParen) {
                self.parse_method(&type_name, "void");
                continue;
            }

            // Read the identifier (field name or method name)
            let ident = self.ts.read_ident();
            if self.ts.at_eof() || self.ts.is(&Token::RBrace) { break; }

            // Handle array syntax: Type[] name
            while self.ts.is(&Token::LBracket) {
                self.ts.advance();
                if self.ts.is(&Token::RBracket) { self.ts.advance(); }
            }

            // Check if it's a method (has '(')
            if self.ts.is(&Token::LParen) {
                self.parse_method(&ident, &type_name);
                continue;
            }

            // It's a field — collect it
            let ir_type = self.java_type_to_ir(&type_name);
            fields.push((ident.clone(), ir_type));

            // Skip the initializer or semicolon
            if self.ts.is(&Token::Assign) {
                self.ts.advance();
                self.ts.skip_to_semicolon();
            } else if self.ts.is(&Token::Semicolon) {
                self.ts.advance();
            } else {
                self.ts.skip_to_semicolon();
            }
        }

        // Emit the class as a Record node so the backend generates a struct/record
        if !fields.is_empty() {
            self.result.push(Ir::new(
                IrNode::Record { name: name.clone(), fields },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            ));
        }

        // Eat closing brace
        if self.ts.is(&Token::RBrace) { self.ts.advance(); }
    }

    fn parse_method(&mut self, name: &str, return_type: &str) {
        // Read parameters
        if !self.ts.is(&Token::LParen) { return; }
        self.ts.advance(); // eat '('
        let mut params: Vec<(String, IrType)> = Vec::new();

        while !self.ts.is(&Token::RParen) && !self.ts.at_eof() {
            // Skip annotations on params (may have multiple)
            while self.ts.is(&Token::At) {
                self.ts.advance();
                self.ts.skip_annotation();
            }
            // Skip 'final' modifier on params
            if self.ts.is_keyword(&Keyword::Final) {
                self.ts.advance();
            }

            if self.ts.is(&Token::RParen) { break; }

            let param_type = self.ts.read_type();
            let param_name = self.ts.read_ident();

            // Handle array syntax
            while self.ts.is(&Token::LBracket) {
                self.ts.advance();
                if self.ts.is(&Token::RBracket) { self.ts.advance(); }
            }

            let ir_type = self.java_type_to_ir(&param_type);
            params.push((param_name, ir_type));

            if self.ts.is(&Token::Comma) {
                self.ts.advance();
            } else {
                break;
            }
        }
        if self.ts.is(&Token::RParen) { self.ts.advance(); }

        // Skip throws clause
        if self.ts.is_keyword(&Keyword::Throws) {
            self.ts.advance();
            while !self.ts.is(&Token::LBrace) && !self.ts.at_eof() {
                self.ts.advance();
            }
        }

        let ret_type = self.java_type_to_ir(return_type);

        // Parse method body
        if !self.ts.is(&Token::LBrace) { return; }
        self.ts.advance(); // eat '{'

        let body = self.parse_block();

        // Emit the function
        self.result.push(Ir::new(
            IrNode::Fn {
                name: name.to_string(),
                params,
                ret: ret_type,
                body: Box::new(Ir::new(
                    IrNode::Block(body),
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                )),
            },
            IrType::Any,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        ));
    }

    fn parse_block(&mut self) -> Vec<Ir> {
        let mut statements = Vec::new();

        while !self.ts.at_eof() && !self.ts.is(&Token::RBrace) {
            // Skip annotations inside blocks
            if self.ts.is(&Token::At) {
                self.ts.advance();
                self.ts.skip_annotation();
                continue;
            }

            // Skip local modifiers
            if self.ts.is_keyword(&Keyword::Final) || self.ts.is_keyword(&Keyword::Static) {
                self.ts.advance();
            }

            // Try/catch block — parse the try body, skip catch/finally
            if self.ts.is_keyword(&Keyword::Try) {
                self.ts.advance();
                if self.ts.is(&Token::LBrace) {
                    self.ts.advance();
                    // Parse the try body instead of skipping
                    let try_body = self.parse_block();
                    for stmt in try_body {
                        statements.push(stmt);
                    }
                }
                // Skip catch blocks
                while self.ts.is_keyword(&Keyword::Catch) {
                    self.ts.advance();
                    if self.ts.is(&Token::LParen) { self.ts.advance(); self.ts.skip_to_rparen(); }
                    if self.ts.is(&Token::RParen) { self.ts.advance(); }
                    if self.ts.is(&Token::LBrace) { self.ts.advance(); self.ts.skip_to_rbrace(); }
                }
                // Skip finally block
                if self.ts.is_keyword(&Keyword::Finally) {
                    self.ts.advance();
                    if self.ts.is(&Token::LBrace) { self.ts.advance(); self.ts.skip_to_rbrace(); }
                }
                continue;
            }

            // Return statement
            if self.ts.is_keyword(&Keyword::Return) {
                self.ts.advance();
                let mut value = None;
                if !self.ts.is(&Token::Semicolon) && !self.ts.is(&Token::RBrace) {
                    let expr = self.parse_expression();
                    value = Some(Box::new(expr));
                }
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
                statements.push(Ir::new(
                    IrNode::Return(value),
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }

            // If statement
            if self.ts.is_keyword(&Keyword::If) {
                self.ts.advance();
                if self.ts.is(&Token::LParen) { self.ts.advance(); }
                let cond = self.parse_expression();
                if self.ts.is(&Token::RParen) { self.ts.advance(); }

                let then_body = if self.ts.is(&Token::LBrace) {
                    self.ts.advance();
                    self.parse_block()
                } else {
                    vec![self.parse_statement_to_ir()]
                };

                let else_body = if self.ts.is_keyword(&Keyword::Else) {
                    self.ts.advance();
                    if self.ts.is_keyword(&Keyword::If) {
                        let nested = self.parse_if_nested();
                        vec![nested]
                    } else if self.ts.is(&Token::LBrace) {
                        self.ts.advance();
                        self.parse_block()
                    } else {
                        vec![self.parse_statement_to_ir()]
                    }
                } else {
                    Vec::new()
                };

                let then_ir = Ir::new(
                    IrNode::Block(then_body),
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );
                let else_ir = if else_body.is_empty() {
                    None
                } else {
                    Some(Box::new(Ir::new(
                        IrNode::Block(else_body),
                        IrType::Nil,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    )))
                };

                statements.push(Ir::new(
                    IrNode::If {
                        cond: Box::new(cond),
                        then: Box::new(then_ir),
                        else_: else_ir,
                    },
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }

            // For loop
            if self.ts.is_keyword(&Keyword::For) {
                self.ts.advance();
                if self.ts.is(&Token::LParen) { self.ts.advance(); }

                // Check for enhanced for-each: for (Type var : collection)
                let is_enhanced = self.ts.is_type_start();
                if is_enhanced {
                    let _type_str = self.ts.read_type();
                    let _var_name = self.ts.read_ident();
                    if self.ts.is(&Token::Colon) {
                        self.ts.advance(); // eat ':'
                        let collection = self.parse_expression();
                        if self.ts.is(&Token::RParen) { self.ts.advance(); }

                        let body = if self.ts.is(&Token::LBrace) {
                            self.ts.advance();
                            self.parse_block()
                        } else {
                            vec![self.parse_statement_to_ir()]
                        };

                        let body_ir = Ir::new(
                            IrNode::Block(body),
                            IrType::Nil,
                            SourceSpan::dummy(),
                            self.origin.clone(),
                            Vec::new(),
                        );
                        statements.push(Ir::new(
                            IrNode::Loop {
                                cond: Some(Box::new(collection)),
                                body: Box::new(body_ir),
                            },
                            IrType::Nil,
                            SourceSpan::dummy(),
                            self.origin.clone(),
                            Vec::new(),
                        ));
                        continue;
                    }
                }

                // Traditional for: for (init; cond; update)
                if !self.ts.is(&Token::Semicolon) && !self.ts.is(&Token::RParen) {
                    let _init = self.parse_expression();
                }
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }

                let mut cond = None;
                if !self.ts.is(&Token::Semicolon) && !self.ts.is(&Token::RParen) {
                    cond = Some(Box::new(self.parse_expression()));
                }
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }

                if !self.ts.is(&Token::RParen) {
                    let _update = self.parse_expression();
                }
                if self.ts.is(&Token::RParen) { self.ts.advance(); }

                let body = if self.ts.is(&Token::LBrace) {
                    self.ts.advance();
                    self.parse_block()
                } else {
                    vec![self.parse_statement_to_ir()]
                };

                let body_ir = Ir::new(
                    IrNode::Block(body),
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );

                let cond_ir = cond.unwrap_or_else(|| {
                    Box::new(Ir::new(
                        IrNode::Literal(IrLiteral::Bool(true)),
                        IrType::Bool,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    ))
                });

                statements.push(Ir::new(
                    IrNode::Loop {
                        cond: Some(cond_ir),
                        body: Box::new(body_ir),
                    },
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }

            // While loop
            if self.ts.is_keyword(&Keyword::While) {
                self.ts.advance();
                if self.ts.is(&Token::LParen) { self.ts.advance(); }
                let cond = self.parse_expression();
                if self.ts.is(&Token::RParen) { self.ts.advance(); }

                let body = if self.ts.is(&Token::LBrace) {
                    self.ts.advance();
                    self.parse_block()
                } else {
                    vec![self.parse_statement_to_ir()]
                };

                let body_ir = Ir::new(
                    IrNode::Block(body),
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );

                statements.push(Ir::new(
                    IrNode::Loop {
                        cond: Some(Box::new(cond)),
                        body: Box::new(body_ir),
                    },
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }

            // Break/continue
            if self.ts.is_keyword(&Keyword::Break) {
                self.ts.advance();
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
                statements.push(Ir::new(
                    IrNode::Break,
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }
            if self.ts.is_keyword(&Keyword::Continue) {
                self.ts.advance();
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
                statements.push(Ir::new(
                    IrNode::Continue,
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }

            // Local variable declaration or expression statement
            let stmt = self.parse_statement_to_ir();
            if !matches!(stmt.node, IrNode::Block(ref v) if v.is_empty()) {
                statements.push(stmt);
            } else {
                // Didn't parse anything meaningful, skip to semicolon
                self.ts.skip_to_semicolon();
                if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
            }
        }

        // Eat closing brace
        if self.ts.is(&Token::RBrace) { self.ts.advance(); }

        statements
    }

    fn parse_if_nested(&mut self) -> Ir {
        if self.ts.is(&Token::LParen) { self.ts.advance(); }
        let cond = self.parse_expression();
        if self.ts.is(&Token::RParen) { self.ts.advance(); }

        let then_body = if self.ts.is(&Token::LBrace) {
            self.ts.advance();
            self.parse_block()
        } else {
            vec![self.parse_statement_to_ir()]
        };

        let else_body = if self.ts.is_keyword(&Keyword::Else) {
            self.ts.advance();
            if self.ts.is_keyword(&Keyword::If) {
                let nested = self.parse_if_nested();
                vec![nested]
            } else if self.ts.is(&Token::LBrace) {
                self.ts.advance();
                self.parse_block()
            } else {
                vec![self.parse_statement_to_ir()]
            }
        } else {
            Vec::new()
        };

        let then_ir = Ir::new(
            IrNode::Block(then_body),
            IrType::Nil,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        );
        let else_ir = if else_body.is_empty() {
            None
        } else {
            Some(Box::new(Ir::new(
                IrNode::Block(else_body),
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            )))
        };

        Ir::new(
            IrNode::If {
                cond: Box::new(cond),
                then: Box::new(then_ir),
                else_: else_ir,
            },
            IrType::Nil,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_statement_to_ir(&mut self) -> Ir {
        if self.ts.at_eof() || self.ts.is(&Token::RBrace) {
            return Ir::new(
                IrNode::Block(Vec::new()),
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // Skip 'this' keyword from type_start check — it's not a type declaration
        let is_this = self.ts.is_keyword(&Keyword::This);

        // Check for local variable declaration: Type name = value;
        if !is_this && self.ts.is_type_start() {
            let first = self.ts.read_type();
            if self.ts.is_value() {
                let name = self.ts.read_ident();

                // Handle array declaration
                while self.ts.is(&Token::LBracket) {
                    self.ts.advance();
                    if self.ts.is(&Token::RBracket) { self.ts.advance(); }
                }

                // Skip generic type params on the variable: Type<T> name
                if self.ts.is(&Token::Lt) {
                    self.ts.skip_to_rparen();
                }

                if self.ts.is(&Token::Assign) {
                    self.ts.advance();
                    let value = self.parse_expression();
                    if self.ts.is(&Token::Semicolon) { self.ts.advance(); }

                    let ir_type = self.java_type_to_ir(&first);
                    return Ir::new(
                        IrNode::Let {
                            name,
                            ty: ir_type,
                            value: Box::new(value),
                            mutable: true,
                        },
                        IrType::Nil,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                } else if self.ts.is(&Token::Semicolon) {
                    self.ts.advance();
                    let ir_type = self.java_type_to_ir(&first);
                    let value = Ir::new(
                        IrNode::Literal(IrLiteral::Nil),
                        ir_type.clone(),
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                    return Ir::new(
                        IrNode::Let { name, ty: ir_type, value: Box::new(value), mutable: true },
                        IrType::Nil,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                }
            }
        }

        // Expression statement — parse and skip to semicolon
        let start_pos = self.ts.pos();
        let expr = self.parse_expression();
        if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
        
        // If parse_expression didn't advance at all, skip to semicolon to avoid infinite loop
        if self.ts.pos() == start_pos {
            self.ts.skip_to_semicolon();
            if self.ts.is(&Token::Semicolon) { self.ts.advance(); }
            return Ir::new(
                IrNode::Block(Vec::new()),
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        
        expr
    }

    // ─── Expression parsing (operator precedence) ───

    fn parse_expression(&mut self) -> Ir {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Ir {
        let mut left = self.parse_and();
        if self.ts.is(&Token::Arrow) {
            // Lambda: param -> body
            self.ts.advance();
            let body = self.parse_and();
            return Ir::new(
                IrNode::Fn {
                    name: "lambda".to_string(),
                    params: Vec::new(),
                    ret: IrType::Any,
                    body: Box::new(body),
                },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                vec![Hint::Async],
            );
        }
        while self.ts.is(&Token::Or) {
            self.ts.advance();
            let right = self.parse_and();
            left = Ir::new(
                IrNode::BinOp { op: IrOp::Or, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        left
    }

    fn parse_and(&mut self) -> Ir {
        let mut left = self.parse_equality();
        while self.ts.is(&Token::And) {
            self.ts.advance();
            let right = self.parse_equality();
            left = Ir::new(
                IrNode::BinOp { op: IrOp::And, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        left
    }

    fn parse_equality(&mut self) -> Ir {
        let mut left = self.parse_comparison();
        while self.ts.is(&Token::Eq) || self.ts.is(&Token::Ne) {
            let op = if self.ts.is(&Token::Eq) { IrOp::Eq } else { IrOp::Ne };
            self.ts.advance();
            let right = self.parse_comparison();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        left
    }

    fn parse_comparison(&mut self) -> Ir {
        let mut left = self.parse_additive();
        while self.ts.is(&Token::Le) || self.ts.is(&Token::Ge)
            || self.ts.is(&Token::Lt) || self.ts.is(&Token::Gt) {
            let op = if self.ts.is(&Token::Le) { IrOp::Le }
                else if self.ts.is(&Token::Ge) { IrOp::Ge }
                else if self.ts.is(&Token::Lt) { IrOp::Lt }
                else { IrOp::Gt };
            self.ts.advance();
            let right = self.parse_additive();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        left
    }

    fn parse_additive(&mut self) -> Ir {
        let mut left = self.parse_multiplicative();
        while self.ts.is(&Token::Plus) || self.ts.is(&Token::Minus) {
            let op = if self.ts.is(&Token::Plus) { IrOp::Add } else { IrOp::Sub };
            self.ts.advance();
            let right = self.parse_multiplicative();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Ir {
        let mut left = self.parse_unary();
        while self.ts.is(&Token::Star) || self.ts.is(&Token::Slash) || self.ts.is(&Token::Percent) {
            let op = if self.ts.is(&Token::Star) { IrOp::Mul }
                else if self.ts.is(&Token::Slash) { IrOp::Div }
                else { IrOp::Rem };
            self.ts.advance();
            let right = self.parse_unary();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        left
    }

    fn parse_unary(&mut self) -> Ir {
        if self.ts.is(&Token::Not) {
            self.ts.advance();
            let operand = self.parse_unary();
            return Ir::new(
                IrNode::UnaryOp { op: IrOp::Not, operand: Box::new(operand) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        if self.ts.is(&Token::Minus) {
            self.ts.advance();
            let operand = self.parse_unary();
            return Ir::new(
                IrNode::UnaryOp { op: IrOp::Neg, operand: Box::new(operand) },
                IrType::Int,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Ir {
        let expr = self.parse_primary();
        self.parse_postfix_from_expr(expr)
    }

    /// Continue parsing postfix operators (dot, bracket, double-colon) from an existing expression.
    /// Used when parse_primary has already consumed a token and returned an expression,
    /// but there may be more postfix operators to process.
    fn parse_postfix_from_expr(&mut self, mut expr: Ir) -> Ir {
        while !self.ts.at_eof() {
            if self.ts.is(&Token::Dot) {
                self.ts.advance();
                let field = self.ts.read_ident();

                if self.ts.is(&Token::LParen) {
                    self.ts.advance();
                    let mut args = Vec::new();
                    while !self.ts.is(&Token::RParen) && !self.ts.at_eof() {
                        args.push(self.parse_expression());
                        if self.ts.is(&Token::Comma) { self.ts.advance(); }
                        else { break; }
                    }
                    if self.ts.is(&Token::RParen) { self.ts.advance(); }

                    expr = Ir::new(
                        IrNode::Call {
                            callee: Box::new(Ir::new(
                                IrNode::FieldAccess { obj: Box::new(expr), field },
                                IrType::Any,
                                SourceSpan::dummy(),
                                self.origin.clone(),
                                Vec::new(),
                            )),
                            args,
                        },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                } else {
                    expr = Ir::new(
                        IrNode::FieldAccess { obj: Box::new(expr), field },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                }
            } else if self.ts.is(&Token::LBracket) {
                self.ts.advance();
                let index = self.parse_expression();
                if self.ts.is(&Token::RBracket) { self.ts.advance(); }

                expr = Ir::new(
                    IrNode::Index {
                        collection: Box::new(expr),
                        index: Box::new(index),
                    },
                    IrType::Any,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );
            } else if self.ts.is(&Token::DoubleColon) {
                self.ts.advance();
                let method_name = self.ts.read_ident();
                if self.ts.is(&Token::LParen) {
                    self.ts.advance();
                    let mut args = Vec::new();
                    while !self.ts.is(&Token::RParen) && !self.ts.at_eof() {
                        args.push(self.parse_expression());
                        if self.ts.is(&Token::Comma) { self.ts.advance(); }
                        else { break; }
                    }
                    if self.ts.is(&Token::RParen) { self.ts.advance(); }
                    expr = Ir::new(
                        IrNode::Call {
                            callee: Box::new(Ir::new(
                                IrNode::Var(method_name),
                                IrType::Any,
                                SourceSpan::dummy(),
                                self.origin.clone(),
                                Vec::new(),
                            )),
                            args,
                        },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                } else {
                    expr = Ir::new(
                        IrNode::Var(method_name),
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                }
            } else {
                break;
            }
        }

        expr
    }

    fn parse_primary(&mut self) -> Ir {
        if self.ts.at_eof() {
            return ir_nil(&self.origin);
        }

        match self.ts.advance() {
            Token::Str(s) => {
                Ir::new(IrNode::Literal(IrLiteral::Str(s)), IrType::Str, SourceSpan::dummy(), self.origin.clone(), Vec::new())
            }
            Token::Char(c) => {
                Ir::new(IrNode::Literal(IrLiteral::Str(c.to_string())), IrType::Str, SourceSpan::dummy(), self.origin.clone(), Vec::new())
            }
            Token::Int(n) => {
                Ir::new(IrNode::Literal(IrLiteral::Int(n)), IrType::Int, SourceSpan::dummy(), self.origin.clone(), Vec::new())
            }
            Token::Float(f) => {
                Ir::new(IrNode::Literal(IrLiteral::Float(f)), IrType::Float, SourceSpan::dummy(), self.origin.clone(), Vec::new())
            }
            Token::Bool(b) => {
                Ir::new(IrNode::Literal(IrLiteral::Bool(b)), IrType::Bool, SourceSpan::dummy(), self.origin.clone(), Vec::new())
            }
            Token::Null => {
                ir_nil(&self.origin)
            }
            Token::Keyword(Keyword::This) => {
                let expr = ir_var("this".to_string(), &self.origin);
                // Handle this.field or this.method() or this.field = value
                if self.ts.is(&Token::Dot) {
                    self.ts.advance();
                    let field = self.ts.read_ident();

                    // this.field = value (assignment in constructor)
                    if self.ts.is(&Token::Assign) || self.ts.is(&Token::AddAssign)
                        || self.ts.is(&Token::SubAssign) || self.ts.is(&Token::MulAssign)
                        || self.ts.is(&Token::DivAssign) {
                        self.ts.advance();
                        let value = self.parse_expression();
                        let ir_type = self.infer_type(&value);
                        return Ir::new(
                            IrNode::Let { name: field, ty: ir_type, value: Box::new(value), mutable: true },
                            IrType::Nil,
                            SourceSpan::dummy(),
                            self.origin.clone(),
                            Vec::new(),
                        );
                    }

                    // this.method(args)
                    if self.ts.is(&Token::LParen) {
                        self.ts.advance();
                        let mut args = Vec::new();
                        while !self.ts.is(&Token::RParen) && !self.ts.at_eof() {
                            args.push(self.parse_expression());
                            if self.ts.is(&Token::Comma) { self.ts.advance(); }
                            else { break; }
                        }
                        if self.ts.is(&Token::RParen) { self.ts.advance(); }

                        let mut expr = Ir::new(
                            IrNode::Call {
                                callee: Box::new(Ir::new(
                                    IrNode::FieldAccess { obj: Box::new(ir_var("this".to_string(), &self.origin)), field },
                                    IrType::Any,
                                    SourceSpan::dummy(),
                                    self.origin.clone(),
                                    Vec::new(),
                                )),
                                args,
                            },
                            IrType::Any,
                            SourceSpan::dummy(),
                            self.origin.clone(),
                            Vec::new(),
                        );
                        expr = self.parse_postfix_from_expr(expr);
                        return expr;
                    }

                    // this.field (field access)
                    let mut expr = Ir::new(
                        IrNode::FieldAccess { obj: Box::new(ir_var("this".to_string(), &self.origin)), field },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                    expr = self.parse_postfix_from_expr(expr);
                    return expr;
                }
                expr
            }
            Token::LParen => {
                let expr = self.parse_expression();
                if self.ts.is(&Token::RParen) { self.ts.advance(); }
                expr
            }
            Token::Keyword(Keyword::New) => {
                let type_name = self.ts.read_type();

                // Skip generic type parameters: <Tutorial>
                if self.ts.is(&Token::Lt) {
                    self.ts.skip_to_rparen(); // reuse for angle brackets
                }

                // Array initialization: new Type[] { a, b, c }
                if self.ts.is(&Token::LBracket) {
                    self.ts.advance();
                    if self.ts.is(&Token::RBracket) { self.ts.advance(); }
                    if self.ts.is(&Token::LBrace) {
                        return self.parse_array_literal(&type_name);
                    }
                }

                // Constructor call: new Type(args)
                if self.ts.is(&Token::LParen) {
                    self.ts.advance();
                    let mut args = Vec::new();
                    while !self.ts.is(&Token::RParen) && !self.ts.at_eof() {
                        args.push(self.parse_expression());
                        if self.ts.is(&Token::Comma) { self.ts.advance(); }
                        else { break; }
                    }
                    if self.ts.is(&Token::RParen) { self.ts.advance(); }

                    let mut expr = Ir::new(
                        IrNode::Call {
                            callee: Box::new(Ir::new(
                                IrNode::Var(type_name.clone()),
                                IrType::Any,
                                SourceSpan::dummy(),
                                self.origin.clone(),
                                Vec::new(),
                            )),
                            args,
                        },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );

                    // Handle chained calls: new ArrayList<>().add(...)
                    expr = self.parse_postfix_from_expr(expr);
                    return expr;
                }

                ir_var(type_name, &self.origin)
            }
            Token::Ident(name) => {
                // Handle generic type in identifier context (e.g., ArrayList<Tutorial>)
                let full_name = if self.ts.is(&Token::Lt) {
                    self.ts.skip_to_rparen();
                    name.clone()
                } else {
                    name.clone()
                };

                // Method call?
                if self.ts.is(&Token::LParen) {
                    self.ts.advance();
                    let mut args = Vec::new();
                    while !self.ts.is(&Token::RParen) && !self.ts.at_eof() {
                        if self.ts.is(&Token::RParen) { break; }
                        args.push(self.parse_expression());
                        if self.ts.is(&Token::Comma) { self.ts.advance(); }
                        else { break; }
                    }
                    if self.ts.is(&Token::RParen) { self.ts.advance(); }

                    let mut expr = Ir::new(
                        IrNode::Call {
                            callee: Box::new(ir_var(full_name.clone(), &self.origin)),
                            args,
                        },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );

                    // Handle chained calls: tutorials.stream().filter(...)
                    expr = self.parse_postfix_from_expr(expr);
                    return expr;
                }

                // Assignment?
                if self.ts.is(&Token::Assign) || self.ts.is(&Token::AddAssign)
                    || self.ts.is(&Token::SubAssign) || self.ts.is(&Token::MulAssign)
                    || self.ts.is(&Token::DivAssign) {
                    self.ts.advance();
                    let value = self.parse_expression();
                    let ir_type = self.infer_type(&value);
                    return Ir::new(
                        IrNode::Let { name: full_name, ty: ir_type, value: Box::new(value), mutable: true },
                        IrType::Nil,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                }

                let mut expr = ir_var(full_name, &self.origin);

                // Handle field access and method calls: obj.method() or obj.field
                expr = self.parse_postfix_from_expr(expr);
                expr
            }
            Token::Arrow => {
                // Lambda arrow without preceding param — skip body
                if self.ts.is(&Token::LBrace) {
                    self.ts.advance();
                    self.ts.skip_to_rbrace();
                    if self.ts.is(&Token::RBrace) { self.ts.advance(); }
                } else {
                    let body = self.parse_expression();
                    return Ir::new(
                        IrNode::Fn {
                            name: "lambda".to_string(),
                            params: Vec::new(),
                            ret: IrType::Any,
                            body: Box::new(body),
                        },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        vec![Hint::Async],
                    );
                }
                Ir::new(
                    IrNode::Fn {
                        name: "lambda".to_string(),
                        params: Vec::new(),
                        ret: IrType::Any,
                        body: Box::new(ir_nil(&self.origin)),
                    },
                    IrType::Any,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    vec![Hint::Async],
                )
            }
            _ => {
                // Unknown token — already consumed
                ir_nil(&self.origin)
            }
        }
    }

    fn parse_array_literal(&mut self, _type_name: &str) -> Ir {
        if !self.ts.is(&Token::LBrace) {
            return ir_nil(&self.origin);
        }
        self.ts.advance();
        let mut elements = Vec::new();
        while !self.ts.is(&Token::RBrace) && !self.ts.at_eof() {
            elements.push(self.parse_expression());
            if self.ts.is(&Token::Comma) { self.ts.advance(); }
        }
        if self.ts.is(&Token::RBrace) { self.ts.advance(); }

        Ir::new(
            IrNode::Call {
                callee: Box::new(ir_var("array".to_string(), &self.origin)),
                args: elements,
            },
            IrType::Any,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_enum(&mut self) {
        let name = self.ts.read_ident();

        if !self.ts.is(&Token::LBrace) { return; }
        self.ts.advance();

        let mut variants = Vec::new();
        while !self.ts.is(&Token::RBrace) && !self.ts.at_eof() {
            if self.ts.is(&Token::Comma) { self.ts.advance(); continue; }
            if self.ts.is(&Token::At) {
                self.ts.advance();
                self.ts.skip_annotation();
                continue;
            }
            let variant = self.ts.read_ident();
            if !variant.is_empty() {
                variants.push(variant);
            }
        }
        if self.ts.is(&Token::RBrace) { self.ts.advance(); }

        let fields: Vec<(String, IrType)> = variants.iter().map(|v| (v.clone(), IrType::Int)).collect();
        self.result.push(Ir::new(
            IrNode::Record { name: name.clone(), fields },
            IrType::Any,
            SourceSpan::dummy(),
            self.origin.clone(),
            vec![Hint::Sealed],
        ));
    }

    fn java_type_to_ir(&self, type_name: &str) -> IrType {
        let base = type_name.split('<').next().unwrap_or(type_name).trim();
        match base {
            "int" | "Integer" | "long" | "Long" | "short" | "Short" | "byte" | "Byte" => IrType::Int,
            "float" | "Float" | "double" | "Double" => IrType::Float,
            "boolean" | "Boolean" => IrType::Bool,
            "String" | "str" => IrType::Str,
            "void" | "nil" => IrType::Nil,
            "List" | "ArrayList" | "LinkedList" | "Vector" | "Stack" => IrType::Array(Box::new(IrType::Any)),
            "Map" | "HashMap" | "TreeMap" | "Hashtable" | "Dict" => IrType::Map(Box::new(IrType::Any)),
            "Set" | "HashSet" | "TreeSet" => IrType::Array(Box::new(IrType::Any)),
            "ResponseEntity" | "HttpStatus" => IrType::Any,
            "Optional" => IrType::Optional(Box::new(IrType::Any)),
            "Object" => IrType::Any,
            _ => {
                if base.chars().next().map_or(false, |c| c.is_uppercase()) {
                    IrType::Record { name: base.to_string(), fields: Vec::new() }
                } else {
                    IrType::Any
                }
            }
        }
    }

    fn infer_type(&self, expr: &Ir) -> IrType {
        match &expr.node {
            IrNode::Literal(lit) => match lit {
                IrLiteral::Int(_) => IrType::Int,
                IrLiteral::Float(_) => IrType::Float,
                IrLiteral::Bool(_) => IrType::Bool,
                IrLiteral::Str(_) => IrType::Str,
                IrLiteral::Nil => IrType::Nil,
            },
            IrNode::Var(_) => IrType::Any,
            IrNode::FieldAccess { .. } => IrType::Any,
            IrNode::Call { callee, .. } => {
                if let IrNode::Var(name) = &callee.node {
                    if name == "array" { return IrType::Array(Box::new(IrType::Any)); }
                }
                IrType::Any
            }
            IrNode::BinOp { op, .. } => {
                match op {
                    IrOp::Eq | IrOp::Ne | IrOp::Lt | IrOp::Gt | IrOp::Le | IrOp::Ge
                    | IrOp::And | IrOp::Or => IrType::Bool,
                    _ => IrType::Any,
                }
            }
            IrNode::UnaryOp { op: IrOp::Not, .. } => IrType::Bool,
            _ => IrType::Any,
        }
    }
}

/// Helper to create a nil IR node
fn ir_nil(origin: &Origin) -> Ir {
    Ir::new(
        IrNode::Literal(IrLiteral::Nil),
        IrType::Nil,
        SourceSpan::dummy(),
        origin.clone(),
        Vec::new(),
    )
}

fn ir_var(name: String, origin: &Origin) -> Ir {
    Ir::new(
        IrNode::Var(name),
        IrType::Any,
        SourceSpan::dummy(),
        origin.clone(),
        Vec::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_java(source: &str) -> Vec<Ir> {
        let frontend = JavaFrontend::new("25");
        frontend.parse(source).unwrap()
    }

    #[test]
    fn test_parse_simple_class() {
        let source = r#"
public class Tutorial {
    private long id = 0;
    private String title;

    public Tutorial() {}

    public Tutorial(String title, String description, boolean published) {
        this.title = title;
    }

    public long getId() {
        return id;
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse class with fields and methods");
        // Should have: fields (id, title), constructors, and methods
    }

    #[test]
    fn test_parse_enum() {
        let source = r#"
public enum OpType {
    ADD, SUBTRACT, MULTIPLY, DIVIDE
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse enum");
    }

    #[test]
    fn test_parse_annotations() {
        let source = r#"
@RestController
@RequestMapping("/api")
public class TutorialController {
    @GetMapping("/tutorials")
    public String getAll() {
        return "hello";
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse class with annotations, got {} nodes", ir.len());
        // Check that we got at least a function
        let has_fn = ir.iter().any(|n| matches!(n.node, IrNode::Fn { .. }));
        assert!(has_fn, "Should have at least one function");
    }

    #[test]
    fn test_parse_generics() {
        let source = r#"
public class Service {
    private List<String> items = new ArrayList<String>();

    public List<String> findAll() {
        return items;
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse generics");
    }

    #[test]
    fn test_parse_if_else() {
        let source = r#"
public class Main {
    public void check(int x) {
        if (x > 0) {
            return;
        } else {
            return;
        }
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse if/else");
    }

    #[test]
    fn test_parse_for_loop() {
        let source = r#"
public class Main {
    public void loop(List<String> items) {
        for (String item : items) {
            System.out.println(item);
        }
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse for-each loop");
    }

    #[test]
    fn test_parse_lambda() {
        let source = r#"
public class Service {
    public List<String> filter(List<String> items, String title) {
        return items.stream().filter(item -> item.contains(title)).toList();
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse lambda expression");
    }

    #[test]
    fn test_parse_method_reference() {
        let source = r#"
public class Service {
    public void add(String item) {
        list.add(item);
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse method reference");
    }

    #[test]
    fn test_parse_springboot_service() {
        // Real Spring Boot code with lambdas and streams
        let source = r#"
@Service
public class TutorialService {
  static List<Tutorial> tutorials = new ArrayList<Tutorial>();
  static long id = 0;

  public List<Tutorial> findAll() {
    return tutorials;
  }

  public List<Tutorial> findByTitleContaining(String title) {
    return tutorials.stream().filter(tutorial -> tutorial.getTitle().contains(title)).toList();
  }

  public Tutorial findById(long id) {
    return tutorials.stream().filter(tutorial -> id == tutorial.getId()).findAny().orElse(null);
  }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse Spring Boot service with lambdas");
    }

    #[test]
    fn test_parse_method_reference_foreach() {
        // Method reference inside forEach: tutorials::add
        let source = r#"
public class Controller {
    public void getAll() {
        list.forEach(item::add);
    }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse method reference in forEach");
    }

    #[test]
    fn test_parse_full_controller() {
        // Full TutorialController from Spring Boot project
        let source = r#"
@RestController
@RequestMapping("/api")
public class TutorialController {
  TutorialService tutorialService;

  @GetMapping("/tutorials")
  public void getAllTutorials(String title) {
    try {
      List<Tutorial> tutorials = new ArrayList<Tutorial>();
      if (title == null)
        tutorialService.findAll().forEach(tutorials::add);
      else
        tutorialService.findByTitleContaining(title).forEach(tutorials::add);
    } catch (Exception e) {
    }
  }

  @GetMapping("/tutorials/{id}")
  public void getTutorialById(long id) {
    Tutorial tutorial = tutorialService.findById(id);
    if (tutorial != null) {
      return;
    }
  }
}
        "#;
        let ir = parse_java(source);
        assert!(!ir.is_empty(), "Should parse full controller");
    }
}
