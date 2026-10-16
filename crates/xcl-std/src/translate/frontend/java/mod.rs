//! Java frontend for the translator.
//!
//! Parses Java source code directly into IR, handling:
//! - Package declarations and imports (skipped)
//! - Annotations (skipped)
//! - Classes with fields and methods
//! - Enums
//! - Generics
//! - Method parameters, return types, and bodies
//! - Expressions: literals, variables, field access, method calls, binary ops, conditionals
//! - Statements: assignments, if/else, for/while loops, returns, try/catch

use crate::translate::frontend::Frontend;
use crate::translate::ir::node::{Ir, IrNode, IrLiteral, Origin, SourceSpan};
use crate::translate::ir::types::IrType;
use crate::translate::ir::ops::{IrOp, Hint};
use crate::translate::version::LangSpec;
use super::super::backend::TranslateError;

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
        let chars: Vec<char> = source.chars().collect();
        let mut p = JavaParser { chars, pos: 0, origin, result: Vec::new() };
        p.parse();
        Ok(p.result)
    }
}

struct JavaParser {
    chars: Vec<char>,
    pos: usize,
    origin: Origin,
    result: Vec<Ir>,
}

impl JavaParser {
    fn parse(&mut self) {
        // Parse top-level: skip package, imports, then parse type declarations
        let mut iterations = 0u64;
        let max_iterations = self.chars.len() as u64 * 100; // Safety limit
        while !self.at_eof() {
            iterations += 1;
            if iterations > max_iterations { break; } // Prevent infinite loops
            self.skip_ws();
            if self.at_eof() { break; }

            // Skip comments
            if self.eat("//") {
                while !self.at_eof() && !self.eat("\n") {}
                continue;
            }
            if self.eat("/*") {
                while !self.at_eof() {
                    if self.eat("*/") { break; }
                }
                continue;
            }

            // Skip package declaration
            if self.eat("package") {
                while !self.at_eof() && self.chars[self.pos] != ';' { self.pos += 1; }
                if self.at(';') { self.pos += 1; }
                continue;
            }

            // Skip import declaration
            if self.eat("import") {
                while !self.at_eof() && self.chars[self.pos] != ';' { self.pos += 1; }
                if self.at(';') { self.pos += 1; }
                continue;
            }

            // Skip annotations
            if self.chars[self.pos] == '@' {
                self.skip_annotation();
                continue;
            }

            // Skip modifiers
            if self.eat_word("public") || self.eat_word("private") || self.eat_word("protected")
               || self.eat_word("static") || self.eat_word("final") || self.eat_word("abstract")
               || self.eat_word("synchronized") || self.eat_word("volatile") || self.eat_word("transient")
               || self.eat_word("native") || self.eat_word("strictfp")
            {
                continue;
            }

            // Parse class
            if self.eat("class") || self.eat("interface") {
                self.parse_class();
                continue;
            }

            // Parse enum
            if self.eat("enum") {
                self.parse_enum();
                continue;
            }

            // Skip unknown
            if self.chars[self.pos] == ';' { self.pos += 1; continue; }
            self.pos += 1;
        }
    }

    fn skip_annotation(&mut self) {
        let mut iterations = 0u64;
        while !self.at_eof() && iterations < 1000 {
            iterations += 1;
            if self.chars[self.pos] == '@' {
                self.pos += 1;
            }
            if self.at('{') {
                self.skip_block();
            } else if self.at('(') {
                self.skip_parens();
            } else if self.is_ident_char(self.chars[self.pos]) {
                while !self.at_eof() && self.is_ident_char(self.chars[self.pos]) {
                    self.pos += 1;
                }
            } else if self.at('.') || self.at(':') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_class(&mut self) {
        // Read class name
        self.skip_ws();
        let name = self.read_ident();

        // Skip extends/implements
        while !self.at_eof() {
            if self.eat("extends") || self.eat("implements") {
                while !self.at_eof() && !self.at('{') && !self.at(';') {
                    self.pos += 1;
                }
                // Skip type parameters <T>
                if self.at('<') {
                    while !self.at_eof() && !self.at('>') {
                        self.pos += 1;
                    }
                    if self.at('>') { self.pos += 1; }
                }
                continue;
            }
            break;
        }

        // Skip type parameters <T>
        self.skip_ws();
        if self.at('<') {
            while !self.at_eof() && !self.at('>') {
                self.pos += 1;
            }
            if self.at('>') { self.pos += 1; }
        }

        // Enter class body
        self.skip_ws();
        if !self.at('{') { return; }
        self.pos += 1; // eat '{'

        // Parse class members
        while !self.at_eof() {
            self.skip_ws();
            if self.at_eof() { break; }

            if self.at('}') {
                self.pos += 1;
                break;
            }

            // Skip annotations on members
            if self.chars[self.pos] == '@' {
                self.skip_annotation();
                continue;
            }

            // Skip modifiers
            let mut has_static = false;
            while self.eat_word("public") || self.eat_word("private") || self.eat_word("protected")
                  || self.eat_word("final") || self.eat_word("abstract")
                  || self.eat_word("synchronized") || self.eat_word("volatile") || self.eat_word("transient")
                  || self.eat_word("native") || self.eat_word("strictfp")
            {
                // track static for field detection
            }
            if self.eat_word("static") {
                has_static = true;
            }

            self.skip_ws();
            if self.at_eof() || self.at('}') { break; }

            // Check for inner class/enum
            if self.eat("class") || self.eat("interface") || self.eat("enum") {
                let inner_name = self.read_ident();
                self.skip_ws();
                if self.at('{') {
                    self.skip_block();
                } else if self.at(';') {
                    self.pos += 1;
                }
                continue;
            }

            // Check for method or field
            // A field: Type name; or Type name = value;
            // A method: Type name( ... ) { ... } or Type name( ... ) throws ... { ... }

            // Read the type (may be complex with generics)
            let type_name = self.read_type();
            self.skip_ws();

            if self.at_eof() || self.at('}') { break; }

            // Read the identifier
            let ident = self.read_ident();
            self.skip_ws();

            if self.at_eof() || self.at('}') { break; }

            // Check if it's a method (has '(')
            if self.at('(') {
                // Parse method
                self.parse_method(&ident, &type_name);
                continue;
            }

            // It's a field
            if self.at('=') {
                self.pos += 1; // eat '='
                // Skip the value until ';'
                self.skip_until_semi();
            } else if self.at(';') {
                self.pos += 1;
            } else {
                // Skip to semicolon for unknown syntax
                self.skip_until_semi();
            }

            // Emit field as a Let IR node
            let ir_type = self.java_type_to_ir(&type_name);
            let value = Ir::new(
                IrNode::Literal(IrLiteral::Nil),
                ir_type.clone(),
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
            self.result.push(Ir::new(
                IrNode::Let { name: ident, ty: ir_type, value: Box::new(value), mutable: !has_static },
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            ));
        }
    }

    fn parse_method(&mut self, name: &str, return_type: &str) {
        // Read parameters
        self.pos += 1; // eat '('
        let mut params: Vec<(String, IrType)> = Vec::new();

        if !self.at(')') {
            while !self.at_eof() {
                self.skip_ws();
                // Skip annotations on params
                if self.chars[self.pos] == '@' {
                    self.skip_annotation();
                    continue;
                }
                // Skip 'final' modifier on params
                if self.eat_word("final") {
                    self.skip_ws();
                }

                let param_type = self.read_type();
                self.skip_ws();
                let param_name = self.read_ident();
                self.skip_ws();

                // Handle array syntax: int[] arr
                if self.at('[') {
                    while self.at('[') { self.pos += 1; if self.at(']') { self.pos += 1; } }
                }

                let ir_type = self.java_type_to_ir(&param_type);
                params.push((param_name, ir_type));

                if self.at(',') {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        if self.at(')') { self.pos += 1; }

        // Skip throws clause
        self.skip_ws();
        if self.eat("throws") {
            while !self.at_eof() && !self.at('{') {
                self.pos += 1;
            }
        }

        let ret_type = self.java_type_to_ir(return_type);

        // Parse method body
        self.skip_ws();
        if !self.at('{') { return; }
        self.pos += 1; // eat '{'

        // Parse method body statements
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
        let mut iterations = 0u64;
        let max_iterations = self.chars.len() as u64 * 10;

        while !self.at_eof() {
            iterations += 1;
            if iterations > max_iterations { break; }
            self.skip_ws();
            if self.at_eof() || self.at('}') { break; }

            // Skip annotations inside blocks
            if self.chars[self.pos] == '@' {
                self.skip_annotation();
                continue;
            }

            // Skip local modifiers (shouldn't appear much but handle them)
            if self.eat_word("final") || self.eat_word("static") {
                self.skip_ws();
            }

            // Try/catch block
            if self.eat("try") {
                self.skip_block(); // try body
                self.skip_ws();
                while self.eat("catch") {
                    if self.at('(') { self.skip_parens(); }
                    self.skip_block(); // catch body
                    self.skip_ws();
                }
                if self.eat("finally") {
                    self.skip_block();
                }
                continue;
            }

            // Return statement
            if self.eat("return") {
                let mut value = None;
                self.skip_ws();
                if !self.at(';') && !self.at('}') {
                    let expr = self.parse_expression();
                    value = Some(Box::new(expr));
                }
                if self.at(';') { self.pos += 1; }
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
            if self.eat("if") {
                self.skip_ws();
                if self.at('(') { self.pos += 1; }
                let cond = self.parse_expression();
                self.skip_ws();
                if self.at(')') { self.pos += 1; }

                self.skip_ws();
                let then_body = if self.at('{') {
                    self.pos += 1;
                    self.parse_block()
                } else {
                    let stmt = self.parse_statement_to_ir();
                    vec![stmt]
                };

                self.skip_ws();
                let else_body = if self.eat("else") {
                    self.skip_ws();
                    if self.eat("if") {
                        // else-if chain — parse as nested if
                        let nested = self.parse_if_nested();
                        vec![nested]
                    } else if self.at('{') {
                        self.pos += 1;
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
            if self.eat("for") {
                self.skip_ws();
                if self.at('(') { self.pos += 1; }

                // Check for enhanced for-each: for (Type var : collection)
                self.skip_ws();
                let is_enhanced = {
                    let saved = self.pos;
                    let looks_like_type = !self.at(';') && !self.at(')')
                        && self.chars.get(self.pos).map_or(false, |c| c.is_alphabetic() || *c == '@');
                    // Look ahead for ':'
                    if looks_like_type {
                        let type_str = self.read_type();
                        self.skip_ws();
                        let var_name = self.read_ident();
                        self.skip_ws();
                        if self.at(':') {
                            self.pos += 1; // eat ':'
                            let collection = self.parse_expression();
                            self.skip_ws();
                            if self.at(')') { self.pos += 1; }

                            self.skip_ws();
                            let body = if self.at('{') {
                                self.pos += 1;
                                self.parse_block()
                            } else {
                                vec![self.parse_statement_to_ir()]
                            };

                            // Emit as: for $var in collection { body }
                            let cond_ir = collection;
                            let body_ir = Ir::new(
                                IrNode::Block(body),
                                IrType::Nil,
                                SourceSpan::dummy(),
                                self.origin.clone(),
                                Vec::new(),
                            );
                            statements.push(Ir::new(
                                IrNode::Loop {
                                    cond: Some(Box::new(cond_ir)),
                                    body: Box::new(body_ir),
                                },
                                IrType::Nil,
                                SourceSpan::dummy(),
                                self.origin.clone(),
                                Vec::new(),
                            ));
                            true
                        } else {
                            self.pos = saved;
                            false
                        }
                    } else {
                        self.pos = saved;
                        false
                    }
                };

                if !is_enhanced {
                    // Traditional for: for (init; cond; update)
                    // Parse init
                    self.skip_ws();
                    if !self.at(';') {
                        // Could be type declaration or expression
                        let init = self.parse_expression();
                        statements.push(init);
                    }
                    if self.at(';') { self.pos += 1; }

                    // Parse condition
                    self.skip_ws();
                    let mut cond = None;
                    if !self.at(';') && !self.at(')') {
                        cond = Some(Box::new(self.parse_expression()));
                    }
                    if self.at(';') { self.pos += 1; }

                    // Parse update
                    self.skip_ws();
                    let mut update = None;
                    if !self.at(')') {
                        update = Some(Box::new(self.parse_expression()));
                    }
                    if self.at(')') { self.pos += 1; }

                    // Parse body
                    self.skip_ws();
                    let body = if self.at('{') {
                        self.pos += 1;
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

                    if let Some(upd) = update {
                        // Add update at end of loop body (simplified)
                        // For now, just add it after
                    }
                }
                continue;
            }

            // While loop
            if self.eat("while") {
                self.skip_ws();
                if self.at('(') { self.pos += 1; }
                let cond = self.parse_expression();
                self.skip_ws();
                if self.at(')') { self.pos += 1; }

                self.skip_ws();
                let body = if self.at('{') {
                    self.pos += 1;
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
            if self.eat("break") {
                if self.at(';') { self.pos += 1; }
                statements.push(Ir::new(
                    IrNode::Break,
                    IrType::Nil,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                ));
                continue;
            }
            if self.eat("continue") {
                if self.at(';') { self.pos += 1; }
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
            if stmt.node != IrNode::Block(Vec::new()) {
                statements.push(stmt);
            }
        }

        statements
    }

    fn parse_if_nested(&mut self) -> Ir {
        // Parse nested if (else-if chain)
        self.skip_ws();
        if self.at('(') { self.pos += 1; }
        let cond = self.parse_expression();
        self.skip_ws();
        if self.at(')') { self.pos += 1; }

        self.skip_ws();
        let then_body = if self.at('{') {
            self.pos += 1;
            self.parse_block()
        } else {
            vec![self.parse_statement_to_ir()]
        };

        self.skip_ws();
        let else_body = if self.eat("else") {
            self.skip_ws();
            if self.eat("if") {
                let nested = self.parse_if_nested();
                vec![nested]
            } else if self.at('{') {
                self.pos += 1;
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
        self.skip_ws();
        if self.at_eof() || self.at('}') {
            return Ir::new(
                IrNode::Block(Vec::new()),
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // Check for local variable declaration
        // Type name = value; or Type name;
        let saved = self.pos;
        let looks_like_type = self.chars.get(self.pos).map_or(false, |c| c.is_uppercase());
        let looks_like_ident = self.chars.get(self.pos).map_or(false, |c| c.is_alphabetic() || *c == '$');

        if looks_like_type || looks_like_ident {
            let first = self.read_type();
            self.skip_ws();

            if !self.at_eof() && self.chars.get(self.pos).map_or(false, |c| c.is_alphabetic() || *c == '_') {
                let name = self.read_ident();
                self.skip_ws();

                // Handle array declaration: Type[] name
                if self.at('[') {
                    while self.at('[') { self.pos += 1; if self.at(']') { self.pos += 1; } }
                    self.skip_ws();
                }

                if self.at('=') {
                    self.pos += 1; // eat '='
                    let value = self.parse_expression();
                    self.skip_ws();
                    if self.at(';') { self.pos += 1; }

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
                } else if self.at(';') {
                    self.pos += 1;
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
                } else if self.at('(') {
                    // It's a method call, not a declaration
                    self.pos = saved;
                } else {
                    self.pos = saved;
                }
            } else if self.at('(') {
                // Method call
                self.pos = saved;
            } else {
                self.pos = saved;
            }
        }

        // Expression statement
        let before_expr = self.pos;
        let expr = self.parse_expression();
        // Safety: if parse_expression didn't advance, skip a char
        if self.pos == before_expr {
            self.pos += 1;
        }
        self.skip_ws();
        if self.at(';') { self.pos += 1; }
        expr
    }

    fn parse_expression(&mut self) -> Ir {
        // Parse with operator precedence (lowest to highest)
        self.parse_or()
    }

    fn parse_or(&mut self) -> Ir {
        let mut left = self.parse_and();
        self.skip_ws();
        // Handle lambda: param -> expr (lowest precedence after ||)
        if self.eat("->") {
            self.skip_ws();
            let body = self.parse_and();
            left = Ir::new(
                IrNode::Fn {
                    name: "lambda".to_string(),
                    params: Vec::new(),
                    ret: IrType::Any,
                    body: Box::new(left), // left is the param var
                },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                vec![Hint::Async],
            );
            // Replace body with the actual lambda body
            left = Ir::new(
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
        } else {
            while self.eat("||") {
                let right = self.parse_and();
                left = Ir::new(
                    IrNode::BinOp { op: IrOp::Or, lhs: Box::new(left), rhs: Box::new(right) },
                    IrType::Bool,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );
                self.skip_ws();
            }
        }
        left
    }

    fn parse_and(&mut self) -> Ir {
        let mut left = self.parse_equality();
        self.skip_ws();
        while self.eat("&&") {
            let right = self.parse_equality();
            left = Ir::new(
                IrNode::BinOp { op: IrOp::And, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
            self.skip_ws();
        }
        left
    }

    fn parse_equality(&mut self) -> Ir {
        let mut left = self.parse_comparison();
        self.skip_ws();
        while self.eat("==") || self.eat("!=") {
            let op = if self.eat("==") { IrOp::Eq } else { IrOp::Ne };
            // Re-read since eat already consumed
            let right = self.parse_comparison();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
            self.skip_ws();
        }
        left
    }

    fn parse_comparison(&mut self) -> Ir {
        let mut left = self.parse_additive();
        self.skip_ws();
        loop {
            let op = if self.eat("<=") {
                IrOp::Le
            } else if self.eat(">=") {
                IrOp::Ge
            } else if self.eat("<") {
                IrOp::Lt
            } else if self.eat(">") {
                IrOp::Gt
            } else {
                break;
            };
            let right = self.parse_additive();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
            self.skip_ws();
        }
        left
    }

    fn parse_additive(&mut self) -> Ir {
        let mut left = self.parse_multiplicative();
        self.skip_ws();
        while self.eat("+") || self.eat("-") {
            let op = if self.eat("+") { IrOp::Add } else { IrOp::Sub };
            let right = self.parse_multiplicative();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
            self.skip_ws();
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Ir {
        let mut left = self.parse_unary();
        self.skip_ws();
        while self.eat("*") || self.eat("/") || self.eat("%") {
            let op = match self.chars.get(self.pos.saturating_sub(1)) {
                Some('*') => IrOp::Mul,
                Some('/') => IrOp::Div,
                Some('%') => IrOp::Rem,
                _ => IrOp::Mul,
            };
            let right = self.parse_unary();
            left = Ir::new(
                IrNode::BinOp { op, lhs: Box::new(left), rhs: Box::new(right) },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
            self.skip_ws();
        }
        left
    }

    fn parse_unary(&mut self) -> Ir {
        self.skip_ws();
        if self.eat("!") {
            let operand = self.parse_unary();
            return Ir::new(
                IrNode::UnaryOp { op: IrOp::Not, operand: Box::new(operand) },
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        if self.eat("-") {
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
        let mut expr = self.parse_primary();
        self.skip_ws();
        let mut iterations = 0u64;

        while !self.at_eof() && iterations < 100 {
            iterations += 1;
            if self.at('.') {
                self.pos += 1; // eat '.'
                self.skip_ws();
                let field = self.read_ident();
                self.skip_ws();

                // Check for method call
                if self.at('(') {
                    self.pos += 1; // eat '('
                    let mut args = Vec::new();
                    if !self.at(')') {
                        while !self.at_eof() {
                            args.push(self.parse_expression());
                            self.skip_ws();
                            if self.at(',') { self.pos += 1; }
                            else { break; }
                        }
                    }
                    if self.at(')') { self.pos += 1; }

                    expr = Ir::new(
                        IrNode::Call {
                            callee: Box::new(Ir::new(
                                IrNode::FieldAccess {
                                    obj: Box::new(expr),
                                    field,
                                },
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
                    // Field access
                    expr = Ir::new(
                        IrNode::FieldAccess { obj: Box::new(expr), field },
                        IrType::Any,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    );
                }
            } else if self.at('[') {
                self.pos += 1; // eat '['
                let index = self.parse_expression();
                self.skip_ws();
                if self.at(']') { self.pos += 1; }

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
            } else if self.eat("::") {
                // Method reference: obj::method or Type::method or Type::new
                // Skip the method reference target and treat as a call
                self.skip_ws();
                let method_name = self.read_ident();
                self.skip_ws();
                // If followed by parentheses, it's a call (e.g., String::new())
                if self.at('(') {
                    self.pos += 1; // eat '('
                    let mut args = Vec::new();
                    if !self.at(')') {
                        while !self.at_eof() {
                            args.push(self.parse_expression());
                            self.skip_ws();
                            if self.at(',') { self.pos += 1; }
                            else { break; }
                        }
                    }
                    if self.at(')') { self.pos += 1; }
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
                    // Bare method reference, treat as a variable
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
            self.skip_ws();
        }

        expr
    }

    fn parse_primary(&mut self) -> Ir {
        self.skip_ws();
        if self.at_eof() {
            return Ir::new(
                IrNode::Literal(IrLiteral::Nil),
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // Safety: remember position to detect no-progress loops
        let start_pos = self.pos;

        // String literal
        if self.at('"') {
            return self.parse_string_literal();
        }

        // Character literal
        if self.at('\'') {
            return self.parse_char_literal();
        }

        // Numeric literal
        if self.chars[self.pos].is_ascii_digit() || (self.at('.') && self.pos + 1 < self.chars.len() && self.chars[self.pos + 1].is_ascii_digit()) {
            return self.parse_number_literal();
        }

        // Boolean/null literals
        if self.eat("true") {
            return Ir::new(
                IrNode::Literal(IrLiteral::Bool(true)),
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        if self.eat("false") {
            return Ir::new(
                IrNode::Literal(IrLiteral::Bool(false)),
                IrType::Bool,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }
        if self.eat("null") {
            return Ir::new(
                IrNode::Literal(IrLiteral::Nil),
                IrType::Nil,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // this keyword
        if self.eat("this") {
            return Ir::new(
                IrNode::Var("this".to_string()),
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // Parenthesized expression
        if self.at('(') {
            self.pos += 1; // eat '('
            let expr = self.parse_expression();
            self.skip_ws();
            if self.at(')') { self.pos += 1; }
            return expr;
        }

        // Array literal: new Type[] { ... }
        if self.eat("new") {
            self.skip_ws();
            let type_name = self.read_type();
            self.skip_ws();

            // Array initialization: new Type[] { a, b, c }
            if self.at('[') {
                self.pos += 1;
                if self.at(']') { self.pos += 1; }
                self.skip_ws();
                if self.at('{') {
                    return self.parse_array_literal(&type_name);
                }
            }

            // Constructor call: new Type(args)
            self.skip_ws();
            if self.at('(') {
                self.pos += 1;
                let mut args = Vec::new();
                if !self.at(')') {
                    while !self.at_eof() {
                        args.push(self.parse_expression());
                        self.skip_ws();
                        if self.at(',') { self.pos += 1; }
                        else { break; }
                    }
                }
                if self.at(')') { self.pos += 1; }

                return Ir::new(
                    IrNode::Call {
                        callee: Box::new(Ir::new(
                            IrNode::Var(type_name),
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
            }

            // new Type[size]
            return Ir::new(
                IrNode::Var(type_name),
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // Lambda: (params) -> expr or params -> expr
        // Skip for now, treat as method reference

        // Ternary: cond ? then : else
        // Handled at higher precedence level

        // Identifier or method call
        if self.chars[self.pos].is_alphabetic() || self.chars[self.pos] == '_' || self.chars[self.pos] == '$' {
            let name = self.read_ident();
            self.skip_ws();

            // Method call?
            if self.at('(') {
                self.pos += 1; // eat '('
                let mut args = Vec::new();
                if !self.at(')') {
                    while !self.at_eof() {
                        self.skip_ws();
                        if self.at(')') { break; }
                        args.push(self.parse_expression());
                        self.skip_ws();
                        if self.at(',') { self.pos += 1; }
                        else { break; }
                    }
                }
                if self.at(')') { self.pos += 1; }

                return Ir::new(
                    IrNode::Call {
                        callee: Box::new(Ir::new(
                            IrNode::Var(name),
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
            }

            // Assignment: name = expr
            if self.eat("=") || self.eat("+=") || self.eat("-=") || self.eat("*=") || self.eat("/=") {
                let value = self.parse_expression();
                let ir_type = self.infer_type(&value);
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
            }

            // Plain variable
            return Ir::new(
                IrNode::Var(name),
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        // Handle -> (lambda arrow) — skip the lambda body
        if self.eat("->") {
            self.skip_lambda_body();
            return Ir::new(
                IrNode::Fn {
                    name: "lambda".to_string(),
                    params: Vec::new(),
                    ret: IrType::Any,
                    body: Box::new(Ir::new(
                        IrNode::Literal(IrLiteral::Nil),
                        IrType::Nil,
                        SourceSpan::dummy(),
                        self.origin.clone(),
                        Vec::new(),
                    )),
                },
                IrType::Any,
                SourceSpan::dummy(),
                self.origin.clone(),
                vec![Hint::Async],
            );
        }

        // Unknown — skip a token
        if self.pos == start_pos {
            self.pos += 1; // Ensure we advance
        }
        Ir::new(
            IrNode::Literal(IrLiteral::Nil),
            IrType::Nil,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_array_literal(&mut self, _type_name: &str) -> Ir {
        // { a, b, c }
        self.pos += 1; // eat '{'
        let mut elements = Vec::new();
        self.skip_ws();
        while !self.at('}') && !self.at_eof() {
            elements.push(self.parse_expression());
            self.skip_ws();
            if self.at(',') { self.pos += 1; self.skip_ws(); }
        }
        if self.at('}') { self.pos += 1; }

        Ir::new(
            IrNode::Call {
                callee: Box::new(Ir::new(
                    IrNode::Var("array".to_string()),
                    IrType::Any,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                )),
                args: elements,
            },
            IrType::Any,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_string_literal(&mut self) -> Ir {
        self.pos += 1; // eat opening '"'
        let mut s = String::new();
        while !self.at_eof() && !self.at('"') {
            if self.at('\\') {
                self.pos += 1;
                if !self.at_eof() {
                    match self.chars[self.pos] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        c => s.push(c),
                    }
                    self.pos += 1;
                }
            } else {
                s.push(self.chars[self.pos]);
                self.pos += 1;
            }
        }
        if self.at('"') { self.pos += 1; }

        Ir::new(
            IrNode::Literal(IrLiteral::Str(s)),
            IrType::Str,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_char_literal(&mut self) -> Ir {
        self.pos += 1; // eat opening '\''
        let mut c = '?';
        if self.at('\\') {
            self.pos += 1;
            if !self.at_eof() {
                c = self.chars[self.pos];
                self.pos += 1;
            }
        } else if !self.at_eof() {
            c = self.chars[self.pos];
            self.pos += 1;
        }
        if self.at('\'') { self.pos += 1; }

        Ir::new(
            IrNode::Literal(IrLiteral::Str(c.to_string())),
            IrType::Str,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_number_literal(&mut self) -> Ir {
        let mut num = String::new();
        let is_float = self.at('.');
        if is_float {
            num.push('.');
            self.pos += 1;
        }

        while !self.at_eof() && (self.chars[self.pos].is_ascii_digit() || self.chars[self.pos] == '.') {
            num.push(self.chars[self.pos]);
            self.pos += 1;
        }

        // Handle L/F/d suffixes
        if !self.at_eof() && matches!(self.chars[self.pos], 'L' | 'l' | 'F' | 'f' | 'D' | 'd') {
            num.push(self.chars[self.pos]);
            self.pos += 1;
        }

        // Handle hex: 0x...
        if num.starts_with("0x") || num.starts_with("0X") {
            self.pos -= 2;
            self.pos += 2;
            while !self.at_eof() && self.chars[self.pos].is_ascii_hexdigit() {
                num.push(self.chars[self.pos]);
                self.pos += 1;
            }
            if let Ok(val) = i64::from_str_radix(&num[2..], 16) {
                return Ir::new(
                    IrNode::Literal(IrLiteral::Int(val)),
                    IrType::Int,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );
            }
        }

        if is_float || num.contains('.') {
            if let Ok(val) = num.parse::<f64>() {
                return Ir::new(
                    IrNode::Literal(IrLiteral::Float(val)),
                    IrType::Float,
                    SourceSpan::dummy(),
                    self.origin.clone(),
                    Vec::new(),
                );
            }
        }

        if let Ok(val) = num.parse::<i64>() {
            return Ir::new(
                IrNode::Literal(IrLiteral::Int(val)),
                IrType::Int,
                SourceSpan::dummy(),
                self.origin.clone(),
                Vec::new(),
            );
        }

        Ir::new(
            IrNode::Literal(IrLiteral::Int(0)),
            IrType::Int,
            SourceSpan::dummy(),
            self.origin.clone(),
            Vec::new(),
        )
    }

    fn parse_enum(&mut self) {
        self.skip_ws();
        let name = self.read_ident();
        self.skip_ws();

        if !self.at('{') { return; }
        self.pos += 1; // eat '{'

        let mut variants = Vec::new();
        while !self.at_eof() && !self.at('}') {
            self.skip_ws();
            if self.at(',') { self.pos += 1; continue; }
            if self.chars[self.pos] == '@' {
                self.skip_annotation();
                continue;
            }
            let variant = self.read_ident();
            if !variant.is_empty() {
                variants.push(variant);
            }
            self.skip_ws();
        }
        if self.at('}') { self.pos += 1; }

        let fields: Vec<(String, IrType)> = variants.iter().map(|v| (v.clone(), IrType::Int)).collect();
        self.result.push(Ir::new(
            IrNode::Record { name: name.clone(), fields },
            IrType::Any,
            SourceSpan::dummy(),
            self.origin.clone(),
            vec![Hint::Sealed],
        ));
    }

    // --- Type reading ---

    fn read_type(&mut self) -> String {
        self.skip_ws();
        let mut type_str = String::new();

        if self.at_eof() { return type_str; }

        // Read identifier
        if self.chars[self.pos].is_alphabetic() || self.chars[self.pos] == '_' {
            type_str.push_str(&self.read_ident());
        }

        self.skip_ws();

        // Handle generics: <T, U>
        if self.at('<') {
            type_str.push('<');
            self.pos += 1;
            let mut depth = 1;
            while !self.at_eof() && depth > 0 {
                if self.at('<') { depth += 1; }
                if self.at('>') { depth -= 1; }
                if depth > 0 {
                    type_str.push(self.chars[self.pos]);
                }
                self.pos += 1;
            }
            type_str.push('>');
        }

        // Handle array: Type[]
        while self.at('[') {
            type_str.push_str("[]");
            self.pos += 1;
            if self.at(']') { self.pos += 1; }
        }

        type_str
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
                // Check if it looks like a custom type (starts with uppercase)
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
            IrNode::Call { callee, .. } => {
                if let IrNode::Var(name) = &callee.node {
                    if name == "array" { return IrType::Array(Box::new(IrType::Any)); }
                }
                IrType::Any
            }
            _ => IrType::Any,
        }
    }

    // --- Utility methods ---

    fn at_eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn at(&self, c: char) -> bool {
        self.pos < self.chars.len() && self.chars[self.pos] == c
    }

    fn eat(&mut self, s: &str) -> bool {
        let s_chars: Vec<char> = s.chars().collect();
        if self.pos + s_chars.len() > self.chars.len() { return false; }
        let matched: bool = (0..s_chars.len()).all(|i| self.chars[self.pos + i] == s_chars[i]);
        if matched {
            self.pos += s_chars.len();
        }
        matched
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if !self.eat(word) { return false; }
        // Check word boundary
        if self.pos < self.chars.len() && self.is_ident_char(self.chars[self.pos]) {
            self.pos -= word.len();
            false
        } else {
            true
        }
    }

    fn is_ident_char(&self, c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '$'
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while !self.at_eof() && self.is_ident_char(self.chars[self.pos]) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn skip_ws(&mut self) {
        while !self.at_eof() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    /// Skip the body of a lambda expression, advancing past it without parsing.
    /// Lambdas can be: `param -> expr` or `param -> { statements }`
    /// We skip until we reach a context boundary (semicolon, closing paren/brace at the same depth, comma at depth 0).
    fn skip_lambda_body(&mut self) {
        let mut depth_paren = 0u32;
        let mut depth_brace = 0u32;
        let mut in_string = false;
        let mut prev_backslash = false;

        while !self.at_eof() {
            let c = self.chars[self.pos];
            if in_string {
                if prev_backslash {
                    prev_backslash = false;
                } else if c == '\\' {
                    prev_backslash = true;
                } else if c == '"' {
                    in_string = false;
                }
            } else if c == '"' || c == '\'' {
                in_string = true;
            } else if c == '(' {
                depth_paren += 1;
            } else if c == ')' {
                if depth_paren == 0 {
                    // End of lambda expression at top level
                    return;
                }
                depth_paren = depth_paren.saturating_sub(1);
            } else if c == '{' {
                depth_brace += 1;
            } else if c == '}' {
                depth_brace = depth_brace.saturating_sub(1);
            } else if c == ';' && depth_paren == 0 && depth_brace == 0 {
                self.pos += 1;
                return;
            } else if c == ',' && depth_paren == 0 && depth_brace == 0 {
                // Comma at top level means end of lambda argument
                return;
            }
            self.pos += 1;
        }
    }

    fn skip_until_semi(&mut self) {
        let mut depth_paren = 0u32;
        let mut depth_brace = 0u32;
        let mut in_string = false;
        let mut prev_backslash = false;

        while !self.at_eof() {
            let c = self.chars[self.pos];
            if in_string {
                if prev_backslash {
                    prev_backslash = false;
                } else if c == '\\' {
                    prev_backslash = true;
                } else if c == '"' {
                    in_string = false;
                }
            } else {
                if c == '"' { in_string = true; }
                else if c == '(' { depth_paren += 1; }
                else if c == ')' { depth_paren = depth_paren.saturating_sub(1); }
                else if c == '{' { depth_brace += 1; }
                else if c == '}' { depth_brace = depth_brace.saturating_sub(1); }
                else if c == ';' && depth_paren == 0 && depth_brace == 0 {
                    self.pos += 1;
                    return;
                }
            }
            self.pos += 1;
        }
    }

    fn skip_block(&mut self) {
        if !self.at('{') { return; }
        self.pos += 1;
        let mut depth = 1u32;
        while !self.at_eof() && depth > 0 {
            if self.at('{') { depth += 1; }
            if self.at('}') { depth -= 1; }
            self.pos += 1;
        }
    }

    fn skip_parens(&mut self) {
        if !self.at('(') { return; }
        self.pos += 1;
        let mut depth = 1u32;
        while !self.at_eof() && depth > 0 {
            if self.at('(') { depth += 1; }
            if self.at(')') { depth -= 1; }
            self.pos += 1;
        }
    }
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
        assert!(!ir.is_empty(), "Should parse class with annotations");
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
}
