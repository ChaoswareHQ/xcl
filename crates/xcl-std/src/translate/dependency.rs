//! Java dependency walking for the translator.
//!
//! Walks Java project dependencies (Maven pom.xml, Gradle build.gradle)
//! and uses them directly in the translated output instead of rewriting them.
//!
//! # Usage
//!
//! ```ignore
//! use xcl_std::translate::dependency::{walk_java_dependencies, DependencyInfo};
//!
//! let deps = walk_java_dependencies("/path/to/project");
//! for dep in deps {
//!     println!("{}: {}@{}", dep.group_id, dep.artifact_id, dep.version);
//! }
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use crate::encoding::xml;

/// A Java dependency from Maven/Gradle.
#[derive(Clone, Debug)]
pub struct DependencyInfo {
    /// Group ID (e.g., "com.fasterxml.jackson.core").
    pub group_id: String,
    /// Artifact ID (e.g., "jackson-databind").
    pub artifact_id: String,
    /// Version (e.g., "2.15.0").
    pub version: String,
    /// Scope (compile, test, provided, runtime).
    pub scope: String,
    /// Source build system (maven, gradle).
    pub build_system: String,
}

impl DependencyInfo {
    /// Generate Maven XML dependency snippet.
    pub fn to_maven_xml(&self) -> String {
        format!(
            "    <dependency>\n      <groupId>{}</groupId>\n      <artifactId>{}</artifactId>\n      <version>{}</version>\n      <scope>{}</scope>\n    </dependency>",
            self.group_id, self.artifact_id, self.version, self.scope
        )
    }

    /// Generate Gradle dependency string.
    pub fn to_gradle_string(&self) -> String {
        format!(
            "{}:{}:{}",
            self.group_id, self.artifact_id, self.version
        )
    }

    /// Generate import statement hint.
    pub fn import_hint(&self) -> String {
        // Convert group_id to package name hint
        let package = self.group_id.clone();
        format!("import {}.{}.*;", package, self.artifact_id)
    }
}

/// Walk a Java project directory and extract dependencies.
///
/// Searches for pom.xml (Maven) and build.gradle (Gradle) files
/// and parses the dependencies from them.
pub fn walk_java_dependencies(project_root: &str) -> Vec<DependencyInfo> {
    let path = Path::new(project_root);
    let mut dependencies = Vec::new();

    // Try Maven pom.xml
    let pom_path = path.join("pom.xml");
    if pom_path.exists() {
        if let Ok(content) = fs::read_to_string(&pom_path) {
            dependencies.extend(parse_maven_pom(&content));
        }
    }

    // Try Gradle build.gradle
    let gradle_path = path.join("build.gradle");
    if gradle_path.exists() {
        if let Ok(content) = fs::read_to_string(&gradle_path) {
            dependencies.extend(parse_gradle_build(&content));
        }
    }

    // Try Gradle Kotlin DSL
    let gradle_kts_path = path.join("build.gradle.kts");
    if gradle_kts_path.exists() {
        if let Ok(content) = fs::read_to_string(&gradle_kts_path) {
            dependencies.extend(parse_gradle_kts(&content));
        }
    }

    // Deduplicate
    dependencies.sort_by(|a, b| {
        a.group_id.cmp(&b.group_id)
            .then(a.artifact_id.cmp(&b.artifact_id))
    });
    dependencies.dedup_by(|a, b| {
        a.group_id == b.group_id && a.artifact_id == b.artifact_id
    });

    dependencies
}

/// Parse Maven pom.xml file.
///
/// Simple XML parser that extracts dependency blocks.
fn parse_maven_pom(content: &str) -> Vec<DependencyInfo> {
    let mut dependencies = Vec::new();

    // Simple state-based parser for Maven dependencies
    let mut in_dependencies = false;
    let mut in_dependency = false;
    let mut group_id = String::new();
    let mut artifact_id = String::new();
    let mut version = String::new();
    let mut scope = String::from("compile");

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "<dependencies>" {
            in_dependencies = true;
            continue;
        }
        if trimmed == "</dependencies>" {
            in_dependencies = false;
            continue;
        }

        if !in_dependencies {
            continue;
        }

        if trimmed.starts_with("<dependency>") {
            in_dependency = true;
            group_id.clear();
            artifact_id.clear();
            version.clear();
            scope = String::from("compile");
            continue;
        }
        if trimmed.starts_with("</dependency>") {
            in_dependency = false;
            dependencies.push(DependencyInfo {
                group_id: group_id.clone(),
                artifact_id: artifact_id.clone(),
                version: version.clone(),
                scope: scope.clone(),
                build_system: "maven".to_string(),
            });
            continue;
        }

        if in_dependency {
            if let Some(value) = xml::extract_value(trimmed, "groupId") {
                group_id = value;
            } else if let Some(value) = xml::extract_value(trimmed, "artifactId") {
                artifact_id = value;
            } else if let Some(value) = xml::extract_value(trimmed, "version") {
                version = value;
            } else if let Some(value) = xml::extract_value(trimmed, "scope") {
                scope = value;
            }
        }
    }

    dependencies
}

/// Extract value from a simple XML element (delegates to encoding::xml).
#[deprecated(since = "0.1.0", note = "Use crate::encoding::xml::extract_value instead")]
fn extract_xml_value(line: &str, tag: &str) -> Option<String> {
    xml::extract_value(line, tag)
}

/// Parse Gradle build.gradle file.
///
/// Extracts dependencies from the dependencies {} block.
fn parse_gradle_build(content: &str) -> Vec<DependencyInfo> {
    let mut dependencies = Vec::new();

    let mut in_dependencies_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("dependencies") {
            in_dependencies_block = true;
            continue;
        }
        if in_dependencies_block && trimmed == "}" {
            in_dependencies_block = false;
            continue;
        }

        if !in_dependencies_block {
            continue;
        }

        // Parse: implementation 'group:artifact:version'
        // Parse: compile 'group:artifact:version'
        // Parse: api "group:artifact:version"
        if let Some((scope, coords)) = parse_gradle_dependency_line(trimmed) {
            let parts: Vec<&str> = coords.split(':').collect();
            if parts.len() >= 3 {
                dependencies.push(DependencyInfo {
                    group_id: parts[0].to_string(),
                    artifact_id: parts[1].to_string(),
                    version: parts[2].to_string(),
                    scope,
                    build_system: "gradle".to_string(),
                });
            }
        }
    }

    dependencies
}

/// Parse a single Gradle dependency line.
fn parse_gradle_dependency_line(line: &str) -> Option<(String, String)> {
    // Match patterns like: implementation 'group:artifact:version'
    let scopes = ["implementation", "compile", "api", "runtimeOnly", "testImplementation", "compileOnly"];

    for scope in &scopes {
        if line.starts_with(scope) {
            let rest = &line[scope.len()..].trim();
            // Remove : after scope if present
            let rest = rest.strip_prefix(':').unwrap_or(rest);
            let rest = rest.trim();

            // Extract quoted string
            if let Some(start) = rest.find('\'').or_else(|| rest.find('"')) {
                let quote = &rest[start..start+1];
                if let Some(end) = rest[start+1..].find(quote) {
                    let coords = &rest[start+1..start+1+end];
                    return Some((scope.to_string(), coords.to_string()));
                }
            }
        }
    }

    None
}

/// Parse Gradle Kotlin DSL build.gradle.kts file.
fn parse_gradle_kts(content: &str) -> Vec<DependencyInfo> {
    // Similar to Gradle but with Kotlin DSL syntax
    parse_gradle_build(content)
}

/// Generate a pom.xml fragment with the given dependencies.
pub fn generate_pom_fragment(dependencies: &[DependencyInfo]) -> String {
    let mut xml = String::from("<dependencies>\n");
    for dep in dependencies {
        xml.push_str(&dep.to_maven_xml());
        xml.push('\n');
    }
    xml.push_str("</dependencies>");
    xml
}

/// Generate a Gradle dependencies block with the given dependencies.
pub fn generate_gradle_fragment(dependencies: &[DependencyInfo]) -> String {
    let mut block = String::from("dependencies {\n");
    for dep in dependencies {
        let scope = match dep.scope.as_str() {
            "compile" | "provided" => "implementation",
            "test" => "testImplementation",
            "runtime" => "runtimeOnly",
            other => other,
        };
        block.push_str(&format!(
            "    {} '{}'\n",
            scope,
            dep.to_gradle_string()
        ));
    }
    block.push_str("}");
    block
}

/// Find Java dependencies in a directory tree.
pub fn find_java_project_root(current_dir: &str) -> Option<PathBuf> {
    let mut current = Path::new(current_dir).to_path_buf();

    for _ in 0..10 {
        // Check up to 10 levels up
        if current.join("pom.xml").exists()
            || current.join("build.gradle").exists()
            || current.join("build.gradle.kts").exists()
        {
            return Some(current);
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_maven_pom() {
        let pom = r#"
<project>
    <dependencies>
        <dependency>
            <groupId>com.fasterxml.jackson.core</groupId>
            <artifactId>jackson-databind</artifactId>
            <version>2.15.0</version>
            <scope>compile</scope>
        </dependency>
    </dependencies>
</project>
"#;

        let deps = parse_maven_pom(pom);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].group_id, "com.fasterxml.jackson.core");
        assert_eq!(deps[0].artifact_id, "jackson-databind");
        assert_eq!(deps[0].version, "2.15.0");
    }

    #[test]
    fn test_parse_gradle_build() {
        let build = r#"
dependencies {
    implementation 'com.google.guava:guava:31.1-jre'
    testImplementation 'junit:junit:4.13.2'
}
"#;

        let deps = parse_gradle_build(build);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].group_id, "com.google.guava");
        assert_eq!(deps[1].scope, "testImplementation");
    }

    #[test]
    fn test_dependency_to_maven_xml() {
        let dep = DependencyInfo {
            group_id: "org.apache.commons".to_string(),
            artifact_id: "commons-lang3".to_string(),
            version: "3.12.0".to_string(),
            scope: "compile".to_string(),
            build_system: "maven".to_string(),
        };

        let xml = dep.to_maven_xml();
        assert!(xml.contains("org.apache.commons"));
        assert!(xml.contains("commons-lang3"));
        assert!(xml.contains("3.12.0"));
    }

    #[test]
    fn test_dependency_to_gradle_string() {
        let dep = DependencyInfo {
            group_id: "org.apache.commons".to_string(),
            artifact_id: "commons-lang3".to_string(),
            version: "3.12.0".to_string(),
            scope: "compile".to_string(),
            build_system: "gradle".to_string(),
        };

        assert_eq!(dep.to_gradle_string(), "org.apache.commons:commons-lang3:3.12.0");
    }

    #[test]
    fn test_extract_xml_value() {
        // Delegated to encoding::xml::extract_value
        assert_eq!(
            xml::extract_value("<groupId>test</groupId>", "groupId"),
            Some("test".to_string())
        );
        assert_eq!(
            xml::extract_value("<unknown>test</unknown>", "groupId"),
            None
        );
    }
}
