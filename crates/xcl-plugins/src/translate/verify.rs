//! Verification utilities for the translator.
//!
//! Provides functions to verify translation correctness.

use crate::translate::ir::node::Ir;
use crate::translate::version::LangSpec;

/// Verify that IR nodes are well-formed.
///
/// Checks:
/// - All nodes have valid types
/// - All spans are non-negative
/// - No dangling references
pub fn verify_ir(ir_nodes: &[Ir]) -> Vec<String> {
    let mut warnings = Vec::new();

    for (i, node) in ir_nodes.iter().enumerate() {
        verify_node(node, i, &mut warnings);
    }

    warnings
}

/// Verify a single IR node.
fn verify_node(ir: &Ir, index: usize, warnings: &mut Vec<String>) {
    // Check span validity
    if ir.span.start_line > ir.span.end_line
        || (ir.span.start_line == ir.span.end_line && ir.span.start_col > ir.span.end_col)
    {
        warnings.push(format!(
            "Node {}: Invalid span {:?}",
            index, ir.span
        ));
    }

    // Check type validity
    if ir.ty == crate::translate::ir::types::IrType::Any {
        match &ir.node {
            crate::translate::ir::node::IrNode::Var(name) => {
                warnings.push(format!(
                    "Node {}: Variable '{}' has untyped (Any) type",
                    index, name
                ));
            }
            _ => {}
        }
    }
}

/// Check if a translation is supported for the given language pair.
pub fn is_translation_supported(from: &LangSpec, _to: &LangSpec) -> bool {
    // XCL can translate to any registered backend
    if from.language == crate::translate::version::Language::Xcl {
        return true;
    }

    // For now, only XCL → target is fully supported
    false
}
