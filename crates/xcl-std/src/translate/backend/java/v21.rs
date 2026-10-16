//! Java 21 backend implementation using the generic emitter.
//!
//! Converts IR to Java 21 source code. Java 21 (LTS, 2023) added
//! virtual threads, record patterns, and pattern matching for switch.

use crate::translate::backend::{Backend, TranslateError};
use crate::translate::emitter::{self, EmitConfig, EmitContext};
use crate::translate::ir::node::{Ir, IrLiteral};
use crate::translate::ir::ops::Feature;
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;
use crate::translate::maps::map_type;
use crate::translate::utils::{self, to_camel_case, to_pascal_case};
use super::idioms::{JavaVersion, apply_idioms};

pub struct Java21;
impl Java21 { pub fn new() -> Self { Self } }

impl Backend for Java21 {
    fn lang_spec(&self) -> LangSpec { LangSpec::java("21") }
    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Records => true,
            Feature::PatternMatching => true,
            Feature::Generics => true,
            Feature::NullSafety => false,
            Feature::Defer => false,
            Feature::Async => true,  // Virtual threads
            Feature::Closures => true,
            Feature::Traits => true,
            Feature::SumTypes => false,
            Feature::StringInterpolation => false,
            Feature::MultiLineStrings => true,
            Feature::RawStrings => true,
            Feature::TypeInference => true,
            Feature::OperatorOverloading => false,
            Feature::MatchExpressions => true,
        }
    }
    fn emit(&self, ir: &[Ir]) -> Result<String, TranslateError> {
        let mut body = String::new();
        let mut context = EmitContext::default();
        for node in ir {
            emitter::emit_node::<Java21Config>(node, &mut body, &mut context, 0)?;
        }
        Ok(emitter::build_output::<Java21Config>(&body, &context))
    }
}

struct Java21Config;
impl EmitConfig for Java21Config {
    fn lang_spec() -> LangSpec { LangSpec::java("21") }
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
    fn map_type(ty: &IrType) -> String {
        map_type(ty, &LangSpec::java("21")).unwrap_or_else(|| "Object".to_string())
    }
    fn convert_identifier(name: &str) -> String { to_camel_case(name) }
    fn convert_type_name(name: &str) -> String { to_pascal_case(name) }

    fn emit_record(name: &str, fields: &[(String, IrType)], output: &mut String, _typedefs: &mut Vec<String>, indent_level: usize) {
        let record_name = to_pascal_case(name);
        let indent = "    ".repeat(indent_level);
        let fields_str: Vec<String> = fields.iter().map(|(fname, fty)| {
            let type_name = map_type(fty, &LangSpec::java("21")).unwrap_or_else(|| "Object".to_string());
            format!("{} {}", type_name, to_camel_case(fname))
        }).collect();
        output.push_str(&format!("{}public record {}({}) {{}}\n", indent, record_name, fields_str.join(", ")));
    }
    fn emit_record_lit(name: &str, fields: &[(String, Ir)], output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let record_name = to_pascal_case(name);
        let fields_str: Vec<String> = fields.iter().map(|(_, fval)| emitter::emit_to_string::<Java21Config>(fval, context, 0)).collect::<Result<Vec<_>, _>>()?;
        output.push_str(&format!("new {}({})", record_name, fields_str.join(", ")));
        Ok(())
    }
    fn emit_function(name: &str, params: &[(String, IrType)], ret: &IrType, body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let method_name = to_camel_case(name);
        let ret_type = map_type(ret, &LangSpec::java("21")).unwrap_or_else(|| "void".to_string());
        let indent = "    ".repeat(indent_level);
        let params_str: Vec<String> = params.iter().map(|(pname, pty)| {
            let type_name = map_type(pty, &LangSpec::java("21")).unwrap_or_else(|| "Object".to_string());
            format!("{} {}", type_name, to_camel_case(pname))
        }).collect();
        let body_str = emitter::emit_to_string::<Java21Config>(body, context, indent_level + 1)?;
        output.push_str(&format!("{}public {} {}({}) {{\n{}\n{}}}\n", indent, ret_type, method_name, params_str.join(", "), body_str, indent));
        Ok(())
    }
    fn emit_defer(body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let body_str = emitter::emit_to_string::<Java21Config>(body, context, indent_level + 1)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}try {{}} finally {{\n{}\n{}}}\n", indent, body_str, indent));
        Ok(())
    }
    fn emit_lambda(params: &[(String, IrType)], body: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(pname, _pty)| to_camel_case(pname)).collect();
        let body_str = emitter::emit_to_string::<Java21Config>(body, context, 0)?;
        output.push_str(&format!("({}) -> {}", params_str.join(", "), body_str));
        Ok(())
    }
    fn emit_match(value: &Ir, arms: &[(String, Vec<String>, Box<Ir>)], output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let value_str = emitter::emit_to_string::<Java21Config>(value, context, 0)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}switch ({}) {{\n", indent, value_str));
        for (pattern, _guards, body_ir) in arms {
            let body_str = emitter::emit_to_string::<Java21Config>(body_ir, context, indent_level + 1)?;
            output.push_str(&format!("{}    case {} -> {{\n{}\n{}    }}\n", indent, pattern, body_str, indent));
        }
        output.push_str(&format!("{}}}\n", indent));
        Ok(())
    }
    fn emit_slice(collection: &Ir, start: &Ir, end: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        emitter::emit_node::<Java21Config>(collection, output, context, 0)?;
        output.push_str(".substring(");
        emitter::emit_node::<Java21Config>(start, output, context, 0)?;
        output.push(',');
        emitter::emit_node::<Java21Config>(end, output, context, 0)?;
        output.push(')');
        Ok(())
    }
    fn emit_wrapper_start(output: &mut String) { output.push_str("public class Main {\n"); }
    fn emit_wrapper_end(output: &mut String) { output.push_str("}\n"); }
    fn emit_imports(imports: &[String], output: &mut String) {
        for import in imports { output.push_str(&format!("import {};\n", import)); }
        if !imports.is_empty() { output.push('\n'); }
    }
    fn emit_typedefs(_typedefs: &[String], _output: &mut String) {}
    fn apply_idioms(code: &str) -> String { apply_idioms(code, JavaVersion::V21) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_java21_lang_spec() { assert_eq!(Java21::new().lang_spec(), LangSpec::java("21")); }
    #[test] fn test_java21_has_virtual_threads() { assert!(Java21::new().supports(Feature::Async)); }
    #[test] fn test_emit_literal() {
        assert_eq!(emitter::emit_literal::<Java21Config>(&IrLiteral::Bool(true)), "true");
        assert_eq!(emitter::emit_literal::<Java21Config>(&IrLiteral::Nil), "null");
    }
}
