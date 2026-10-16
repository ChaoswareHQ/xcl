//! Java 8 backend implementation using the generic emitter.
//!
//! Converts IR to Java 8 source code. Java 8 (LTS, 2014) introduced
//! lambdas, Streams API, and Optional — but lacks `var`, records,
//! text blocks, and switch expressions.

use crate::translate::backend::{Backend, TranslateError};
use crate::translate::emitter::{self, EmitConfig, EmitContext};
use crate::translate::ir::node::{Ir, IrLiteral};
use crate::translate::ir::ops::Feature;
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;
use crate::translate::maps::map_type;
use crate::translate::utils::{self, to_camel_case, to_pascal_case};
use super::idioms::{JavaVersion, apply_idioms};

/// Java 8 backend.
pub struct Java8;

impl Java8 {
    pub fn new() -> Self { Self }
}

impl Backend for Java8 {
    fn lang_spec(&self) -> LangSpec { LangSpec::java("8") }

    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Records => false,
            Feature::PatternMatching => false,
            Feature::Generics => true,
            Feature::NullSafety => false,
            Feature::Defer => false,
            Feature::Async => false,
            Feature::Closures => true,
            Feature::Traits => true,
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
            emitter::emit_node::<Java8Config>(node, &mut body, &mut context, 0)?;
        }
        Ok(emitter::build_output::<Java8Config>(&body, &context))
    }
}

struct Java8Config;

impl EmitConfig for Java8Config {
    fn lang_spec() -> LangSpec { LangSpec::java("8") }
    fn bool_true() -> &'static str { "true" }
    fn bool_false() -> &'static str { "false" }
    fn nil_literal() -> &'static str { "null" }
    fn int_suffix() -> &'static str { "" }
    fn float_suffix() -> &'static str { ".0" }
    fn quote_char() -> char { '"' }
    fn var_keyword() -> &'static str { "Object" }
    fn mut_prefix() -> &'static str { "/* mutable */ " }
    fn const_prefix() -> &'static str { "" }
    fn semicolon() -> &'static str { ";" }
    fn block_open() -> &'static str { "{" }
    fn block_close() -> &'static str { "}" }

    fn map_type(ty: &IrType) -> String {
        map_type(ty, &LangSpec::java("8")).unwrap_or_else(|| "Object".to_string())
    }

    fn convert_identifier(name: &str) -> String { to_camel_case(name) }
    fn convert_type_name(name: &str) -> String { to_pascal_case(name) }

    fn emit_record(
        name: &str,
        fields: &[(String, IrType)],
        output: &mut String,
        _typedefs: &mut Vec<String>,
        indent_level: usize,
    ) {
        // Java 8: no records, emit as class with fields and getters
        let record_name = to_pascal_case(name);
        let indent = "    ".repeat(indent_level);
        let fields_str: Vec<String> = fields
            .iter()
            .map(|(fname, fty)| {
                let type_name = map_type(fty, &LangSpec::java("8"))
                    .unwrap_or_else(|| "Object".to_string());
                format!("    private {} {};", type_name, to_camel_case(fname))
            })
            .collect();
        let getters: Vec<String> = fields
            .iter()
            .map(|(fname, fty)| {
                let type_name = map_type(fty, &LangSpec::java("8"))
                    .unwrap_or_else(|| "Object".to_string());
                let field_name = to_camel_case(fname);
                let first_upper: String = fname.chars().next().unwrap().to_uppercase().collect();
                let rest = if fname.len() > 1 { &fname[1..] } else { "" };
                let getter_name = format!("get{}{}", first_upper, rest);
                format!("    public {} {}() {{\n        return this.{};\n    }}", type_name, getter_name, field_name)
            })
            .collect();
        output.push_str(&format!(
            "{}public class {} {{\n{}\n\n{}\n    }}\n",
            indent, record_name, fields_str.join("\n"), getters.join("\n")
        ));
    }

    fn emit_record_lit(
        name: &str,
        fields: &[(String, Ir)],
        output: &mut String,
        context: &mut EmitContext,
    ) -> Result<(), TranslateError> {
        let record_name = to_pascal_case(name);
        let fields_str: Vec<String> = fields
            .iter()
            .map(|(_fname, fval)| emitter::emit_to_string::<Java8Config>(fval, context, 0))
            .collect::<Result<Vec<_>, _>>()?;
        output.push_str(&format!("new {}({})", record_name, fields_str.join(", ")));
        Ok(())
    }

    fn emit_function(
        name: &str,
        params: &[(String, IrType)],
        ret: &IrType,
        body: &Ir,
        output: &mut String,
        context: &mut EmitContext,
        indent_level: usize,
    ) -> Result<(), TranslateError> {
        let method_name = to_camel_case(name);
        let ret_type = map_type(ret, &LangSpec::java("8")).unwrap_or_else(|| "void".to_string());
        let indent = "    ".repeat(indent_level);
        let params_str: Vec<String> = params
            .iter()
            .map(|(pname, pty)| {
                let type_name = map_type(pty, &LangSpec::java("8")).unwrap_or_else(|| "Object".to_string());
                format!("{} {}", type_name, to_camel_case(pname))
            })
            .collect();
        let body_str = emitter::emit_to_string::<Java8Config>(body, context, indent_level + 1)?;
        let inner_indent = "    ".repeat(indent_level + 1);
        let body_indented = emitter::re_indent(&body_str, &inner_indent);
        output.push_str(&format!(
            "{}public {} {}({}) {{\n{}\n{}}}\n\n",
            indent, ret_type, method_name, params_str.join(", "), body_indented, indent
        ));
        Ok(())
    }

    fn emit_defer(
        body: &Ir,
        output: &mut String,
        context: &mut EmitContext,
        indent_level: usize,
    ) -> Result<(), TranslateError> {
        let body_str = emitter::emit_to_string::<Java8Config>(body, context, indent_level + 1)?;
        let inner_indent = "    ".repeat(indent_level + 1);
        let body_indented = emitter::re_indent(&body_str, &inner_indent);
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!(
            "{}try {{}} finally {{\n{}\n{}}}\n",
            indent, body_indented, indent
        ));
        Ok(())
    }

    fn emit_lambda(
        params: &[(String, IrType)],
        body: &Ir,
        output: &mut String,
        context: &mut EmitContext,
    ) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(pname, _pty)| to_camel_case(pname)).collect();
        let body_str = emitter::emit_to_string::<Java8Config>(body, context, 0)?;
        output.push_str(&format!("({}) -> {}", params_str.join(", "), body_str));
        Ok(())
    }

    fn emit_match(
        value: &Ir,
        arms: &[(String, Vec<String>, Box<Ir>)],
        output: &mut String,
        context: &mut EmitContext,
        indent_level: usize,
    ) -> Result<(), TranslateError> {
        // Java 8: no switch expressions, use traditional switch
        let value_str = emitter::emit_to_string::<Java8Config>(value, context, 0)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}switch ({}) {{\n", indent, value_str));
        for (pattern, _guards, body_ir) in arms {
            let body_str = emitter::emit_to_string::<Java8Config>(body_ir, context, indent_level + 1)?;
            let inner_indent = "    ".repeat(indent_level + 2);
            let body_indented = emitter::re_indent(&body_str, &inner_indent);
            output.push_str(&format!(
                "{}    case {}:\n{}\n        break;\n",
                indent, pattern, body_indented
            ));
        }
        output.push_str(&format!("{}}}\n", indent));
        Ok(())
    }

    fn emit_slice(
        collection: &Ir,
        start: &Ir,
        end: &Ir,
        output: &mut String,
        context: &mut EmitContext,
    ) -> Result<(), TranslateError> {
        emitter::emit_node::<Java8Config>(collection, output, context, 0)?;
        output.push_str(".substring(");
        emitter::emit_node::<Java8Config>(start, output, context, 0)?;
        output.push(',');
        emitter::emit_node::<Java8Config>(end, output, context, 0)?;
        output.push(')');
        Ok(())
    }

    fn emit_wrapper_start(output: &mut String) {
        output.push_str("\npublic class Main {\n");
    }
    fn emit_wrapper_end(output: &mut String) {
        output.push_str("}\n");
    }
    fn emit_imports(imports: &[String], output: &mut String) {
        for import in imports {
            output.push_str(&format!("import {};\n", import));
        }
        if !imports.is_empty() { output.push('\n'); }
    }
    fn emit_typedefs(_typedefs: &[String], _output: &mut String) {}
    fn apply_idioms(code: &str) -> String { apply_idioms(code, JavaVersion::V8) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_java8_lang_spec() {
        let backend = Java8::new();
        assert_eq!(backend.lang_spec(), LangSpec::java("8"));
    }

    #[test]
    fn test_java8_no_records() {
        let backend = Java8::new();
        assert!(!backend.supports(Feature::Records));
    }

    #[test]
    fn test_java8_no_var() {
        let backend = Java8::new();
        assert!(!backend.supports(Feature::TypeInference));
    }

    #[test]
    fn test_java8_has_lambdas() {
        let backend = Java8::new();
        assert!(backend.supports(Feature::Closures));
    }

    #[test]
    fn test_emit_literal() {
        assert_eq!(emitter::emit_literal::<Java8Config>(&IrLiteral::Bool(true)), "true");
        assert_eq!(emitter::emit_literal::<Java8Config>(&IrLiteral::Int(42)), "42");
        assert_eq!(emitter::emit_literal::<Java8Config>(&IrLiteral::Nil), "null");
    }
}
