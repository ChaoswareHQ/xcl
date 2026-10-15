use std::fmt::Write;
use crate::model::{DocModel, TypeDoc};
use crate::highlight::escape_html;
use super::{page, link_type};

pub fn render(t: &TypeDoc, model: &DocModel) -> String {
    let mut body = String::new();
    let prefix = "../"; // type pages are always in types/ subdirectory

    // Title with kind badge — rustdoc style
    writeln!(body, "<h1><code>{}</code> <span class=\"kind-badge\">{}</span></h1>",
        escape_html(&t.name), t.kind).unwrap();

    // Summary docblock — summary + description + notes flowing together
    if let Some(s) = &t.summary {
        write!(body, "<div class=\"docblock\"><p>{}</p>", escape_html(s)).unwrap();
    }
    if let Some(desc) = &t.description {
        // Split on double newlines for paragraphs
        for para in desc.split("\n\n") {
            writeln!(body, "<p>{}</p>", escape_html(para.trim())).unwrap();
        }
    }
    if !t.notes.is_empty() {
        writeln!(body, "<p>").unwrap();
        for note in &t.notes {
            writeln!(body, "<em>{} </em>", escape_html(note)).unwrap();
        }
        writeln!(body, "</p>").unwrap();
    }
    writeln!(body, "</div>").unwrap();

    // Examples
    if !t.examples.is_empty() {
        writeln!(body, "<h2 id=\"examples\">Examples</h2>").unwrap();
        for example in &t.examples {
            writeln!(body, "<div class=\"example-wrap\"><pre><code>{}</code></pre></div>",
                crate::highlight::highlight(example)).unwrap();
        }
    }

    // Variants (for Sum types like Result)
    if !t.variants.is_empty() {
        writeln!(body, "<h2 id=\"variants\">Variants</h2>").unwrap();
        for v in &t.variants {
            writeln!(body, "<div class=\"item-list\">").unwrap();
            writeln!(body, "<div class=\"item-name\"><code>{}</code></div>", escape_html(&v.name)).unwrap();
            if !v.fields.is_empty() {
                writeln!(body, "<table><tr><th>Name</th><th>Type</th></tr>").unwrap();
                for f in &v.fields {
                    let linked_ty = link_type(&f.ty, model, prefix);
                    let ty_display = if f.optional {
                        format!("{}?", linked_ty)
                    } else {
                        linked_ty
                    };
                    writeln!(body, "<tr><td><code>{}</code></td><td>{}</td></tr>",
                        f.name, ty_display).unwrap();
                }
                writeln!(body, "</table>").unwrap();
            }
            writeln!(body, "</div>").unwrap();
        }
    }

    // Fields (for Record types)
    if !t.fields.is_empty() {
        writeln!(body, "<h2 id=\"fields\">Fields</h2>").unwrap();
        for f in &t.fields {
            let linked_ty = link_type(&f.ty, model, prefix);
            let ty_display = if f.optional {
                format!("{}?", linked_ty)
            } else {
                linked_ty
            };
            writeln!(body, "<div class=\"item-list\">").unwrap();
            writeln!(body, "<div class=\"item-name\"><code>{}</code>: {}</div>",
                escape_html(&f.name), ty_display).unwrap();
            writeln!(body, "</div>").unwrap();
        }
    }

    // Related functions — find all functions that take or return this type
    let related: Vec<_> = model.natives.iter().filter(|n| {
        // Returns this type
        n.ret.as_deref() == Some(&t.name) ||
        // Takes this type as a parameter
        n.params.iter().any(|p| p.ty == t.name) ||
        // Named param uses this type
        n.named.iter().any(|p| p.ty == t.name)
    }).collect();

    if !related.is_empty() {
        writeln!(body, "<h2 id=\"methods\">Methods</h2>").unwrap();
        for n in &related {
            // Build signature
            let sig = if let Some(args) = &n.args {
                let xcl_args = args.split(';')
                    .map(|arg| {
                        let parts: Vec<&str> = arg.trim().split(':').collect();
                        if parts.len() == 2 {
                            format!("${}: {}", parts[0].trim(), parts[1].trim())
                        } else {
                            arg.trim().to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{}({}) -> {}", n.method, xcl_args, n.ret.as_deref().unwrap_or("none"))
            } else {
                format!("{}() -> {}", n.method, n.ret.as_deref().unwrap_or("none"))
            };
            writeln!(body, "<div class=\"item-list\">").unwrap();
            writeln!(body, "<div class=\"item-name\"><a href=\"{prefix}functions/{}.html\"><code>{}</code></a></div>",
                n.full_name, escape_html(&sig)).unwrap();
            if let Some(summary) = &n.summary {
                writeln!(body, "<div class=\"docblock\"><p>{}</p></div>", escape_html(summary)).unwrap();
            }
            writeln!(body, "</div>").unwrap();
        }
    }

    // Impl blocks — "impl Trait for Type"
    let impls: Vec<_> = model.impls.iter()
        .filter(|i| i.type_name == t.name)
        .collect();

    if !impls.is_empty() {
        writeln!(body, "<h2 id=\"implementations\">Implementations</h2>").unwrap();
        for impl_doc in &impls {
            writeln!(body, "<div class=\"impl-block\">").unwrap();
            writeln!(body, "<div class=\"impl-header\">impl <code>{}</code> for <code>{}</code></div>",
                escape_html(&impl_doc.trait_name), escape_html(&impl_doc.type_name)).unwrap();

            if !impl_doc.methods.is_empty() {
                writeln!(body, "<ul class=\"func-list\">").unwrap();
                for m in &impl_doc.methods {
                    writeln!(body, "<li><code>{}</code>", escape_html(&m.name)).unwrap();
                    if let Some(args) = &m.args {
                        write!(body, " (<code>{}</code>", escape_html(args)).unwrap();
                        if let Some(ret) = &m.ret {
                            write!(body, " → <code>{}</code>", escape_html(ret)).unwrap();
                        }
                        writeln!(body, ")").unwrap();
                    }
                    if let Some(summary) = &m.summary {
                        writeln!(body, " — {}", escape_html(summary)).unwrap();
                    }
                    writeln!(body, "</li>").unwrap();
                }
                writeln!(body, "</ul>").unwrap();
            }
            writeln!(body, "</div>").unwrap();
        }
    }

    page(&t.name, Some("types"), model, &body)
}
