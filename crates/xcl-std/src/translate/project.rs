//! Multi-file project translation.
//!
//! Walks a source directory tree, translates each source file to the target language,
//! and preserves the folder/file structure. Generates appropriate build manifest files
//! (e.g., CMakeLists.txt for C, pom.xml for Java, etc.).
//!
//! # Usage
//!
//! ```text
//! translate.project("src/main/java", "output/c-project", "c", "99")
//! ```
//!
//! This will:
//! 1. Walk `src/main/java` recursively
//! 2. Find all `.java` files
//! 3. Translate each file to `.c`
//! 4. Preserve the directory structure in `output/c-project`
//! 5. Generate `CMakeLists.txt` at the output root

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Translator, LangSpec, Language, TranslateError};

/// Mapping from source language to file extensions.
const SOURCE_EXTENSIONS: &[(&str, &str)] = &[
    ("java", ".java"),
    ("rust", ".rs"),
    ("python", ".py"),
    ("c", ".c"),
    ("cpp", ".cpp"),
    ("xcl", ".xcl"),
];

/// Mapping from target language to file extensions.
const TARGET_EXTENSIONS: &[(&str, &str)] = &[
    ("java", ".java"),
    ("rust", ".rs"),
    ("python", ".py"),
    ("c", ".c"),
    ("cpp", ".cpp"),
    ("xcl", ".xcl"),
];

/// Result of a project translation.
#[derive(Debug)]
pub struct ProjectTranslationResult {
    /// Number of files successfully translated.
    pub files_translated: usize,
    /// Number of files that failed to translate.
    pub files_failed: usize,
    /// Paths of translated output files.
    pub translated_files: Vec<PathBuf>,
    /// Errors encountered during translation.
    pub errors: Vec<(PathBuf, String)>,
    /// Path to the generated build manifest (if any).
    pub build_manifest: Option<PathBuf>,
}

/// Translate an entire project directory from one language to another.
///
/// # Arguments
/// - `source_dir` — Path to the source project directory
/// - `output_dir` — Path to the output directory (will be created if it doesn't exist)
/// - `target_lang` — Target language: "java", "rust", "python", "c", "cpp"
/// - `target_version` — Target version string
///
/// # Returns
/// A `ProjectTranslationResult` with statistics and file paths.
pub fn translate_project(
    source_dir: &str,
    output_dir: &str,
    target_lang: &str,
    target_version: &str,
) -> Result<ProjectTranslationResult, TranslateError> {
    let source_path = Path::new(source_dir);
    let output_path = Path::new(output_dir);

    // Validate source directory
    if !source_path.exists() {
        return Err(TranslateError::ParseError {
            lang: Language::Xcl,
            message: format!("Source directory does not exist: {}", source_dir),
            span: super::ir::node::SourceSpan::dummy(),
        });
    }

    // Determine source language from file extensions found
    let source_lang = detect_source_language(source_path)?;

    // Get file extensions
    let source_ext = source_extension(&source_lang);
    let target_ext = target_extension(target_lang);

    // Create output directory
    fs::create_dir_all(output_path).map_err(|e| TranslateError::ParseError {
        lang: Language::Xcl,
        message: format!("Failed to create output directory: {}", e),
        span: super::ir::node::SourceSpan::dummy(),
    })?;

    let translator = Translator::new();
    // Use default version for source language
    let default_source_version = match source_lang.as_str() {
        "java" => "25",
        "rust" => "2024",
        "python" => "313",
        "c" => "99",
        "cpp" => "17",
        _ => "1",
    };
    let from_spec = lang_spec_from_str(&source_lang, default_source_version)?;
    let to_spec = lang_spec_from_str(target_lang, target_version)?;

    let mut result = ProjectTranslationResult {
        files_translated: 0,
        files_failed: 0,
        translated_files: Vec::new(),
        errors: Vec::new(),
        build_manifest: None,
    };

    // Walk source directory and translate each file
    walk_and_translate(
        source_path,
        output_path,
        source_ext,
        target_ext,
        &translator,
        &from_spec,
        &to_spec,
        &mut result,
    )?;

    // Generate build manifest
    let manifest = generate_build_manifest(
        output_path,
        target_lang,
        &result.translated_files,
        source_dir,
    )?;
    result.build_manifest = manifest;

    Ok(result)
}

/// Detect the source language by scanning file extensions in the directory.
fn detect_source_language(dir: &Path) -> Result<String, TranslateError> {
    let mut extensions = HashMap::new();

    for entry in walkdir_recursive(dir) {
        let path = entry.into_path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let count = extensions.entry(format!(".{}", ext)).or_insert(0);
            *count += 1;
        }
    }

    // Find the most common extension and map it to a language
    if let Some((ext, _count)) = extensions.into_iter().max_by_key(|(_, count)| *count) {
        for (lang, lang_ext) in SOURCE_EXTENSIONS {
            if ext == *lang_ext {
                return Ok(lang.to_string());
            }
        }
        // Default to the extension found
        return Ok(ext.trim_start_matches('.').to_string());
    }

    Err(TranslateError::ParseError {
        lang: Language::Xcl,
        message: "Could not detect source language: no source files found".to_string(),
        span: super::ir::node::SourceSpan::dummy(),
    })
}

/// Get the file extension for a source language.
fn source_extension(lang: &str) -> &'static str {
    for (l, ext) in SOURCE_EXTENSIONS {
        if *l == lang {
            return ext;
        }
    }
    ".xcl"
}

/// Get the file extension for a target language.
fn target_extension(lang: &str) -> &'static str {
    for (l, ext) in TARGET_EXTENSIONS {
        if *l == lang {
            return ext;
        }
    }
    ".c"
}

/// Create a LangSpec from language and version strings.
fn lang_spec_from_str(lang: &str, version: &str) -> Result<LangSpec, TranslateError> {
    let language = match lang {
        "java" => Language::Java,
        "rust" => Language::Rust,
        "python" => Language::Python,
        "c" => Language::C,
        "cpp" => Language::Cpp,
        "xcl" => Language::Xcl,
        _ => {
            return Err(TranslateError::UnknownVersion {
                lang: Language::Xcl,
                version: lang.to_string(),
            })
        }
    };

    Ok(LangSpec {
        language,
        version: super::Version(version.to_string()),
    })
}

/// Walk the source directory and translate each matching file.
fn walk_and_translate(
    source_dir: &Path,
    output_dir: &Path,
    source_ext: &str,
    target_ext: &str,
    translator: &Translator,
    from_spec: &LangSpec,
    to_spec: &LangSpec,
    result: &mut ProjectTranslationResult,
) -> Result<(), TranslateError> {
    for entry in walkdir_recursive(source_dir) {
        let source_path = entry.into_path();

        // Skip non-source files
        let Some(ext) = source_path.extension().and_then(|e| e.to_str()) else {
            continue;
        };

        if format!(".{}", ext) != source_ext {
            continue;
        }

        // Compute relative path and output path
        let relative = source_path
            .strip_prefix(source_dir)
            .unwrap_or(&source_path);

        // Change extension to target
        let mut output_path = output_dir.to_path_buf();
        output_path.push(relative.parent().unwrap_or(Path::new("")));
        let file_stem = source_path.file_stem().unwrap_or_default().to_str().unwrap_or("");
        output_path.push(format!("{}{}", file_stem, target_ext));

        // Create output parent directory
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        // Read source file
        let source = match fs::read_to_string(&source_path) {
            Ok(content) => content,
            Err(e) => {
                result.errors.push((source_path.clone(), format!("Failed to read: {}", e)));
                result.files_failed += 1;
                continue;
            }
        };

        // Translate the source using the detected source language
        match translator.translate(&source, from_spec.clone(), to_spec.clone()) {
            Ok(translated) => {
                match fs::write(&output_path, &translated) {
                    Ok(_) => {
                        result.translated_files.push(output_path.clone());
                        result.files_translated += 1;
                    }
                    Err(e) => {
                        result.errors.push((source_path.clone(), format!("Failed to write: {}", e)));
                        result.files_failed += 1;
                    }
                }
            }
            Err(e) => {
                result.errors.push((source_path.clone(), format!("{:?}", e)));
                result.files_failed += 1;
            }
        }
    }

    Ok(())
}

/// Simple recursive directory walker (avoids external dependency).
fn walkdir_recursive(dir: &Path) -> Vec<dir_entry::DirEntry> {
    let mut entries = Vec::new();
    walkdir_impl(dir, &mut entries);
    entries
}

mod dir_entry {
    use std::path::PathBuf;

    /// Minimal directory entry for our walker.
    #[derive(Debug)]
    pub struct DirEntry {
        pub path: PathBuf,
    }

    impl DirEntry {
        pub fn into_path(self) -> PathBuf {
            self.path
        }
    }
}

fn walkdir_impl(dir: &Path, entries: &mut Vec<dir_entry::DirEntry>) {
    if let Ok(read_dir) = fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            entries.push(dir_entry::DirEntry { path: path.clone() });
            if path.is_dir() {
                walkdir_impl(&path, entries);
            }
        }
    }
}

/// Generate a build manifest file for the target language.
fn generate_build_manifest(
    output_dir: &Path,
    target_lang: &str,
    translated_files: &[PathBuf],
    source_dir: &str,
) -> Result<Option<PathBuf>, TranslateError> {
    match target_lang {
        "c" | "cpp" => generate_cmake(output_dir, translated_files),
        "java" => generate_pom_xml(output_dir, source_dir),
        "python" => generate_requirements(output_dir, source_dir),
        "rust" => generate_cargo_toml(output_dir, source_dir),
        _ => Ok(None),
    }
}

/// Generate CMakeLists.txt for C/C++ projects.
fn generate_cmake(
    output_dir: &Path,
    translated_files: &[PathBuf],
) -> Result<Option<PathBuf>, TranslateError> {
    let cmake_path = output_dir.join("CMakeLists.txt");
    let mut cmake = String::new();

    cmake.push_str("cmake_minimum_required(VERSION 3.15)\n");

    // Extract project name from source directory
    let project_name = output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    cmake.push_str(&format!("project({})\n\n", project_name));

    // Collect source files
    let mut sources = Vec::new();
    for file in translated_files {
        if let Ok(rel) = file.strip_prefix(output_dir) {
            sources.push(rel.to_string_lossy().to_string());
        }
    }
    sources.sort();

    if sources.is_empty() {
        cmake.push_str("# No source files found\n");
    } else {
        cmake.push_str("add_executable(");
        cmake.push_str(project_name);
        cmake.push(' ');
        for (i, src) in sources.iter().enumerate() {
            if i > 0 {
                cmake.push(' ');
            }
            cmake.push_str(src);
        }
        cmake.push_str(")\n");
    }

    fs::write(&cmake_path, cmake).map_err(|e| TranslateError::ParseError {
        lang: Language::C,
        message: format!("Failed to write CMakeLists.txt: {}", e),
        span: super::ir::node::SourceSpan::dummy(),
    })?;

    Ok(Some(cmake_path))
}

/// Generate pom.xml for Java projects.
fn generate_pom_xml(
    output_dir: &Path,
    source_dir: &str,
) -> Result<Option<PathBuf>, TranslateError> {
    let pom_path = output_dir.join("pom.xml");
    let project_name = output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let pom = format!(
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0
         http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.translated</groupId>
  <artifactId>{}</artifactId>
  <version>1.0-SNAPSHOT</version>
  <name>{}</name>
  <!-- Translated from {} -->
</project>
"#,
        project_name, project_name, source_dir
    );

    fs::write(&pom_path, pom).map_err(|e| TranslateError::ParseError {
        lang: Language::Java,
        message: format!("Failed to write pom.xml: {}", e),
        span: super::ir::node::SourceSpan::dummy(),
    })?;

    Ok(Some(pom_path))
}

/// Generate requirements.txt for Python projects.
fn generate_requirements(
    output_dir: &Path,
    source_dir: &str,
) -> Result<Option<PathBuf>, TranslateError> {
    let req_path = output_dir.join("requirements.txt");
    let req = format!(
        "# Translated from {}\n# No external dependencies\n",
        source_dir
    );

    fs::write(&req_path, req).map_err(|e| TranslateError::ParseError {
        lang: Language::Python,
        message: format!("Failed to write requirements.txt: {}", e),
        span: super::ir::node::SourceSpan::dummy(),
    })?;

    Ok(Some(req_path))
}

/// Generate Cargo.toml for Rust projects.
fn generate_cargo_toml(
    output_dir: &Path,
    source_dir: &str,
) -> Result<Option<PathBuf>, TranslateError> {
    let cargo_path = output_dir.join("Cargo.toml");
    let project_name = output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let cargo = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

# Translated from {}

[dependencies]
"#,
        project_name, source_dir
    );

    fs::write(&cargo_path, cargo).map_err(|e| TranslateError::ParseError {
        lang: Language::Rust,
        message: format!("Failed to write Cargo.toml: {}", e),
        span: super::ir::node::SourceSpan::dummy(),
    })?;

    Ok(Some(cargo_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_project(dir: &Path, files: &[(&str, &str)]) -> Result<(), std::io::Error> {
        for (path, content) in files {
            let full_path = dir.join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::File::create(&full_path)?;
            file.write_all(content.as_bytes())?;
        }
        Ok(())
    }

    #[test]
    fn test_detect_source_language_java() {
        let temp = std::env::temp_dir().join("xcl_test_detect_java");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        create_temp_project(
            &temp,
            &[
                ("src/Main.java", "public class Main {}"),
                ("src/Util.java", "class Util {}"),
                ("README.md", "# Project"),
            ],
        )
        .unwrap();

        let lang = detect_source_language(&temp).unwrap();
        assert_eq!(lang, "java");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_detect_source_language_python() {
        let temp = std::env::temp_dir().join("xcl_test_detect_python");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        create_temp_project(
            &temp,
            &[
                ("main.py", "print('hello')"),
                ("utils.py", "def util(): pass"),
            ],
        )
        .unwrap();

        let lang = detect_source_language(&temp).unwrap();
        assert_eq!(lang, "python");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_translate_simple_project() {
        let source = std::env::temp_dir().join("xcl_test_source");
        let output = std::env::temp_dir().join("xcl_test_output");
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&source).unwrap();

        create_temp_project(
            &source,
            &[
                ("src/main/java/com/App.java", "let x = 42\nlet y = 100"),
                ("src/main/java/com/Util.java", "let name = \"hello\""),
            ],
        )
        .unwrap();

        let result = translate_project(
            source.to_str().unwrap(),
            output.to_str().unwrap(),
            "c",
            "99",
        )
        .unwrap();

        assert!(
            result.files_translated >= 2,
            "Should translate at least 2 files"
        );
        assert!(
            result.build_manifest.is_some(),
            "Should generate CMakeLists.txt"
        );

        // Verify CMakeLists.txt exists
        let cmake = output.join("CMakeLists.txt");
        assert!(cmake.exists(), "CMakeLists.txt should exist");

        // Verify translated files exist
        for file in &result.translated_files {
            assert!(file.exists(), "Translated file should exist: {:?}", file);
        }

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
    }

    #[test]
    fn test_preserves_directory_structure() {
        let source = std::env::temp_dir().join("xcl_test_structure");
        let output = std::env::temp_dir().join("xcl_test_structure_out");
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&source).unwrap();

        create_temp_project(
            &source,
            &[
                ("src/main/java/com/App.java", "let x = 1"),
                ("src/main/java/com/model/User.java", "let name = \"test\""),
                ("src/test/java/com/AppTest.java", "let ok = true"),
            ],
        )
        .unwrap();

        let result = translate_project(
            source.to_str().unwrap(),
            output.to_str().unwrap(),
            "c",
            "99",
        )
        .unwrap();

        // Check that directory structure is preserved
        let app_c = output.join("src/main/java/com/App.c");
        assert!(app_c.exists(), "App.c should exist at preserved path");

        let user_c = output.join("src/main/java/com/model/User.c");
        assert!(user_c.exists(), "User.c should exist at preserved path");

        let app_test_c = output.join("src/test/java/com/AppTest.c");
        assert!(app_test_c.exists(), "AppTest.c should exist at preserved path");

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
    }

    #[test]
    fn test_generate_cmake() {
        let temp = std::env::temp_dir().join("xcl_test_cmake");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let files = vec![
            temp.join("src/App.c"),
            temp.join("src/Util.c"),
        ];

        let result = generate_cmake(&temp, &files).unwrap();
        assert!(result.is_some());
        let cmake_path = result.unwrap();
        assert!(cmake_path.exists());

        let content = fs::read_to_string(&cmake_path).unwrap();
        assert!(content.contains("cmake_minimum_required"));
        assert!(content.contains("add_executable"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_translate_to_java() {
        let source = std::env::temp_dir().join("xcl_test_java_src");
        let output = std::env::temp_dir().join("xcl_test_java_out");
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&source).unwrap();

        create_temp_project(
            &source,
            &[("src/Main.xcl", "let x = 42")],
        )
        .unwrap();

        let result = translate_project(
            source.to_str().unwrap(),
            output.to_str().unwrap(),
            "java",
            "25",
        )
        .unwrap();

        assert!(result.files_translated >= 1);
        assert!(result.build_manifest.is_some());

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
    }

    #[test]
    fn test_translate_to_python() {
        let source = std::env::temp_dir().join("xcl_test_py_src");
        let output = std::env::temp_dir().join("xcl_test_py_out");
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&source).unwrap();

        create_temp_project(
            &source,
            &[("src/main.xcl", "let x = 42")],
        )
        .unwrap();

        let result = translate_project(
            source.to_str().unwrap(),
            output.to_str().unwrap(),
            "python",
            "313",
        )
        .unwrap();

        assert!(result.files_translated >= 1);
        assert!(result.build_manifest.is_some());

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&output);
    }
}
