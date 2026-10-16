//! Rust 2024 backend implementation using the generic emitter.
//!
//! Converts IR to Rust 2024 source code.

use crate::translate::backend::{Backend, TranslateError};
use crate::translate::emitter::{self, EmitConfig, EmitContext};
use crate::translate::ir::node::{Ir, IrLiteral};
use crate::translate::ir::ops::Feature;
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;
use crate::translate::maps::map_type;
use crate::translate::utils;

pub struct Rust2024;
impl Rust2024 { pub fn new() -> Self { Self } }

impl Backend for Rust2024 {
    fn lang_spec(&self) -> LangSpec { LangSpec::rust("2024") }
    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Records => true,
            Feature::PatternMatching => true,
            Feature::Generics => true,
            Feature::NullSafety => true,
            Feature::Defer => true,
            Feature::Async => true,
            Feature::Closures => true,
            Feature::Traits => true,
            Feature::SumTypes => true,
            Feature::StringInterpolation => false,
            Feature::MultiLineStrings => true,
            Feature::RawStrings => true,
            Feature::TypeInference => true,
            Feature::OperatorOverloading => true,
            Feature::MatchExpressions => true,
        }
    }
    fn emit(&self, ir: &[Ir]) -> Result<String, TranslateError> {
        let mut body = String::new();
        let mut context = EmitContext::default();
        for node in ir {
            emitter::emit_node::<Rust2024Config>(node, &mut body, &mut context, 0)?;
        }
        Ok(emitter::build_output::<Rust2024Config>(&body, &context))
    }
}

struct Rust2024Config;
impl EmitConfig for Rust2024Config {
    fn lang_spec() -> LangSpec { LangSpec::rust("2024") }
    fn bool_true() -> &'static str { "true" }
    fn bool_false() -> &'static str { "false" }
    fn nil_literal() -> &'static str { "None" }
    fn int_suffix() -> &'static str { "i64" }
    fn float_suffix() -> &'static str { "" }
    fn quote_char() -> char { '"' }
    fn var_keyword() -> &'static str { "" }
    fn mut_prefix() -> &'static str { "mut " }
    fn const_prefix() -> &'static str { "" }
    fn semicolon() -> &'static str { ";" }
    fn block_open() -> &'static str { "{" }
    fn block_close() -> &'static str { "}" }
    fn map_type(ty: &IrType) -> String {
        map_type(ty, &LangSpec::rust("2024")).unwrap_or_else(|| "any".to_string())
    }
    fn convert_identifier(name: &str) -> String { name.to_string() }
    fn convert_type_name(name: &str) -> String { name.to_string() }

    fn emit_record(name: &str, fields: &[(String, IrType)], output: &mut String, _typedefs: &mut Vec<String>, indent_level: usize) {
        let indent = "    ".repeat(indent_level);
        let fields_str: Vec<String> = fields.iter().map(|(fname, fty)| {
            let type_name = map_type(fty, &LangSpec::rust("2024")).unwrap_or_else(|| "any".to_string());
            format!("    {}: {},", type_name, fname)
        }).collect();
        output.push_str(&format!("{}struct {} {{\n{}\n{}}}\n", indent, name, fields_str.join("\n"), indent));
    }
    fn emit_record_lit(name: &str, fields: &[(String, Ir)], output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let fields_str: Vec<String> = fields.iter().map(|(fname, fval)| {
            let val_str = emitter::emit_to_string::<Rust2024Config>(fval, context, 0)?;
            Ok(format!("{}: {}", fname, val_str))
        }).collect::<Result<Vec<_>, _>>()?;
        output.push_str(&format!("{} {{ {} }}", name, fields_str.join(", ")));
        Ok(())
    }
    fn emit_function(name: &str, params: &[(String, IrType)], ret: &IrType, body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let ret_type = map_type(ret, &LangSpec::rust("2024")).unwrap_or_else(|| "()".to_string());
        let indent = "    ".repeat(indent_level);
        let params_str: Vec<String> = params.iter().map(|(pname, pty)| {
            let type_name = map_type(pty, &LangSpec::rust("2024")).unwrap_or_else(|| "any".to_string());
            format!("{}: {}", pname, type_name)
        }).collect();
        let body_str = emitter::emit_to_string::<Rust2024Config>(body, context, indent_level + 1)?;
        let inner_indent = "    ".repeat(indent_level + 1);
        let body_indented = emitter::re_indent(&body_str, &inner_indent);
        output.push_str(&format!("{}fn {}({}) -> {} {{\n{}\n{}}}\n\n", indent, name, params_str.join(", "), ret_type, body_indented, indent));
        Ok(())
    }
    fn emit_defer(body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let body_str = emitter::emit_to_string::<Rust2024Config>(body, context, indent_level + 1)?;
        let inner_indent = "    ".repeat(indent_level + 1);
        let body_indented = emitter::re_indent(&body_str, &inner_indent);
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}defer {{\n{}\n{}}}\n", indent, body_indented, indent));
        Ok(())
    }
    fn emit_lambda(params: &[(String, IrType)], body: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
        let body_str = emitter::emit_to_string::<Rust2024Config>(body, context, 0)?;
        output.push_str(&format!("|{}| {}", params_str.join(", "), body_str));
        Ok(())
    }
    fn emit_match(value: &Ir, arms: &[(String, Vec<String>, Box<Ir>)], output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let value_str = emitter::emit_to_string::<Rust2024Config>(value, context, 0)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}match {} {{\n", indent, value_str));
        for (pattern, _guards, body_ir) in arms {
            let body_str = emitter::emit_to_string::<Rust2024Config>(body_ir, context, indent_level + 1)?;
            let inner_indent = "    ".repeat(indent_level + 2);
            let body_indented = emitter::re_indent(&body_str, &inner_indent);
            output.push_str(&format!("{}    {} => {{\n{}\n{}    }},\n", indent, pattern, body_indented, indent));
        }
        output.push_str(&format!("{}}}\n", indent));
        Ok(())
    }
    fn emit_slice(collection: &Ir, start: &Ir, end: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        emitter::emit_node::<Rust2024Config>(collection, output, context, 0)?;
        output.push_str("[");
        emitter::emit_node::<Rust2024Config>(start, output, context, 0)?;
        output.push(':');
        emitter::emit_node::<Rust2024Config>(end, output, context, 0)?;
        output.push(']');
        Ok(())
    }
    fn emit_wrapper_start(_output: &mut String) {}
    fn emit_wrapper_end(_output: &mut String) {}
    fn emit_imports(_imports: &[String], _output: &mut String) {}
    fn emit_typedefs(_typedefs: &[String], _output: &mut String) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_rust2024_lang_spec() { assert_eq!(Rust2024::new().lang_spec(), LangSpec::rust("2024")); }
    #[test] fn test_rust2024_has_defer() { assert!(Rust2024::new().supports(Feature::Defer)); }
    #[test] fn test_emit_literal() {
        assert_eq!(emitter::emit_literal::<Rust2024Config>(&IrLiteral::Bool(true)), "true");
        assert_eq!(emitter::emit_literal::<Rust2024Config>(&IrLiteral::Int(42)), "42i64");
        assert_eq!(emitter::emit_literal::<Rust2024Config>(&IrLiteral::Nil), "None");
    }
}
