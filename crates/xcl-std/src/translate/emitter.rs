//! Generic IR emitter framework.
//!
//! This module provides a single `emit_node` function that all backends share.
//! Each backend configures the emitter through the `EmitConfig` trait, reducing
//! each backend from ~350-420 lines to ~50-100 lines of configuration.
//!
//! The emitter handles all 22 `IrNode` variants generically, delegating only
//! the language-specific parts (literals, type names, identifier conversion,
//! block syntax, special emit hooks) to the `EmitConfig` trait.

use crate::translate::backend::TranslateError;
use crate::translate::ir::node::{Ir, IrNode, IrLiteral};
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;

/// Context passed through the emitter for collecting imports, typedefs, etc.
#[derive(Default)]
pub struct EmitContext {
    /// Import statements collected during emit
    pub imports: Vec<String>,
    /// Type definitions collected during emit (e.g., C structs, Java records)
    pub typedefs: Vec<String>,
}

/// Configuration trait for language-specific emit behavior.
///
/// Each backend implements this trait once, and the generic `emit_node` function
/// handles all IR node traversal. Only the language-specific formatting is delegated
/// to these trait methods.
pub trait EmitConfig {
    /// Return the language specification for this backend.
    fn lang_spec() -> LangSpec;

    // ─── Literals ───

    /// Boolean true literal (e.g., "true", "True", "1")
    fn bool_true() -> &'static str;

    /// Boolean false literal (e.g., "false", "False", "0")
    fn bool_false() -> &'static str;

    /// Nil/null literal (e.g., "null", "None", "NULL", "nullptr")
    fn nil_literal() -> &'static str;

    /// Integer suffix (e.g., "i64" for Rust, "" for most languages)
    fn int_suffix() -> &'static str;

    /// Float suffix for whole numbers (e.g., ".0" for C/Java, "" for Python)
    fn float_suffix() -> &'static str;

    /// Quote character for strings (e.g., '"' or '\'')
    fn quote_char() -> char;

    // ─── Syntax ───

    /// Variable declaration keyword (e.g., "var", "auto", "Object", "")
    fn var_keyword() -> &'static str;

    /// Mutable variable prefix (e.g., "mut " for Rust, "" for most)
    fn mut_prefix() -> &'static str;

    /// Immutable variable prefix (e.g., "const " for C, "" for most)
    fn const_prefix() -> &'static str;

    /// Semicolon at end of statements (e.g., ";" or "" for Python)
    fn semicolon() -> &'static str;

    /// Block open delimiter (e.g., "{" or "" for Python)
    fn block_open() -> &'static str;

    /// Block close delimiter (e.g., "}" or "" for Python)
    fn block_close() -> &'static str;

    /// Whether this language uses indentation-based blocks (Python)
    fn is_indent_based() -> bool {
        false
    }

    /// Whether to emit newlines after statements
    fn emit_newline_after_statement() -> bool {
        true
    }

    // ─── Type Mapping ───

    /// Map an IR type to the target language type name.
    fn map_type(ty: &IrType) -> String;

    /// Get the type name for a variable declaration.
    fn map_type_for_let(ty: &IrType) -> String {
        Self::map_type(ty)
    }

    /// Get the type name for a function return type.
    fn map_type_for_ret(ty: &IrType) -> String {
        Self::map_type(ty)
    }

    /// Get the type name for a function parameter.
    fn map_type_for_param(ty: &IrType) -> String {
        Self::map_type(ty)
    }

    // ─── Identifier Conversion ───

    /// Convert an identifier to the target language's naming convention.
    fn convert_identifier(name: &str) -> String;

    /// Convert a type/record name to the target language's naming convention.
    fn convert_type_name(name: &str) -> String;

    // ─── Special Emit Hooks ───

    /// Emit a record/struct definition.
    fn emit_record(
        name: &str,
        fields: &[(String, IrType)],
        output: &mut String,
        typedefs: &mut Vec<String>,
        indent_level: usize,
    );

    /// Emit a record literal (constructor call).
    fn emit_record_lit(
        name: &str,
        fields: &[(String, Ir)],
        output: &mut String,
        context: &mut EmitContext,
    ) -> Result<(), TranslateError>;

    /// Emit a function definition.
    fn emit_function(
        name: &str,
        params: &[(String, IrType)],
        ret: &IrType,
        body: &Ir,
        output: &mut String,
        context: &mut EmitContext,
        indent_level: usize,
    ) -> Result<(), TranslateError>;

    /// Emit a defer block.
    fn emit_defer(
        body: &Ir,
        output: &mut String,
        context: &mut EmitContext,
        indent_level: usize,
    ) -> Result<(), TranslateError>;

    /// Emit a lambda/closure.
    fn emit_lambda(
        params: &[(String, IrType)],
        body: &Ir,
        output: &mut String,
        context: &mut EmitContext,
    ) -> Result<(), TranslateError>;

    /// Emit a match/switch expression.
    fn emit_match(
        value: &Ir,
        arms: &[(String, Vec<String>, Box<Ir>)],
        output: &mut String,
        context: &mut EmitContext,
        indent_level: usize,
    ) -> Result<(), TranslateError>;

    /// Emit a slice operation.
    fn emit_slice(
        collection: &Ir,
        start: &Ir,
        end: &Ir,
        output: &mut String,
        context: &mut EmitContext,
    ) -> Result<(), TranslateError>;

    /// Emit binary operator string (allows language-specific overrides like Python's "and"/"or")
    fn bin_op_str(op: &crate::translate::ir::ops::IrOp) -> String {
        op.to_string()
    }

    /// Emit unary operator string (allows language-specific overrides like Python's "not")
    fn unary_op_str(op: &crate::translate::ir::ops::IrOp) -> &'static str {
        match op {
            crate::translate::ir::ops::IrOp::Not => "!",
            crate::translate::ir::ops::IrOp::Neg => "-",
            _ => "",
        }
    }

    // ─── Wrapper ───

    /// Emit the file-level wrapper start (e.g., "public class Main {", "int main(void) {")
    fn emit_wrapper_start(output: &mut String);

    /// Emit the file-level wrapper end (e.g., "}", "    return 0;\n}")
    fn emit_wrapper_end(output: &mut String);

    /// Emit imports at the top of the file
    fn emit_imports(imports: &[String], output: &mut String);

    /// Emit typedefs at the top of the file
    fn emit_typedefs(typedefs: &[String], output: &mut String);

    /// Apply language-specific idioms to the final output
    fn apply_idioms(code: &str) -> String {
        code.to_string()
    }
}

/// Emit a literal value using the EmitConfig's literal settings.
pub fn emit_literal<C: EmitConfig>(lit: &IrLiteral) -> String {
    crate::translate::utils::emit_literal(
        lit,
        C::bool_true(),
        C::bool_false(),
        C::nil_literal(),
        C::int_suffix(),
        C::float_suffix(),
        C::quote_char(),
    )
}

/// Emit an IR node to a String (helper for collecting sub-expressions).
pub fn emit_to_string<C: EmitConfig>(
    ir: &Ir,
    context: &mut EmitContext,
    indent_level: usize,
) -> Result<String, TranslateError> {
    let mut s = String::new();
    emit_node::<C>(ir, &mut s, context, indent_level)?;
    Ok(s)
}

/// Generic emit_node — shared by all backends.
///
/// This function handles all 22 `IrNode` variants. Language-specific behavior
/// is delegated to the `EmitConfig` trait methods.
pub fn emit_node<C: EmitConfig>(
    ir: &Ir,
    output: &mut String,
    context: &mut EmitContext,
    indent_level: usize,
) -> Result<(), TranslateError> {
    let indent = "    ".repeat(indent_level);

    match &ir.node {
        IrNode::Literal(lit) => {
            output.push_str(&emit_literal::<C>(lit));
        }

        IrNode::Var(name) => {
            output.push_str(&C::convert_identifier(name));
        }

        IrNode::Call { callee, args } => {
            emit_node::<C>(callee, output, context, 0)?;
            output.push('(');
            let args_str: Vec<String> = args
                .iter()
                .map(|arg| emit_to_string::<C>(arg, context, 0))
                .collect::<Result<Vec<_>, _>>()?;
            output.push_str(&args_str.join(", "));
            output.push(')');
        }

        IrNode::BinOp { op, lhs, rhs } => {
            emit_node::<C>(lhs, output, context, 0)?;
            output.push_str(&format!(" {} ", C::bin_op_str(op)));
            emit_node::<C>(rhs, output, context, 0)?;
        }

        IrNode::UnaryOp { op, operand } => {
            output.push_str(C::unary_op_str(op));
            emit_node::<C>(operand, output, context, 0)?;
        }

        IrNode::If { cond, then, else_ } => {
            let cond_str = emit_to_string::<C>(cond, context, 0)?;
            let then_str = emit_to_string::<C>(then, context, indent_level + 1)?;

            if C::is_indent_based() {
                output.push_str(&format!("{}if {}:\n{}", indent, cond_str, then_str));
            } else {
                output.push_str(&format!(
                    "{}if ({}) {}\n{}\n{}{}\n",
                    indent,
                    cond_str,
                    C::block_open(),
                    then_str,
                    indent,
                    C::block_close()
                ));
            }

            if let Some(else_ir) = else_ {
                let else_str = emit_to_string::<C>(else_ir, context, indent_level + 1)?;
                if C::is_indent_based() {
                    output.push_str(&format!("{}else:\n{}", indent, else_str));
                } else {
                    output.push_str(&format!(
                        "{}else {}\n{}\n{}{}\n",
                        indent,
                        C::block_open(),
                        else_str,
                        indent,
                        C::block_close()
                    ));
                }
            }
        }

        IrNode::Loop { body, cond } => {
            if let Some(cond_ir) = cond {
                let cond_str = emit_to_string::<C>(cond_ir, context, 0)?;
                let body_str = emit_to_string::<C>(body, context, indent_level + 1)?;
                if C::is_indent_based() {
                    output.push_str(&format!(
                        "{}while {}:\n{}\n",
                        indent, cond_str, body_str
                    ));
                } else {
                    output.push_str(&format!(
                        "{}while ({}) {}\n{}\n{}{}\n",
                        indent,
                        cond_str,
                        C::block_open(),
                        body_str,
                        indent,
                        C::block_close()
                    ));
                }
            } else {
                let body_str = emit_to_string::<C>(body, context, indent_level + 1)?;
                let true_literal = C::bool_true();
                if C::is_indent_based() {
                    output.push_str(&format!("{}while True:\n{}\n", indent, body_str));
                } else {
                    output.push_str(&format!(
                        "{}while ({}) {}\n{}\n{}{}\n",
                        indent,
                        true_literal,
                        C::block_open(),
                        body_str,
                        indent,
                        C::block_close()
                    ));
                }
            }
        }

        IrNode::Block(nodes) => {
            for node in nodes {
                emit_node::<C>(node, output, context, indent_level)?;
                if C::emit_newline_after_statement() {
                    output.push('\n');
                }
            }
        }

        IrNode::Return(value) => {
            if let Some(val) = value {
                let val_str = emit_to_string::<C>(val, context, 0)?;
                output.push_str(&format!(
                    "{}return {}{}\n",
                    indent,
                    val_str,
                    C::semicolon()
                ));
            } else {
                output.push_str(&format!("{}return{}\n", indent, C::semicolon()));
            }
        }

        IrNode::Let { name, ty, value, mutable } => {
            let value_str = emit_to_string::<C>(value, context, 0)?;
            let type_name = C::map_type_for_let(ty);
            let var_name = C::convert_identifier(name);

            let (mut_prefix, const_prefix) = if *mutable {
                (C::mut_prefix(), "")
            } else {
                ("", C::const_prefix())
            };

            if C::is_indent_based() {
                output.push_str(&format!(
                    "{}{} = {}\n",
                    indent, var_name, value_str
                ));
            } else {
                output.push_str(&format!(
                    "{}{}{}{} {} = {}{}\n",
                    indent,
                    const_prefix,
                    mut_prefix,
                    type_name,
                    var_name,
                    value_str,
                    C::semicolon()
                ));
            }
        }

        IrNode::Record { name, fields } => {
            C::emit_record(name, fields, output, &mut context.typedefs, indent_level);
        }

        IrNode::RecordLit { name, fields } => {
            C::emit_record_lit(name, fields, output, context)?;
        }

        IrNode::Fn { name, params, ret, body } => {
            C::emit_function(name, params, ret, body, output, context, indent_level)?;
        }

        IrNode::Lambda { params, ret: _, body } => {
            C::emit_lambda(params, body, output, context)?;
        }

        IrNode::Defer(body_ir) => {
            C::emit_defer(body_ir, output, context, indent_level)?;
        }

        IrNode::FieldAccess { obj, field } => {
            emit_node::<C>(obj, output, context, 0)?;
            output.push('.');
            output.push_str(&C::convert_identifier(field));
        }

        IrNode::Index { collection, index } => {
            emit_node::<C>(collection, output, context, 0)?;
            output.push('[');
            emit_node::<C>(index, output, context, 0)?;
            output.push(']');
        }

        IrNode::Slice { collection, start, end } => {
            C::emit_slice(collection, start, end, output, context)?;
        }

        IrNode::Cast { value, to_type } => {
            let type_name = C::map_type_for_param(to_type);
            output.push('(');
            output.push_str(&type_name);
            output.push(')');
            emit_node::<C>(value, output, context, 0)?;
        }

        IrNode::Break => {
            output.push_str(&format!("{}break{}\n", indent, C::semicolon()));
        }

        IrNode::Continue => {
            output.push_str(&format!("{}continue{}\n", indent, C::semicolon()));
        }

        IrNode::Match { value, arms } => {
            C::emit_match(value, arms, output, context, indent_level)?;
        }
    }

    Ok(())
}

/// Build the final output by wrapping the emitted body with imports, typedefs,
/// and language-specific wrappers.
pub fn build_output<C: EmitConfig>(
    body: &str,
    context: &EmitContext,
) -> String {
    let mut final_output = String::new();

    // Emit imports
    C::emit_imports(&context.imports, &mut final_output);

    // Emit typedefs
    C::emit_typedefs(&context.typedefs, &mut final_output);

    // Emit wrapper start
    C::emit_wrapper_start(&mut final_output);

    // Emit body
    final_output.push_str(body);

    // Emit wrapper end
    C::emit_wrapper_end(&mut final_output);

    // Apply idioms
    C::apply_idioms(&final_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal test config for testing the generic emitter.
    struct TestConfig;

    impl EmitConfig for TestConfig {
        fn lang_spec() -> LangSpec { LangSpec::xcl("1") }
        fn bool_true() -> &'static str { "true" }
        fn bool_false() -> &'static str { "false" }
        fn nil_literal() -> &'static str { "null" }
        fn int_suffix() -> &'static str { "" }
        fn float_suffix() -> &'static str { ".0" }
        fn quote_char() -> char { '"' }
        fn var_keyword() -> &'static str { "var" }
        fn mut_prefix() -> &'static str { "" }
        fn const_prefix() -> &'static str { "" }
        fn semicolon() -> &'static str { ";" }
        fn block_open() -> &'static str { "{" }
        fn block_close() -> &'static str { "}" }
        fn map_type(ty: &IrType) -> String { ty.name() }
        fn convert_identifier(name: &str) -> String { name.to_string() }
        fn convert_type_name(name: &str) -> String { name.to_string() }
        fn emit_record(_name: &str, _fields: &[(String, IrType)], _output: &mut String, _typedefs: &mut Vec<String>, _indent: usize) {}
        fn emit_record_lit(_name: &str, _fields: &[(String, Ir)], _output: &mut String, _ctx: &mut EmitContext) -> Result<(), TranslateError> { Ok(()) }
        fn emit_function(_name: &str, _params: &[(String, IrType)], _ret: &IrType, _body: &Ir, _output: &mut String, _ctx: &mut EmitContext, _indent: usize) -> Result<(), TranslateError> { Ok(()) }
        fn emit_defer(_body: &Ir, _output: &mut String, _ctx: &mut EmitContext, _indent: usize) -> Result<(), TranslateError> { Ok(()) }
        fn emit_lambda(_params: &[(String, IrType)], _body: &Ir, _output: &mut String, _ctx: &mut EmitContext) -> Result<(), TranslateError> { Ok(()) }
        fn emit_match(_value: &Ir, _arms: &[(String, Vec<String>, Box<Ir>)], _output: &mut String, _ctx: &mut EmitContext, _indent: usize) -> Result<(), TranslateError> { Ok(()) }
        fn emit_slice(_collection: &Ir, _start: &Ir, _end: &Ir, _output: &mut String, _ctx: &mut EmitContext) -> Result<(), TranslateError> { Ok(()) }
        fn emit_wrapper_start(_output: &mut String) {}
        fn emit_wrapper_end(_output: &mut String) {}
        fn emit_imports(_imports: &[String], _output: &mut String) {}
        fn emit_typedefs(_typedefs: &[String], _output: &mut String) {}
    }

    #[test]
    fn test_emit_literal() {
        assert_eq!(emit_literal::<TestConfig>(&IrLiteral::Bool(true)), "true");
        assert_eq!(emit_literal::<TestConfig>(&IrLiteral::Bool(false)), "false");
        assert_eq!(emit_literal::<TestConfig>(&IrLiteral::Int(42)), "42");
        assert_eq!(emit_literal::<TestConfig>(&IrLiteral::Nil), "null");
    }

    #[test]
    fn test_emit_var() {
        let ir = Ir::var("foo".to_string(), crate::translate::ir::node::Origin::new(LangSpec::xcl("1")));
        let mut output = String::new();
        let mut ctx = EmitContext::default();
        emit_node::<TestConfig>(&ir, &mut output, &mut ctx, 0).unwrap();
        assert_eq!(output, "foo");
    }
}
