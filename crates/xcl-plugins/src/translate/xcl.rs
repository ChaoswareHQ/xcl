//! XCL translate domain — exposes the polyglot translator to XCL code.
//!
//! Allows XCL scripts to translate source code between languages and write output files.
//!
//! # Usage
//!
//! ```xcl
//! // Translate XCL source to Java
//! $java_code = translate.to "java" "25" $source
//!
//! // Translate and write to file
//! translate.file "input.xcl" "output.java" "java" "25"
//! ```

use std::collections::HashMap;

use xcl_macros::xcl_fn;
use xcl_vm::Value;

use super::{Translator, LangSpec, Language, project};
use crate::encoding::json;

fn val_str(s: &str) -> Value {
    Value::from_str_boxed(s.to_string())
}

fn val_map(entries: &[(&str, Value)]) -> Value {
    let map: HashMap<String, Value> = entries.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    Value::from_map_boxed(map)
}

/// Convert a Value to JSON string, falling back to string representation.
/// Reuses `encoding::json::to_json_string` for DRY compliance.
pub fn value_to_json_string(v: Value) -> String {
    json::to_json_string(v).unwrap_or_else(|| v.as_string().to_string())
}

/// Translate XCL source code to a target language.
///
/// # Arguments
/// - `lang` — Target language: "java", "rust", "python", "c", "cpp"
/// - `version` — Target version: "25" (Java), "2024" (Rust), "313" (Python), "99"/"11"/"17"/"23" (C)
/// - `source` — XCL source code string
///
/// # Returns
/// Translated source code string, or an error map if translation fails.
#[xcl_fn(name = "translate.to", args = "lang: str; version: str; source: str", ret = "str")]
fn translate_to(args: &[Value]) -> Value {
    let lang_str = args[0].as_str().unwrap_or("");
    let version_str = args[1].as_str().unwrap_or("");
    let source = args[2].as_str().unwrap_or("");

    let lang = match lang_str {
        "java" => Language::Java,
        "rust" => Language::Rust,
        "python" => Language::Python,
        "c" => Language::C,
        "cpp" => Language::Cpp,
        _ => {
            return val_map(&[
                ("error", val_str("unknown language")),
                ("lang", val_str(lang_str)),
            ]);
        }
    };

    let to_spec = LangSpec {
        language: lang,
        version: super::Version(version_str.to_string()),
    };

    let translator = Translator::new();
    match translator.translate(source, LangSpec::xcl("1"), to_spec) {
        Ok(code) => val_str(&code),
        Err(e) => val_map(&[("error", val_str(&format!("{:?}", e)))], ),
    }
}

/// Translate XCL source code and write the result to a file.
///
/// # Arguments
/// - `input` — Input XCL file path
/// - `output` — Output file path (e.g., "output.java", "output.c", "output.py")
/// - `lang` — Target language: "java", "rust", "python", "c", "cpp"
/// - `version` — Target version string
///
/// # Returns
/// A result map with "ok": true/false and "message" describing the outcome.
#[xcl_fn(name = "translate.file", args = "input: str; output: str; lang: str; version: str", ret = "map")]
fn translate_file(args: &[Value]) -> Value {
    let input_path = args[0].as_str().unwrap_or("");
    let output_path = args[1].as_str().unwrap_or("");
    let lang_str = args[2].as_str().unwrap_or("");
    let version_str = args[3].as_str().unwrap_or("");

    // Read input file
    let source = match std::fs::read_to_string(input_path) {
        Ok(content) => content,
        Err(e) => {
            return val_map(&[
                ("ok", Value::bool_(false)),
                ("message", val_str(&format!("Failed to read {}: {}", input_path, e))),
            ]);
        }
    };

    // Translate
    let translated = translate_to(&[
        val_str(lang_str),
        val_str(version_str),
        val_str(&source),
    ]);

    // Check for translation error
    if let Some(err_map) = translated.as_map() {
        if let Some(error_val) = err_map.get("error") {
            if let Some(error_msg) = error_val.as_str() {
                return val_map(&[
                    ("ok", Value::bool_(false)),
                    ("message", val_str(&format!("Translation error: {}", error_msg))),
                ]);
            }
        }
    }

    let code = translated.as_str().unwrap_or("");

    // Write output file
    match std::fs::write(output_path, code) {
        Ok(_) => val_map(&[
            ("ok", Value::bool_(true)),
            ("message", val_str(&format!("Written {} bytes to {}", code.len(), output_path))),
            ("output", val_str(output_path)),
        ]),
        Err(e) => val_map(&[
            ("ok", Value::bool_(false)),
            ("message", val_str(&format!("Failed to write {}: {}", output_path, e))),
        ]),
    }
}

/// Translate XCL source from one string to another language and return the result.
///
/// # Arguments
/// - `source` — XCL source code string
/// - `lang` — Target language
/// - `version` — Target version
///
/// # Returns
/// Translated code string or error map.
#[xcl_fn(name = "translate.string", args = "source: str; lang: str; version: str", ret = "str")]
fn translate_string(args: &[Value]) -> Value {
    let source = args[0].as_str().unwrap_or("");
    let lang_str = args[1].as_str().unwrap_or("");
    let version_str = args[2].as_str().unwrap_or("");

    translate_to(&[
        val_str(lang_str),
        val_str(version_str),
        val_str(source),
    ])
}

/// List all supported target languages and versions.
///
/// # Returns
/// Array of language specification strings.
#[xcl_fn(name = "translate.targets", args = "", ret = "array")]
fn translate_targets(_args: &[Value]) -> Value {
    let translator = Translator::new();
    let targets = translator.supported_targets();
    let mut result = Vec::with_capacity(targets.len());
    for spec in targets {
        let lang_name = match spec.language {
            Language::Xcl => "xcl",
            Language::Java => "java",
            Language::Rust => "rust",
            Language::Python => "python",
            Language::C => "c",
            Language::Cpp => "cpp",
        };
        result.push(val_str(&format!("{} {}", lang_name, spec.version.0)));
    }
    Value::from_array_boxed(result)
}

/// Translate an entire project directory from one language to another.
///
/// Walks the source directory, translates each source file, preserves the folder structure,
/// and generates a build manifest file (CMakeLists.txt for C, pom.xml for Java, etc.).
///
/// # Arguments
/// - `source_dir` — Path to the source project directory
/// - `output_dir` — Path to the output directory (will be created)
/// - `lang` — Target language: "java", "rust", "python", "c", "cpp"
/// - `version` — Target version string
///
/// # Returns
/// A result map with translation statistics.
#[xcl_fn(name = "translate.project", args = "source_dir: str; output_dir: str; lang: str; version: str", ret = "map")]
fn translate_project_fn(args: &[Value]) -> Value {
    let source_dir = args[0].as_str().unwrap_or("");
    let output_dir = args[1].as_str().unwrap_or("");
    let lang_str = args[2].as_str().unwrap_or("");
    let version_str = args[3].as_str().unwrap_or("");

    match project::translate_project(source_dir, output_dir, lang_str, version_str) {
        Ok(result) => {
            let mut entries: Vec<(&str, Value)> = vec![
                ("ok", Value::bool_(true)),
                ("files_translated", Value::from_i48(result.files_translated as i64)),
                ("files_failed", Value::from_i48(result.files_failed as i64)),
            ];

            // Add translated files list
            let files: Vec<Value> = result
                .translated_files
                .iter()
                .map(|f| val_str(&f.to_string_lossy()))
                .collect();
            entries.push(("translated_files", Value::from_array_boxed(files)));

            // Add errors list
            let errors: Vec<Value> = result
                .errors
                .iter()
                .map(|(path, msg)| {
                    val_map(&[
                        ("file", val_str(&path.to_string_lossy())),
                        ("error", val_str(msg)),
                    ])
                })
                .collect();
            entries.push(("errors", Value::from_array_boxed(errors)));

            // Add build manifest path
            if let Some(manifest) = &result.build_manifest {
                entries.push(("build_manifest", val_str(&manifest.to_string_lossy())));
            }

            val_map(&entries)
        }
        Err(e) => val_map(&[
            ("ok", Value::bool_(false)),
            ("message", val_str(&format!("{:?}", e))),
        ]),
    }
}
