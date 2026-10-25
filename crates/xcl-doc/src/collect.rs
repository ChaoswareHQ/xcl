use crate::model::*;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use xcl_rt::{IMPL_REGISTRY, NATIVE_REGISTRY, TRAIT_REGISTRY, TYPE_REGISTRY};
use xcl_toolchain::TypeKind;

/// Parse an args string like "path: str; mode: str" into ParamDoc structs.
fn parse_params(args: &str) -> Vec<ParamDoc> {
    args.split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let mut parts = pair.splitn(2, ':');
            let name = parts.next()?.trim().to_string();
            let ty = parts.next()?.trim().to_string();
            Some(ParamDoc { name, ty })
        })
        .collect()
}

/// Parsed documentation from a single Rust doc comment block.
#[derive(Debug, Clone, Default)]
struct ParsedDoc {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub examples: Vec<String>,
    pub notes: Vec<String>,
}

/// A primitive type entry: (name, summary, description, examples, notes)
type PrimitiveEntry = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

/// Extract doc comments from the engine's standard-library sources.
///
/// Walks `crates/xcl-rt/src/stdlib`, reads each `.rs` file, and extracts
/// `///` doc comments that immediately precede `#[xcl_fn]` or `#[xcl_type]` attributes.
/// Returns a map from the function/type name to the full parsed documentation.
fn extract_rust_doc_comments() -> HashMap<String, ParsedDoc> {
    let mut comments = HashMap::new();

    // Path to the standard-library sources.
    let std_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new("."))
        .join("crates")
        .join("xcl-rt")
        .join("src")
        .join("stdlib");

    if !std_src.is_dir() {
        return comments;
    }

    collect_rust_docs_recursive(&std_src, &mut comments);
    comments
}

fn collect_rust_docs_recursive(dir: &Path, comments: &mut HashMap<String, ParsedDoc>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        if path.is_dir() {
            collect_rust_docs_recursive(&path, comments);
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        extract_docs_from_file(&source, comments);
    }
}

/// Parse a block of doc comment lines into structured sections.
///
/// Recognizes these section headers (case-insensitive):
///   ## SYNTAX
///   ## DESCRIPTION
///   ## PARAMETERS
///   ## EXAMPLES
///   ## INPUT TYPES
///   ## OUTPUT TYPES
///   ## NOTES
///
/// Text before any section header becomes the summary (first line) and description.
fn parse_doc_block(doc_lines: &[String]) -> ParsedDoc {
    let mut doc = ParsedDoc::default();
    let mut current_section: Option<String> = None;
    let mut text_buf: Vec<String> = Vec::new();
    let mut example_buf: String = String::new();

    let flush_text = |doc: &mut ParsedDoc, section: &Option<String>, buf: &mut Vec<String>| {
        let text = buf
            .iter()
            .filter(|l| !l.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return;
        }
        match section.as_deref() {
            None => {
                // Before any section: first line = summary, rest = description
                if doc.summary.is_none() {
                    if let Some(first) = buf.first() {
                        doc.summary = Some(first.clone());
                    }
                    if buf.len() > 1 {
                        doc.description = Some(buf[1..].join("\n"));
                    }
                }
            }
            Some("DESCRIPTION") => {
                doc.description = Some(text);
            }
            Some("NOTES") => {
                doc.notes.push(text);
            }
            _ => {}
        }
        buf.clear();
    };

    let flush_example = |doc: &mut ParsedDoc, buf: &mut String| {
        // Strip the leading backtick flag character used to mark code block state
        let text = buf
            .strip_prefix('`')
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| buf.trim().to_string());
        if !text.is_empty() {
            doc.examples.push(text);
        }
        buf.clear();
    };

    for line in doc_lines {
        // Check for section headers: ## SECTION_NAME
        if let Some(header) = line.strip_prefix("## ") {
            // Flush previous section
            if example_buf.starts_with('`') {
                flush_example(&mut doc, &mut example_buf);
            } else {
                flush_text(&mut doc, &current_section, &mut text_buf);
            }

            let header_upper = header.trim().to_uppercase();
            current_section = Some(header_upper);
            continue;
        }

        // Code block delimiter (``` with optional language tag like ```xcl)
        if line.starts_with("```") {
            // Flush current buffer before toggling
            if example_buf.starts_with('`') {
                // Closing a code block — flush it as an example
                flush_example(&mut doc, &mut example_buf);
                example_buf.clear();
            } else {
                // Opening a code block — flush any pending text first
                flush_text(&mut doc, &current_section, &mut text_buf);
                // Mark that we're inside a code block (use backtick as flag, don't store it)
                example_buf.push('`');
            }
            continue;
        }

        // Accumulate into the appropriate buffer
        if example_buf.starts_with('`') {
            // Inside a code block — collect lines for the example
            if example_buf.len() > 1 && !example_buf.ends_with('\n') {
                example_buf.push('\n');
            }
            example_buf.push_str(line);
        } else {
            text_buf.push(line.clone());
        }
    }

    // Flush remaining buffers
    if !example_buf.is_empty() {
        flush_example(&mut doc, &mut example_buf);
    }
    flush_text(&mut doc, &current_section, &mut text_buf);

    doc
}

/// Extract doc comments from a single Rust source file.
///
/// Looks for patterns like:
/// ```
/// /// Summary line.
/// /// More details...
/// #[xcl_fn(name = "foo", ...)]
/// fn foo_fn(...) { ... }
/// ```
fn extract_docs_from_file(source: &str, comments: &mut HashMap<String, ParsedDoc>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Look for /// doc comments
        if line.starts_with("///") {
            let mut doc_lines: Vec<String> = Vec::new();

            // Collect consecutive doc comment lines
            while i < lines.len() {
                let l = lines[i].trim();
                if let Some(stripped) = l.strip_prefix("///") {
                    doc_lines.push(stripped.trim().to_string());
                    i += 1;
                } else {
                    break;
                }
            }

            // Skip blank lines between doc comment and attribute
            while i < lines.len() && lines[i].trim().is_empty() {
                i += 1;
            }

            // Check if the next non-blank line is an xcl_fn or xcl_type attribute
            if i < lines.len() {
                let attr_line = lines[i].trim();

                if attr_line.starts_with("#[xcl_fn") {
                    // Collect the full multi-line attribute
                    let full_attr = collect_attribute_lines(&lines, &mut i);
                    // Extract the name from #[xcl_fn(name = "io.foo", ...)]
                    // If no name attribute, derive from the function name on the next line
                    let full_name = extract_attr_name(&full_attr, "name").or_else(|| {
                        // Find the function name on the next line
                        if i < lines.len() {
                            let fn_line = lines[i].trim();
                            if let Some(stripped) = fn_line.strip_prefix("fn ") {
                                let fn_name = stripped.split_whitespace().next()?;
                                // Convert snake_case to domain.method
                                if let Some(underscore_pos) = fn_name.find('_') {
                                    let method = &fn_name[underscore_pos + 1..];
                                    Some(method.to_string())
                                } else {
                                    Some(fn_name.to_string())
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                    if let Some(method_name) = full_name {
                        let parsed = parse_doc_block(&doc_lines);
                        // If full_name is dotted (e.g., "io.foo"), extract just the method part
                        let method_only = if let Some(dot_pos) = method_name.find('.') {
                            method_name[dot_pos + 1..].to_string()
                        } else {
                            method_name
                        };
                        comments.insert(method_only, parsed);
                    }
                } else if attr_line.starts_with("#[xcl_type") {
                    // Collect the full multi-line attribute
                    let full_attr = collect_attribute_lines(&lines, &mut i);
                    // Extract the name from #[xcl_type(name = "File", ...)]
                    if let Some(name) = extract_attr_name(&full_attr, "name") {
                        let parsed = parse_doc_block(&doc_lines);
                        // For types, use a special key
                        comments.insert(format!("type:{}", name), parsed);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Collect all lines of a multi-line attribute, advancing `i` past the closing `)]`.
fn collect_attribute_lines(lines: &[&str], i: &mut usize) -> String {
    let mut result = String::new();
    let start = *i;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;

    while *i < lines.len() {
        let line = lines[*i];
        result.push_str(line);
        result.push('\n');

        for c in line.chars() {
            match c {
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
        }

        // Check if we've closed the attribute: `)]` at the end
        let trimmed = line.trim_end();
        if trimmed.ends_with(")]") && paren_depth == 0 && bracket_depth == 0 {
            *i += 1;
            return result;
        }

        *i += 1;
    }

    // Fallback: return what we collected
    let _ = start;
    result
}

/// Extract a named parameter value from an attribute string.
/// e.g., `#[xcl_fn(name = "foo", domain = "io")]` -> Some("foo") for key "name"
fn extract_attr_name(attr: &str, key: &str) -> Option<String> {
    // Search for `key = "value"` pattern
    let search_key = format!("{} = \"", key);
    if let Some(pos) = attr.find(&search_key) {
        let start = pos + search_key.len();
        if let Some(end) = attr[start..].find('"') {
            return Some(attr[start..start + end].to_string());
        }
    }

    None
}

pub fn collect_std() -> DocModel {
    // Force-link the engine so its linkme entries are not stripped by the linker.
    // Without this, NATIVE_REGISTRY and TYPE_REGISTRY will be empty.
    let _ = xcl_rt::new_vm();

    // Extract doc comments from Rust source files
    let rust_docs = extract_rust_doc_comments();

    let mut domains: Vec<Domain> = Vec::new();
    let mut natives: Vec<Native> = Vec::new();
    let mut types: Vec<TypeDoc> = Vec::new();
    let mut impls: Vec<ImplDoc> = Vec::new();

    // ── Domains: derived from natives ──
    let mut domain_names: Vec<&str> = Vec::new();
    for n in NATIVE_REGISTRY.iter() {
        if let Some(d) = n.domain {
            domain_names.push(d);
        }
    }
    domain_names.sort();
    domain_names.dedup();
    for name in domain_names {
        domains.push(Domain {
            name: name.to_string(),
            summary: None,
        });
    }

    // ── Natives ──
    for n in NATIVE_REGISTRY.iter() {
        let full_name = match n.domain {
            Some(d) => format!("{}.{}", d, n.method),
            None => n.method.to_string(),
        };
        let args_str = n.args.map(|s| s.to_string());
        let params = args_str.as_deref().map(parse_params).unwrap_or_default();

        // Prefer source doc comment, fall back to registry summary
        // Doc comments are keyed by the method name (e.g., "open" for "io.open")
        let parsed = rust_docs.get(n.method);
        let summary = parsed
            .and_then(|p| p.summary.clone())
            .or_else(|| n.summary.map(|s| s.to_string()));
        let description = parsed.and_then(|p| p.description.clone());
        let examples = parsed.map(|p| p.examples.clone()).unwrap_or_default();
        let notes = parsed.map(|p| p.notes.clone()).unwrap_or_default();

        natives.push(Native {
            full_name,
            domain: n.domain.map(|s| s.to_string()),
            method: n.method.to_string(),
            args: args_str,
            ret: n.ret.map(|s| s.to_string()),
            params,
            named: n
                .named
                .iter()
                .map(|p| NamedDoc {
                    name: p.name.to_string(),
                    ty: p.ty.to_string(),
                    default: p.default.to_string(),
                })
                .collect(),
            summary,
            arg_desc: n
                .arg_desc
                .iter()
                .map(|(name, desc)| (name.to_string(), desc.to_string()))
                .collect(),
            description,
            examples,
            notes,
        });
    }
    natives.sort_by(|a, b| a.full_name.cmp(&b.full_name));

    // ── Types ──
    for t in TYPE_REGISTRY.iter() {
        // Prefer source doc comment, fall back to registry summary
        let type_key = format!("type:{}", t.name);
        let parsed = rust_docs.get(&type_key);
        let summary = parsed
            .and_then(|p| p.summary.clone())
            .or_else(|| t.summary.map(|s| s.to_string()));

        let description = parsed.and_then(|p| p.description.clone());
        let examples = parsed.map(|p| p.examples.clone()).unwrap_or_default();
        let notes = parsed.map(|p| p.notes.clone()).unwrap_or_default();

        types.push(TypeDoc {
            name: t.name.to_string(),
            domain: t.domain.map(|s| s.to_string()),
            kind: kind_name(t.kind).to_string(),
            summary,
            description,
            examples,
            notes,
            fields: t
                .fields
                .iter()
                .map(|f| FieldDoc {
                    name: f.name.to_string(),
                    ty: f.ty.to_string(),
                    optional: f.optional,
                })
                .collect(),
            variants: t
                .variants
                .iter()
                .map(|v| VariantDoc {
                    name: v.name.to_string(),
                    fields: v
                        .fields
                        .iter()
                        .map(|f| FieldDoc {
                            name: f.name.to_string(),
                            ty: f.ty.to_string(),
                            optional: f.optional,
                        })
                        .collect(),
                })
                .collect(),
        });
    }
    types.sort_by(|a, b| a.name.cmp(&b.name));

    // ── Traits ──
    let mut traits: Vec<TraitDoc> = Vec::new();
    for t in TRAIT_REGISTRY.iter() {
        let trait_key = format!("trait:{}", t.name);
        let parsed = rust_docs.get(&trait_key);
        let summary = parsed
            .and_then(|p| p.summary.clone())
            .or_else(|| t.summary.map(|s| s.to_string()));
        traits.push(TraitDoc {
            name: t.name.to_string(),
            domain: t.domain.map(|s| s.to_string()),
            summary,
            methods: t
                .methods
                .iter()
                .map(|m| TraitMethodDoc {
                    name: m.name.to_string(),
                    args: m.args.map(|s| s.to_string()),
                    ret: m.ret.map(|s| s.to_string()),
                    summary: m.summary.map(|s| s.to_string()),
                })
                .collect(),
        });
    }
    traits.sort_by(|a, b| a.name.cmp(&b.name));

    // ── Impls ──
    for i in IMPL_REGISTRY.iter() {
        impls.push(ImplDoc {
            type_name: i.type_name.to_string(),
            trait_name: i.trait_name.to_string(),
            domain: i.domain.map(|s| s.to_string()),
            methods: i
                .methods
                .iter()
                .map(|m| ImplMethodDoc {
                    name: m.name.to_string(),
                    args: m.args.map(|s| s.to_string()),
                    ret: m.ret.map(|s| s.to_string()),
                    summary: m.summary.map(|s| s.to_string()),
                })
                .collect(),
        });
    }
    impls.sort_by(|a, b| a.type_name.cmp(&b.type_name));

    // ── Primitive types ──
    // These are built-in types that don't come from the registry
    let primitives: Vec<PrimitiveEntry> = vec![
        (
            "str",
            "A UTF-8 encoded string.",
            "Strings in XCL are immutable sequences of UTF-8 characters. They are the primary text type, supporting concatenation with `+`, interpolation with `${...}`, and comparison operators. Strings are stored as UTF-8 byte sequences and support Unicode characters.\n\nStrings are created with double-quoted literals: `\"hello world\"`. They can be concatenated using the `+` operator or formatted using string interpolation.\n\nString operations include length checking, comparison, and conversion to/from other types.",
            &[
                "$s = \"Hello\" + \", World!\"",
                "$name = \"Alice\"",
                "$greeting = \"Hello, ${$name}!\"",
                "if len $s > 0 { print $s }",
            ],
            &[
                "Strings are immutable — concatenation creates a new string.",
                "Use `io.to_string` to convert other types to strings.",
            ],
        ),
        (
            "i64",
            "A 64-bit signed integer.",
            "The default integer type in XCL. Represents whole numbers in the range of -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807.\n\nInteger literals are written without a decimal point: `42`, `-17`, `0`. Arithmetic operations include `+`, `-`, `*`, `/`, and `%` (modulo). Comparison operators `==`, `!=`, `<`, `>`, `<=`, `>=` work as expected.\n\nUsed for counting, indexing, arithmetic, and as return codes in Result values.",
            &[
                "$x = 42",
                "$sum = 1 + 2 + 3",
                "$result = 10 / 3    # 3",
                "$remainder = 10 % 3 # 1",
                "if $x > 0 { print \"positive\" }",
            ],
            &[
                "Integer division truncates toward zero.",
                "Use `io.input --int` to read integers from input.",
            ],
        ),
        (
            "f64",
            "A 64-bit floating-point number.",
            "The default floating-point type in XCL, following IEEE 754 double precision. Represents real numbers with fractional parts.\n\nFloating-point literals include a decimal point: `3.14`, `-0.5`, `1.0`. Scientific notation is also supported: `1.5e10`, `2.5e-3`.\n\nUsed for mathematical calculations, measurements, and any computation requiring fractional precision.",
            &[
                "$pi = 3.14159",
                "$area = $pi * 5.0 * 5.0",
                "$ratio = 1.0 / 3.0    # 0.3333333333333333",
                "$scientific = 1.5e10  # 15000000000.0",
            ],
            &[
                "Floating-point arithmetic may have precision errors.",
                "Use `io.input --float` to read floats from input.",
            ],
        ),
        (
            "bool",
            "A boolean value: `true` or `false`.",
            "Represents logical truth values. Booleans are the result of comparison operations and are used in conditional expressions (`if`, `while`).\n\nThe two values are `true` and `false`. Logical operations include `and`, `or`, and `not`. Comparison operators (`==`, `!=`, `<`, `>`, `<=`, `>=`) return booleans.\n\nUsed for control flow, flags, and the result of type tests (`$x is Type`).",
            &[
                "$enabled = true",
                "$ready = false",
                "if $enabled and $ready { print \"go\" }",
                "if not $ready { print \"waiting\" }",
                "$big = ($x > 100)",
            ],
            &[
                "Use `io.input --bool` to read booleans from input.",
                "Booleans can be compared with `==` and `!=`.",
            ],
        ),
        (
            "arr",
            "A dynamic array of elements.",
            "Arrays are ordered, mutable collections that can grow and shrink at runtime. Elements are accessed by index starting from 0.\n\nArrays are created with square bracket syntax: `[1; 2; 3]`. Typed arrays use the `arr<Type>` syntax for type annotations. Elements can be any type, including other arrays for nested structures.\n\nCommon operations include indexing (`$arr[0]`), length (`len $arr`), concatenation (`+`), and iteration with `for` loops.",
            &[
                "$nums = [1; 2; 3]",
                "$first = $nums[0]",
                "$combined = [1; 2] + [3; 4]",
                "for $item in $nums { print $item }",
                "$empty: arr<i64> = []",
            ],
            &[
                "Arrays are zero-indexed.",
                "Use `len $arr` to get the number of elements.",
                "Arrays can be heterogeneous when typed as `arr<any>`.",
            ],
        ),
        (
            "dict",
            "A key-value dictionary (hash map).",
            "Dictionaries store key-value pairs where each key is unique. They provide fast lookup by key and are created using curly brace syntax.\n\nKeys can be strings or other hashable types. Values can be any type. Access values using bracket notation: `$dict[\"key\"]`.\n\nUsed for configuration data, records with dynamic fields, and any situation requiring named associations.",
            &[
                "$config = { host: \"localhost\"; port: 8080 }",
                "$host = $config[\"host\"]",
                "$user = { name: \"Alice\"; age: 30 }",
            ],
            &[
                "Keys must be unique — duplicate keys overwrite previous values.",
                "Use `len $dict` to get the number of entries.",
            ],
        ),
        (
            "fn",
            "A function type.",
            "Represents callable functions as first-class values. Functions can be passed as arguments, stored in variables, and returned from other functions.\n\nFunction types are written as `fn(arg_type) -> return_type`. For example, `fn(i64) -> str` describes a function that takes an integer and returns a string.\n\nUsed for callbacks, event handlers, and higher-order functions like `map`, `filter`, and `sort`.",
            &[
                "$greet = ($name) => \"Hello, \" + $name",
                "print $greet \"Alice\"",
                "# Function as a parameter",
                "apply($f: fn(i64) -> i64; $x: i64) -> i64 { $f $x }",
            ],
            &[
                "Functions are closures — they capture their environment.",
                "Use `fn` type annotations for function parameters.",
            ],
        ),
        (
            "none",
            "The unit type representing no value.",
            "The `none` type represents the absence of a meaningful return value. Functions that perform side effects (like printing or writing to files) typically return `none`.\n\nThere is only one value of type `none`, written as `none`. It is similar to `void` in other languages but is a proper type that can be used in type annotations.\n\nUsed as the return type for functions that don't produce a result.",
            &[
                "# A function returning none",
                "greet($name: str) -> none { print \"Hello, \" + $name }",
                "$result = greet \"Alice\"  # $result is none",
            ],
            &[
                "`none` is different from `null` — it's a real value, not an absence.",
                "Functions without an explicit return type default to `none`.",
            ],
        ),
        (
            "any",
            "A dynamic type that can hold any value.",
            "The `any` type is the top type in XCL's type system — every other type is a subtype of `any`. It allows storing values of unknown or mixed types.\n\nUsed when the exact type is not known at compile time, such as in heterogeneous collections or generic APIs. Type tests (`$x is Type`) can narrow `any` to a specific type.\n\nCommonly used for function return types that can produce different types, or for dictionary values.",
            &[
                "$value: any = 42",
                "$value = \"hello\"",
                "$value = [1; 2; 3]",
                "if $value is str { print \"it's a string\" }",
            ],
            &[
                "Use type tests to check the actual type at runtime.",
                "Prefer specific types over `any` when possible for type safety.",
            ],
        ),
        (
            "i8",
            "An 8-bit signed integer.",
            "Represents small signed integers in the range -128 to 127. Useful for memory-efficient storage of small values, such as color channels or error codes.",
            &["$x: i8 = 42", "$y: i8 = -128"],
            &[
                "Overflow behavior depends on the platform.",
                "Use `i64` for general-purpose integers.",
            ],
        ),
        (
            "i16",
            "A 16-bit signed integer.",
            "Represents signed integers in the range -32,768 to 32,767. Useful for fixed-width data formats and protocol buffers.",
            &["$x: i16 = 32767", "$y: i16 = -32768"],
            &["Use `i64` for general-purpose integers."],
        ),
        (
            "i32",
            "A 32-bit signed integer.",
            "Represents signed integers in the range -2,147,483,648 to 2,147,483,647. Commonly used for interoperability with 32-bit APIs and file formats.",
            &["$x: i32 = 2147483647"],
            &["Use `i64` for general-purpose integers."],
        ),
        (
            "isize",
            "A pointer-sized signed integer.",
            "Width matches the platform's pointer size (64 bits on 64-bit systems, 32 bits on 32-bit systems). Used for pointer arithmetic and platform-dependent sizes.",
            &["$offset: isize = -10"],
            &["Prefer `i64` for portable code."],
        ),
        (
            "u8",
            "An 8-bit unsigned integer.",
            "Represents unsigned integers in the range 0 to 255. The fundamental byte type — used for raw binary data, byte arrays, and color values.",
            &["$byte: u8 = 255", "$color: u8 = 0xFF"],
            &["Arrays of `u8` are used for binary data: `arr<u8>`."],
        ),
        (
            "u16",
            "A 16-bit unsigned integer.",
            "Represents unsigned integers in the range 0 to 65,535. Useful for Unicode code points (BMP) and port numbers.",
            &["$port: u16 = 8080"],
            &["Use `i64` for general-purpose integers."],
        ),
        (
            "u32",
            "A 32-bit unsigned integer.",
            "Represents unsigned integers in the range 0 to 4,294,967,295. Commonly used for hashes, checksums, and 32-bit color values.",
            &["$hash: u32 = 0xDEADBEEF"],
            &["Use `i64` for general-purpose integers."],
        ),
        (
            "u64",
            "A 64-bit unsigned integer.",
            "Represents unsigned integers in the range 0 to 18,446,744,073,709,551,615. Used for large counters, file sizes, and memory addresses.",
            &["$size: u64 = 18446744073709551615"],
            &["Use `i64` for general-purpose signed integers."],
        ),
        (
            "usize",
            "A pointer-sized unsigned integer.",
            "Width matches the platform's pointer size. Used for sizes, indices, and lengths — always non-negative.",
            &["$len: usize = len $arr"],
            &["Prefer `i64` for portable code."],
        ),
        (
            "f32",
            "A 32-bit floating-point number.",
            "IEEE 754 single precision floating-point. Uses half the memory of `f64` but with less precision. Useful for graphics, audio, and memory-constrained applications.",
            &["$x: f32 = 3.14"],
            &["Prefer `f64` for general-purpose floating-point."],
        ),
        (
            "char",
            "A single Unicode character.",
            "Represents a single Unicode scalar value (code point). Characters are written with single quotes: `'A'`, `'中'`, `'🎉'`.",
            &["$c: char = 'A'", "$emoji: char = '🎉'"],
            &["Characters are Unicode code points, not bytes."],
        ),
    ];

    for &(name, summary, description, examples, notes) in &primitives {
        // Don't add if already in the registry (e.g., Result)
        if !types.iter().any(|t| t.name == name) {
            types.push(TypeDoc {
                name: name.to_string(),
                domain: None,
                kind: "Primitive".to_string(),
                summary: Some(summary.to_string()),
                description: Some(description.to_string()),
                examples: examples.iter().map(|s| s.to_string()).collect(),
                notes: notes.iter().map(|s| s.to_string()).collect(),
                fields: Vec::new(),
                variants: Vec::new(),
            });
        }
    }
    types.sort_by(|a, b| a.name.cmp(&b.name));

    DocModel {
        domains,
        natives,
        types,
        traits,
        impls,
    }
}

fn kind_name(k: TypeKind) -> &'static str {
    match k {
        TypeKind::Opaque => "Opaque",
        TypeKind::Record => "Record",
        TypeKind::Sum => "Sum",
        TypeKind::Alias => "Alias",
    }
}

/// Merge user source files into the model.
pub fn collect_src(path: &Path, model: &mut DocModel) -> Result<()> {
    if path.is_file() {
        collect_one(path, model)?;
    } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                collect_src(&p, model)?;
            } else if p.extension().map(|e| e == "xcl").unwrap_or(false) {
                collect_one(&p, model)?;
            }
        }
    }
    Ok(())
}

fn collect_one(path: &Path, model: &mut DocModel) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let file = path.display().to_string();
    let tokens = xcl_toolchain::lexer::lex(&source, &file);
    let ast = xcl_toolchain::parser::parse(&tokens, &source);

    // Find doc comments preceding functions
    // In XCL, comments start with # — we look for comment tokens before function defs
    let mut pending_doc: Option<String> = None;

    for node in &ast {
        if let xcl_toolchain::AstNode::FunctionDef {
            name,
            params,
            return_type,
            ..
        } = node
        {
            let summary = pending_doc.take();
            let args_str = params
                .iter()
                .map(|p| {
                    let ty = p.type_annotation.as_deref().unwrap_or("any");
                    format!("{}: {}", p.name, ty)
                })
                .collect::<Vec<_>>()
                .join("; ");
            let parsed_params = if args_str.is_empty() {
                Vec::new()
            } else {
                parse_params(&args_str)
            };
            model.natives.push(Native {
                full_name: name.clone(),
                domain: None,
                method: name.clone(),
                args: if args_str.is_empty() {
                    None
                } else {
                    Some(args_str)
                },
                ret: return_type.clone(),
                params: parsed_params,
                named: Vec::new(),
                summary,
                arg_desc: Vec::new(),
                description: None,
                examples: Vec::new(),
                notes: Vec::new(),
            });
        }
    }

    Ok(())
}
