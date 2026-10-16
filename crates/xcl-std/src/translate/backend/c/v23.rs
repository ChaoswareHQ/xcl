//! C23 backend implementation using the generic emitter.
//!
//! Converts IR to C23 (ISO/IEC 9899:2024) source code. C23 added:
//! `true`/`false` keywords, `nullptr`, `bool`, `auto` type inference,
//! `typeof`, `_BitInt`, and `constexpr`.

use crate::translate::backend::{Backend, TranslateError};
use crate::translate::emitter::{self, EmitConfig, EmitContext};
use crate::translate::ir::node::{Ir, IrLiteral, IrNode};
use crate::translate::ir::ops::Feature;
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;
use crate::translate::maps::map_type;
use crate::translate::utils;
use super::idioms::{CVersion, to_c_identifier, apply_idioms};

pub struct C23;
impl C23 { pub fn new() -> Self { Self } }

impl Backend for C23 {
    fn lang_spec(&self) -> LangSpec { LangSpec::c("23") }
    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Records => true,
            Feature::PatternMatching => false,
            Feature::Generics => true,
            Feature::NullSafety => true,
            Feature::Defer => false,
            Feature::Async => false,
            Feature::Closures => false,
            Feature::Traits => false,
            Feature::SumTypes => false,
            Feature::StringInterpolation => false,
            Feature::MultiLineStrings => true,
            Feature::RawStrings => false,
            Feature::TypeInference => true,
            Feature::OperatorOverloading => false,
            Feature::MatchExpressions => false,
        }
    }
    fn emit(&self, ir: &[Ir]) -> Result<String, TranslateError> {
        let mut context = EmitContext::default();
        let mut file_level = String::new();
        let mut main_body = String::new();
        for node in ir {
            match &node.node {
                IrNode::Record { .. } | IrNode::Fn { .. } => {
                    emitter::emit_node::<C23Config>(node, &mut file_level, &mut context, 0)?;
                }
                _ => {
                    emitter::emit_node::<C23Config>(node, &mut main_body, &mut context, 0)?;
                }
            }
        }
        let mut final_output = String::new();
        C23Config::emit_imports(&context.imports, &mut final_output);
        C23Config::emit_typedefs(&context.typedefs, &mut final_output);
        if !file_level.trim().is_empty() {
            final_output.push_str(file_level.trim());
            final_output.push('\n');
        }
        let trimmed = main_body.trim();
        if !trimmed.is_empty() {
            final_output.push_str("int main(void) {\n");
            let indented = emitter::re_indent(trimmed, "    ");
            final_output.push_str(&indented);
            final_output.push_str("\n    return 0;\n}\n");
        }
        Ok(C23Config::apply_idioms(&final_output))
    }
}

struct C23Config;
impl EmitConfig for C23Config {
    fn lang_spec() -> LangSpec { LangSpec::c("23") }
    fn bool_true() -> &'static str { "true" }
    fn bool_false() -> &'static str { "false" }
    fn nil_literal() -> &'static str { "nullptr" }
    fn int_suffix() -> &'static str { "" }
    fn float_suffix() -> &'static str { ".0" }
    fn quote_char() -> char { '"' }
    fn var_keyword() -> &'static str { "auto" }
    fn mut_prefix() -> &'static str { "" }
    fn const_prefix() -> &'static str { "const " }
    fn semicolon() -> &'static str { ";" }
    fn block_open() -> &'static str { "{" }
    fn block_close() -> &'static str { "}" }
    fn map_type(ty: &IrType) -> String {
        if !ty.is_primitive() { "auto".to_string() }
        else { map_type(ty, &LangSpec::c("23")).unwrap_or_else(|| "auto".to_string()) }
    }
    fn convert_identifier(name: &str) -> String { to_c_identifier(name, CVersion::C23) }
    fn convert_type_name(name: &str) -> String { to_c_identifier(name, CVersion::C23) }

    fn emit_record(name: &str, fields: &[(String, IrType)], _output: &mut String, typedefs: &mut Vec<String>, _indent_level: usize) {
        let record_name = to_c_identifier(name, CVersion::C23);
        let fields_str: Vec<String> = fields.iter().map(|(fname, fty)| {
            let type_name = map_type(fty, &LangSpec::c("23")).unwrap_or_else(|| "int".to_string());
            format!("    {} {};", type_name, to_c_identifier(fname, CVersion::C23))
        }).collect();
        typedefs.push(format!("typedef struct {{\n{}\n}} {};\n", fields_str.join("\n"), record_name));
    }
    fn emit_record_lit(name: &str, fields: &[(String, Ir)], output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let record_name = to_c_identifier(name, CVersion::C23);
        let fields_str: Vec<String> = fields.iter().map(|(fname, fval)| {
            let val_str = emitter::emit_to_string::<C23Config>(fval, context, 0)?;
            Ok(format!("{}.{} = {}", record_name, to_c_identifier(fname, CVersion::C23), val_str))
        }).collect::<Result<Vec<_>, _>>()?;
        output.push_str(&format!("({{{}}})", fields_str.join(", ")));
        Ok(())
    }
    fn emit_function(name: &str, params: &[(String, IrType)], ret: &IrType, body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let func_name = to_c_identifier(name, CVersion::C23);
        let ret_type = map_type(ret, &LangSpec::c("23")).unwrap_or_else(|| "void".to_string());
        let indent = "    ".repeat(indent_level);
        let inner_indent = "    ".repeat(indent_level + 1);
        let params_str: Vec<String> = params.iter().map(|(pname, pty)| {
            let type_name = map_type(pty, &LangSpec::c("23")).unwrap_or_else(|| "int".to_string());
            format!("{} {}", type_name, to_c_identifier(pname, CVersion::C23))
        }).collect();
        let body_str = emitter::emit_to_string::<C23Config>(body, context, indent_level + 1)?;
        let body_indented = emitter::re_indent(&body_str, &inner_indent);
        output.push_str(&format!("{}{} {}({}) {{\n{}\n{}}}\n\n", indent, ret_type, func_name, params_str.join(", "), body_indented, indent));
        Ok(())
    }
    fn emit_defer(body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let body_str = emitter::emit_to_string::<C23Config>(body, context, indent_level + 1)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}// TODO: manual cleanup (defer not supported in C23)\n{}\n", indent, body_str));
        Ok(())
    }
    fn emit_lambda(params: &[(String, IrType)], body: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(pname, pty)| {
            let type_name = map_type(pty, &LangSpec::c("23")).unwrap_or_else(|| "int".to_string());
            format!("{} {}", type_name, to_c_identifier(pname, CVersion::C23))
        }).collect();
        let body_str = emitter::emit_to_string::<C23Config>(body, context, 0)?;
        output.push_str(&format!("// Lambda not supported in C23\nstatic inline void lambda_func({}) {{\n{}\n}}\n", params_str.join(", "), body_str));
        Ok(())
    }
    fn emit_match(value: &Ir, arms: &[(String, Vec<String>, Box<Ir>)], output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let value_str = emitter::emit_to_string::<C23Config>(value, context, 0)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}switch ({}) {{\n", indent, value_str));
        for (pattern, _guards, body_ir) in arms {
            let body_str = emitter::emit_to_string::<C23Config>(body_ir, context, indent_level + 1)?;
            output.push_str(&format!("{}    case {}:\n{}\n        break;\n", indent, pattern, body_str));
        }
        output.push_str(&format!("{}}}\n", indent));
        Ok(())
    }
    fn emit_slice(collection: &Ir, start: &Ir, end: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        emitter::emit_node::<C23Config>(collection, output, context, 0)?;
        output.push_str(" /* slice: use strncpy */");
        let _ = (start, end);
        Ok(())
    }
    fn emit_wrapper_start(output: &mut String) { output.push_str("\nint main(void) {\n"); }
    fn emit_wrapper_end(output: &mut String) { output.push_str("    return 0;\n}\n"); }
    fn emit_imports(imports: &[String], output: &mut String) {
        let mut includes = imports.to_vec();
        includes.push("stdio.h".to_string());
        includes.push("stdlib.h".to_string());
        includes.push("string.h".to_string());
        includes.push("stdalign.h".to_string());
        includes.push("stdatomic.h".to_string());
        includes.push("stdnoreturn.h".to_string());
        includes.push("stdbool.h".to_string());
        includes.sort();
        includes.dedup();
        for include in &includes { output.push_str(&format!("#include <{}>\n", include)); }
        output.push('\n');
    }
    fn emit_typedefs(typedefs: &[String], output: &mut String) {
        for typedef in typedefs { output.push_str(typedef); output.push('\n'); }
        if !typedefs.is_empty() { output.push('\n'); }
    }
    fn apply_idioms(code: &str) -> String { apply_idioms(code, CVersion::C23) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_c23_lang_spec() { assert_eq!(C23::new().lang_spec(), LangSpec::c("23")); }
    #[test] fn test_c23_supports_type_inference() { assert!(C23::new().supports(Feature::TypeInference)); }
    #[test] fn test_c23_supports_null_safety() { assert!(C23::new().supports(Feature::NullSafety)); }
    #[test] fn test_emit_literal_bool_c23() {
        assert_eq!(emitter::emit_literal::<C23Config>(&IrLiteral::Bool(true)), "true");
        assert_eq!(emitter::emit_literal::<C23Config>(&IrLiteral::Bool(false)), "false");
    }
    #[test] fn test_emit_literal_nil_c23() {
        assert_eq!(emitter::emit_literal::<C23Config>(&IrLiteral::Nil), "nullptr");
    }
}
