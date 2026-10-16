//! Python 3.13 backend implementation using the generic emitter.
//!
//! Converts IR to Python 3.13 source code.

use crate::translate::backend::{Backend, TranslateError};
use crate::translate::emitter::{self, EmitConfig, EmitContext};
use crate::translate::ir::node::Ir;
use crate::translate::ir::ops::{Feature, IrOp};
use crate::translate::ir::types::IrType;
use crate::translate::version::LangSpec;

pub struct Python313;
impl Python313 { pub fn new() -> Self { Self } }

impl Backend for Python313 {
    fn lang_spec(&self) -> LangSpec { LangSpec::python("313") }
    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Records => true,
            Feature::PatternMatching => true,
            Feature::Generics => true,
            Feature::NullSafety => false,
            Feature::Defer => false,
            Feature::Async => true,
            Feature::Closures => true,
            Feature::Traits => false,
            Feature::SumTypes => true,
            Feature::StringInterpolation => true,
            Feature::MultiLineStrings => true,
            Feature::RawStrings => true,
            Feature::TypeInference => false,
            Feature::OperatorOverloading => true,
            Feature::MatchExpressions => true,
        }
    }
    fn emit(&self, ir: &[Ir]) -> Result<String, TranslateError> {
        let mut body = String::new();
        let mut context = EmitContext::default();
        for node in ir {
            emitter::emit_node::<Python313Config>(node, &mut body, &mut context, 0)?;
            body.push('\n');
        }
        Ok(emitter::build_output::<Python313Config>(&body, &context))
    }
}

struct Python313Config;
impl EmitConfig for Python313Config {
    fn lang_spec() -> LangSpec { LangSpec::python("313") }
    fn bool_true() -> &'static str { "True" }
    fn bool_false() -> &'static str { "False" }
    fn nil_literal() -> &'static str { "None" }
    fn int_suffix() -> &'static str { "" }
    fn float_suffix() -> &'static str { "" }
    fn quote_char() -> char { '\'' }
    fn var_keyword() -> &'static str { "" }
    fn mut_prefix() -> &'static str { "" }
    fn const_prefix() -> &'static str { "" }
    fn semicolon() -> &'static str { "" }
    fn block_open() -> &'static str { "" }
    fn block_close() -> &'static str { "" }
    fn is_indent_based() -> bool { true }
    fn map_type(ty: &IrType) -> String { ty.name() }
    fn convert_identifier(name: &str) -> String { name.to_string() }
    fn convert_type_name(name: &str) -> String { name.to_string() }

    fn bin_op_str(op: &IrOp) -> String {
        match op {
            IrOp::And => "and".to_string(),
            IrOp::Or => "or".to_string(),
            IrOp::Eq => "==".to_string(),
            IrOp::Ne => "!=".to_string(),
            _ => op.to_string(),
        }
    }
    fn unary_op_str(op: &IrOp) -> &'static str {
        match op {
            IrOp::Not => "not ",
            IrOp::Neg => "-",
            _ => "",
        }
    }

    fn emit_record(name: &str, fields: &[(String, IrType)], output: &mut String, _typedefs: &mut Vec<String>, indent_level: usize) {
        let indent = "    ".repeat(indent_level);
        let fields_str: Vec<String> = fields.iter().map(|(n, t)| format!("    {}: {}", n, t)).collect();
        output.push_str(&format!("{}class {}:\n{}\n", indent, name, fields_str.join("\n")));
    }
    fn emit_record_lit(name: &str, fields: &[(String, Ir)], output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let fields_str: Vec<String> = fields.iter().map(|(fname, fval)| {
            let val_str = emitter::emit_to_string::<Python313Config>(fval, context, 0)?;
            Ok(format!("{}={}", fname, val_str))
        }).collect::<Result<Vec<_>, _>>()?;
        output.push_str(&format!("{}({})", name, fields_str.join(", ")));
        Ok(())
    }
    fn emit_function(name: &str, params: &[(String, IrType)], ret: &IrType, body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(n, t)| format!("{}: {}", n, t)).collect();
        let ret_str = if *ret != IrType::Nil { format!(" -> {}", ret) } else { String::new() };
        let indent = "    ".repeat(indent_level);
        let inner_indent = "    ".repeat(indent_level + 1);
        let body_str = emitter::emit_to_string::<Python313Config>(body, context, indent_level + 1)?;
        let body_indented = emitter::re_indent(&body_str, &inner_indent);
        output.push_str(&format!("{}def {}({}){}:\n{}\n\n", indent, name, params_str.join(", "), ret_str, body_indented));
        Ok(())
    }
    fn emit_defer(body: &Ir, output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let body_str = emitter::emit_to_string::<Python313Config>(body, context, indent_level + 1)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}\n# defer (use context manager)\n{}", indent, body_str));
        Ok(())
    }
    fn emit_lambda(params: &[(String, IrType)], body: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        let params_str: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
        let body_str = emitter::emit_to_string::<Python313Config>(body, context, 0)?;
        output.push_str(&format!("lambda {}: {}", params_str.join(", "), body_str));
        Ok(())
    }
    fn emit_match(value: &Ir, arms: &[(String, Vec<String>, Box<Ir>)], output: &mut String, context: &mut EmitContext, indent_level: usize) -> Result<(), TranslateError> {
        let value_str = emitter::emit_to_string::<Python313Config>(value, context, 0)?;
        let indent = "    ".repeat(indent_level);
        output.push_str(&format!("{}match {}:", indent, value_str));
        for (pattern, _guards, body_ir) in arms {
            let inner_indent = "    ".repeat(indent_level + 2);
            let body_str = emitter::emit_to_string::<Python313Config>(body_ir, context, indent_level + 2)?;
            let body_indented = emitter::re_indent(&body_str, &inner_indent);
            output.push_str(&format!("\n{}    case {}:", indent, pattern));
            output.push_str(&format!("\n{}", body_indented));
        }
        Ok(())
    }
    fn emit_slice(collection: &Ir, start: &Ir, end: &Ir, output: &mut String, context: &mut EmitContext) -> Result<(), TranslateError> {
        emitter::emit_node::<Python313Config>(collection, output, context, 0)?;
        output.push_str("[");
        emitter::emit_node::<Python313Config>(start, output, context, 0)?;
        output.push(':');
        emitter::emit_node::<Python313Config>(end, output, context, 0)?;
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
    use crate::translate::ir::node::IrLiteral;

    #[test] fn test_python313_lang_spec() { assert_eq!(Python313::new().lang_spec(), LangSpec::python("313")); }
    #[test] fn test_python313_has_match() { assert!(Python313::new().supports(Feature::MatchExpressions)); }
    #[test] fn test_emit_literal() {
        assert_eq!(emitter::emit_literal::<Python313Config>(&IrLiteral::Bool(true)), "True");
        assert_eq!(emitter::emit_literal::<Python313Config>(&IrLiteral::Bool(false)), "False");
        assert_eq!(emitter::emit_literal::<Python313Config>(&IrLiteral::Nil), "None");
    }
}
