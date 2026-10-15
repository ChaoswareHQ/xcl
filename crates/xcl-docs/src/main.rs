use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use xcl_vm::{NATIVE_REGISTRY, TYPE_REGISTRY};

fn main() {
    // Force linkme to collect all entries by creating a VM
    let _vm = xcl_std::new_vm();

    let out_dir = Path::new("docs");
    fs::create_dir_all(out_dir).expect("Failed to create docs directory");

    // Collect functions grouped by domain
    let mut functions: BTreeMap<Option<&'static str>, Vec<&xcl_vm::NativeEntry>> = BTreeMap::new();
    for entry in NATIVE_REGISTRY.iter() {
        functions.entry(entry.domain).or_default().push(entry);
    }

    // Collect types grouped by domain
    let mut types: BTreeMap<Option<&'static str>, Vec<&xcl_vm::TypeDescriptor>> = BTreeMap::new();
    for desc in TYPE_REGISTRY.iter() {
        types.entry(desc.domain).or_default().push(desc);
    }

    // Generate README.md
    let mut readme = String::new();
    readme.push_str("# XCL Standard Library\n\n");
    readme.push_str("Auto-generated documentation for XCL built-in functions and types.\n\n");
    readme.push_str("## Table of Contents\n\n");

    let mut all_domains: Vec<Option<&'static str>> = Vec::new();
    for domain in functions.keys().chain(types.keys()) {
        if !all_domains.contains(domain) {
            all_domains.push(*domain);
        }
    }

    for domain in &all_domains {
        let label = match domain {
            Some(d) => d.to_string(),
            None => "Global".to_string(),
        };
        readme.push_str(&format!("- [{label}](#{label_slug})\n", label = label, label_slug = slugify(&label)));
    }

    readme.push('\n');

    // Generate domain sections
    for domain in &all_domains {
        let label = match domain {
            Some(d) => d.to_string(),
            None => "Global".to_string(),
        };
        readme.push_str(&format!("## {label}\n\n"));

        // Functions
        if let Some(entries) = functions.get(domain) {
            if !entries.is_empty() {
                readme.push_str("### Functions\n\n");
                for entry in entries {
                    readme.push_str(&format_fn_doc(entry));
                    readme.push('\n');
                }
            }
        }

        // Types
        if let Some(descs) = types.get(domain) {
            if !descs.is_empty() {
                readme.push_str("### Types\n\n");
                for desc in descs {
                    readme.push_str(&format_type_doc(desc));
                    readme.push('\n');
                }
            }
        }
    }

    let readme_path = out_dir.join("README.md");
    let mut file = fs::File::create(&readme_path).expect("Failed to create README.md");
    file.write_all(readme.as_bytes())
        .expect("Failed to write README.md");

    println!("Generated docs/README.md");
}

fn format_fn_doc(entry: &xcl_vm::NativeEntry) -> String {
    let mut out = String::new();
    let full_name = match entry.domain {
        Some(d) => format!("{}.{}", d, entry.method),
        None => entry.method.to_string(),
    };

    out.push_str(&format!("#### `{}`\n\n", full_name));

    if let Some(summary) = entry.summary {
        out.push_str(summary);
        out.push('\n');
        out.push('\n');
    }

    if let Some(args) = entry.args {
        out.push_str("**Signature:**\n\n");
        out.push_str(&format!("```xcl\n{}({}) -> {}\n```\n\n",
            full_name,
            args,
            entry.ret.unwrap_or("none")));
    } else if let Some(ret) = entry.ret {
        out.push_str(&format!("**Returns:** `{}`\n\n", ret));
    }

    if !entry.arg_desc.is_empty() {
        out.push_str("**Arguments:**\n\n");
        out.push_str("| Parameter | Description |\n");
        out.push_str("|---|---|\n");
        for (name, desc) in entry.arg_desc {
            out.push_str(&format!("| `{}` | {} |\n", name, desc));
        }
        out.push('\n');
    }

    out
}

fn format_type_doc(desc: &xcl_vm::TypeDescriptor) -> String {
    let mut out = String::new();
    out.push_str(&format!("#### `{}`\n\n", desc.name));

    if let Some(summary) = desc.summary {
        out.push_str(summary);
        out.push('\n');
        out.push('\n');
    }

    let kind_str = match desc.kind {
        xcl_vm::TypeKind::Opaque => "Opaque",
        xcl_vm::TypeKind::Record => "Record",
        xcl_vm::TypeKind::Sum => "Sum",
        xcl_vm::TypeKind::Alias => "Alias",
    };
    out.push_str(&format!("**Kind:** {}\n\n", kind_str));

    if !desc.fields.is_empty() {
        out.push_str("**Fields:**\n\n");
        out.push_str("| Name | Type | Optional |\n");
        out.push_str("|---|---|---|\n");
        for field in desc.fields {
            out.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                field.name,
                field.ty,
                if field.optional { "yes" } else { "no" }
            ));
        }
        out.push('\n');
    }

    if !desc.variants.is_empty() {
        out.push_str("**Variants:**\n\n");
        for variant in desc.variants {
            out.push_str(&format!("##### `{}`\n\n", variant.name));
            if !variant.fields.is_empty() {
                out.push_str("| Name | Type | Optional |\n");
                out.push_str("|---|---|---|\n");
                for field in variant.fields {
                    out.push_str(&format!(
                        "| `{}` | `{}` | {} |\n",
                        field.name,
                        field.ty,
                        if field.optional { "yes" } else { "no" }
                    ));
                }
                out.push('\n');
            }
        }
    }

    out
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .replace(['.', ' '], "_")
}
