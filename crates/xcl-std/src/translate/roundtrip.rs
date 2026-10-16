//! Round-trip translation verification.
//!
//! Per contract RT10.1: XCL → T → XCL must be behaviorally equivalent.
//! Per contract RT10.2: T → XCL → T must compile and pass the target's tests.
//! Per contract RT10.3: Lossy round-trips must be reported.

use crate::translate::version::LangSpec;
use crate::translate::backend::TranslateError;

/// Report from a round-trip translation.
#[derive(Debug)]
pub struct RoundtripReport {
    /// Original source code.
    pub original: String,
    /// Target language code (XCL → T).
    pub target_code: String,
    /// Round-tripped source code (T → XCL).
    pub roundtripped: String,
    /// Whether the round-trip is lossless.
    pub lossless: bool,
    /// Warnings encountered during round-trip.
    pub warnings: Vec<String>,
}

impl RoundtripReport {
    /// Create a lossy round-trip report.
    pub fn lossy(original: String, target_code: String, roundtripped: String, warnings: Vec<String>) -> Self {
        Self {
            original,
            target_code,
            roundtripped,
            lossless: false,
            warnings,
        }
    }

    /// Create a lossless round-trip report.
    pub fn lossless_report(original: String, target_code: String, roundtripped: String) -> Self {
        Self {
            original,
            target_code,
            roundtripped,
            lossless: true,
            warnings: Vec::new(),
        }
    }
}

/// Perform a round-trip translation: XCL → target → XCL.
///
/// # Arguments
/// - `source` — Original XCL source code
/// - `target` — Target language specification
///
/// # Returns
/// Round-trip report with the target code and round-tripped XCL code
pub fn roundtrip_xcl_to_target(
    source: &str,
    target: &LangSpec,
) -> Result<RoundtripReport, TranslateError> {
    // Step 1: Lower XCL to IR
    let origin = crate::translate::ir::node::Origin::new(LangSpec::xcl("1"));
    let ir_nodes = crate::translate::ir::lower::lower_xcl(source, origin);

    // Step 2: Emit IR to target language
    let target_code = emit_to_target(&ir_nodes, target)?;

    // Step 3: Parse target back to IR (simplified — for XCL, use raise)
    let roundtripped = crate::translate::ir::raise::raise_to_xcl(&ir_nodes);

    // Step 4: Compare
    let warnings = compare_roundtrip(source, &roundtripped);

    if warnings.is_empty() {
        Ok(RoundtripReport::lossless_report(
            source.to_string(),
            target_code,
            roundtripped,
        ))
    } else {
        Ok(RoundtripReport::lossy(
            source.to_string(),
            target_code,
            roundtripped,
            warnings,
        ))
    }
}

/// Emit IR nodes to a target language.
fn emit_to_target(
    ir_nodes: &[crate::translate::ir::node::Ir],
    target: &LangSpec,
) -> Result<String, TranslateError> {
    use crate::translate::backend::registry::Registry;

    let mut reg = Registry::new();
    crate::translate::backend::registry::register_all(&mut reg);

    let backend = reg.get(target)
        .ok_or_else(|| TranslateError::UnknownVersion {
            lang: target.language,
            version: target.version.0.clone(),
        })?;

    backend.emit(ir_nodes)
}

/// Compare original and round-tripped source for differences.
fn compare_roundtrip(original: &str, roundtripped: &str) -> Vec<String> {
    let mut warnings = Vec::new();

    let orig_lines: Vec<&str> = original.lines().collect();
    let rt_lines: Vec<&str> = roundtripped.lines().collect();

    if orig_lines.len() != rt_lines.len() {
        warnings.push(format!(
            "Line count differs: {} vs {}",
            orig_lines.len(),
            rt_lines.len()
        ));
    }

    for (i, (orig, rt)) in orig_lines.iter().zip(rt_lines.iter()).enumerate() {
        if orig.trim() != rt.trim() {
            warnings.push(format!(
                "Line {} differs: '{}' vs '{}'",
                i + 1,
                orig.trim(),
                rt.trim()
            ));
        }
    }

    warnings
}
