//! C11 backend implementation using the generic emitter.
//!
//! Converts IR to C11 (ISO/IEC 9899:2011) source code.

use crate::translate::backend::{Backend, TranslateError};
use crate::translate::emitter::{self, EmitConfig, EmitContext};
use crate::translate::ir::node::{Ir, IrLiteral};
use crate::translate::ir::ops::Feature;
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;
use crate::translate::maps::map_type;
use crate::translate::utils;
use super::idioms::{CVersion, to_c_identifier, apply_idioms};

pub struct C11;
impl C11 { pub fn new() -> Self { Self } }

impl Backend for C11 {
    fn lang_spec(&self) -> LangSpec { LangSpec::c("11") }
    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Records => true,
            Feature::PatternMatching => false,
            Feature::Generics => true, // _Generic
            Feature::NullSafety => false,
            Feature::Defer => false,
            Feature::Async => false,
            Feature::Closures => false,
            Feature::Traits => false,
            Feature::SumTypes => false,
            Feature::StringInterpolation => false,
            Feature::MultiLineStrings => false,
            Feature::RawStrings => false,
            Feature::TypeInference => false,
            Feature::OperatorOverloading => false,
            Feature::MatchExpressions => false,
        }
    }
    fn emit(&self, ir: &[Ir]) -> Result<String, TranslateError> {
        let mut body = String::new();
        let mut context = EmitContext::default();
        for node in ir {
            emitter::emit_node::<C11Config>(node, &mut body, &mut context, 0)?;
        }
        Ok(emitter::build_output::<C11Config>(&body, &context))
    }
}

struct C11Config;
impl EmitConfig for C11Config {
    fn lang_spec() -> LangSpec { LangSpec::c("11") }
    fn bool_true() -> &'static str { "1" }
    fn bool_false() -> &'static str { "0" }
    fn nil_literal() -> &'static str { "NULL" }
    fn int_suffix() -> &'static str { "" }
    fn float_suffix() -> &'static str { ".0" }
    fn quote_char() -> char { '"' }
    fn var_keyword() -> &'static str { "int" }
    fn mut_prefix() -> &'static str { "" }
    fn const_prefix() -> &'static str { "const " }
    fn semicolon() -> &'static str { ";" }
    fn block_open() -> &'static str { "{" }
    fn block_close() -> &'static str { "}" }
    fn map_type(ty: &IrType) -> String {
        map_type(ty, &LangSpec::c("11")).unwrap_or_else(|| "int".to_string())
    }
    fn convert_identifier(name: &str) -> String { to_c_identifier(name, CVersion::C11) }
    fn convert_type_name(name: &str) -> String { to_c_identifier(name, CVersion::C11) }

    fn emit_record(name: &str, fields: &[(String, IrType)], _output: &mut String, typedefs: &mut Vec<String>, _indent_level: usize) {
        let record_name = to_c_identifier(name, CVersion::C11);
        let fields_str: Vec<String> = fields.iter().map(|(fname, fty)| {
            let type_name = map_type(fty, &LangSpec::c("11")).unwrap_or_else(|| "int".to_string());
            format!("    {} {},", type_name, to_c_identifier(fname, CVersion::C11))
        }).collect();
        typedefs.push(format!("typedef struct {{\n{}\n}} {};\n", fields_str.join("\n"), record_name));
    }
    fn emit_record_lit(name: &str, fields: &[(String, Ir)], output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let record_name = to_c_identifier(name, CVersion::C11);
        let fields_str: Vec<String> = fields.iter().map(|(fname, fval)| {
            let val_str = emitter::emit_to_string::<C11Config>(fval, context, 0)?;
            Ok(format!("{}.{} = {}", record_name, to_c_identifier(fname, CVersion::C11), val_str))
        }).collect::<Result<Vec<_>, _>>()?;
        output.push_str(&format!("({{{}}})", fields_str.join(", ")));
        Ok(())
    }
    fn emit_function(name: &str, params: &[(String, IrType)], ret: &IrType, body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let func_name = to_c_identifier(name, CVersion::C11);
        let ret_type = map_type(ret, &LangSpec::c("11")).unwrap_or_else(|| "void".to_string());
        let indent = "    ".repeat(indent_level);
        let params_str: Vec<String> = params.iter().map(|(pname, pty)| {
            let type_name = map_type(pty, &LangSpec::c("11")).unwrap_or_else(|| "int".to_string());
            format!("{} {}", type_name, to_c_identifier(pname, CVersion::C11))
        }).collect();
        let body_str = emitter::emit_to_string::<C11Config>(body, context, indent_level + 1)?;
        output.push_str(&format!("{}{} {}({}) {{\n{}\n{}}}\n", indent, ret_type, func_name, params_str.join(", "), body_str, indent));
        Ok(())
    }
    fn emit_defer(body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let body_str = emitter::emit_to_string::<C11Config>(body, context, indent_level + 1)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}// TODO: manual cleanup (defer not supported in C11)\n{}\n", indent, body_str));
        Ok(())
    }
    fn emit_lambda(params: &[(String, IrType)], body: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(pname, pty)| {
            let type_name = map_type(pty, &LangSpec::c("11")).unwrap_or_else(|| "int".to_string());
            format!("{} {}", type_name, to_c_identifier(pname, CVersion::C11))
        }).collect();
        let body_str = emitter::emit_to_string::<C11Config>(body, context, 0)?;
        output.push_str(&format!("// Lambda not supported in C11\nstatic inline void lambda_func({}) {{\n{}\n}}\n", params_str.join(", "), body_str));
        Ok(())
    }
    fn emit_match(value: &Ir, arms: &[(String, Vec<String>, Box<Ir>)], output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let value_str = emitter::emit_to_string::<C11Config>(value, context, 0)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}switch ({}) {{\n", indent, value_str));
        for (pattern, _guards, body_ir) in arms {
            let body_str = emitter::emit_to_string::<C11Config>(body_ir, context, indent_level + 1)?;
            output.push_str(&format!("{}    case {}:\n{}\n        break;\n", indent, pattern, body_str));
        }
        output.push_str(&format!("{}}}\n", indent));
        Ok(())
    }
    fn emit_slice(collection: &Ir, start: &Ir, end: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        emitter::emit_node::<C11Config>(collection, output, context, 0)?;
        output.push_str(" /* slice: use strncpy */");
        let _ = (start, end);
        Ok(())
    }
    fn emit_wrapper_start(output: &mut String) { output.push_str("int main(void) {\n"); }
    fn emit_wrapper_end(output: &mut String) { output.push_str("\n    return 0;\n}\n"); }
    fn emit_imports(imports: &[String], output: &mut String) {
        let mut includes = imports.to_vec();
        includes.push("stdio.h".to_string());
        includes.push("stdlib.h".to_string());
        includes.push("string.h".to_string());
        includes.push("stdatomic.h".to_string());
        includes.sort();
        includes.dedup();
        for include in &includes { output.push_str(&format!("#include <{}>\n", include)); }
        output.push('\n');
    }
    fn emit_typedefs(typedefs: &[String], output: &mut String) {
        for typedef in typedefs { output.push_str(typedef); output.push('\n'); }
        if !typedefs.is_empty() { output.push('\n'); }
    }
    fn apply_idioms(code: &str) -> String { apply_idioms(code, CVersion::C11) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_c11_lang_spec() { assert_eq!(C11::new().lang_spec(), LangSpec::c("11")); }
    #[test] fn test_c11_has_generics() { assert!(C11::new().supports(Feature::Generics)); }
    #[test] fn test_emit_literal() {
        assert_eq!(emitter::emit_literal::<C11Config>(&IrLiteral::Bool(true)), "1");
        assert_eq!(emitter::emit_literal::<C11Config>(&IrLiteral::Nil), "NULL");
    }
}
