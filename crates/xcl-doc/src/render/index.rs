use std::fmt::Write;
use crate::model::DocModel;
use crate::highlight::escape_html;
use super::{page, link_type};

pub fn render(model: &DocModel) -> String {
    let mut body = String::new();
    let prefix = "";

    // Crate header — rustdoc style, no stats
    writeln!(body, "<h1>XCL Standard Library <span class=\"kind-badge\">crate</span></h1>").unwrap();
    writeln!(body, "<div class=\"docblock\">").unwrap();
    writeln!(body, "<p>XCL (eXCLerate) is a scripting language designed for simplicity and expressiveness. The standard library provides core functionality for file I/O, string manipulation, input/output, and system interaction.</p>").unwrap();
    writeln!(body, "<p>XCL uses dynamic typing with optional type annotations, supporting records, traits, and sum types for structured programming. Variables are prefixed with `$`, and functions are called with space-separated arguments or named parameters using `--name` syntax.</p>").unwrap();
    writeln!(body, "<p>The language compiles to bytecode executed by a virtual machine, enabling fast startup and portable execution. Built-in types include integers (`i64`), floats (`f64`), strings (`str`), booleans (`bool`), arrays (`arr`), and dictionaries (`dict`).</p>").unwrap();
    writeln!(body, "<p>Explore the full library structure with the interactive <a href=\"{prefix}tree.html\">Tree View</a> visualization.</p>").unwrap();
    writeln!(body, "<h2 id=\"summary\">How to read this documentation</h2>").unwrap();
    writeln!(body, "<p>Each type and function has its own page with a signature, description, examples, and notes. Type names are linked to their documentation — click on <a href=\"{prefix}types/str.html\">str</a>, <a href=\"{prefix}types/i64.html\">i64</a>, or <a href=\"{prefix}types/Result.html\">Result</a> to see their full documentation.</p>").unwrap();
    writeln!(body, "<p>Functions are organized into domains. The <a href=\"{prefix}io/index.html\">io</a> domain contains all input/output operations: file reading and writing, standard input and output, and system interaction.</p>").unwrap();
    writeln!(body, "</div>").unwrap();

    // Summary section — matches rustdoc's "In XCL Standard Library" section
    writeln!(body, "<h2 id=\"in-xcl-standard-library\">In XCL Standard Library</h2>").unwrap();

    // Domains
    if !model.domains.is_empty() {
        writeln!(body, "<h3 id=\"domains\"><a href=\"#domains\">§</a>Domains</h3>").unwrap();
        writeln!(body, "<ul class=\"item-list\">").unwrap();
        for d in &model.domains {
            writeln!(body, "<li><a class=\"mod\" href=\"{}/index.html\">{}</a></li>",
                d.name, escape_html(&d.name)).unwrap();
        }
        writeln!(body, "</ul>").unwrap();
    }

    // Top-level functions
    let top: Vec<_> = model.natives.iter().filter(|n| n.domain.is_none()).collect();
    if !top.is_empty() {
        writeln!(body, "<h3 id=\"functions\"><a href=\"#functions\">§</a>Functions</h3>").unwrap();
        for n in &top {
            render_item(&mut body, n, model, prefix);
        }
    }

    // Types — only top-level types (no domain), domain-scoped types appear on their domain page
    let top_types: Vec<_> = model.types.iter().filter(|t| t.domain.is_none()).collect();
    if !top_types.is_empty() {
        writeln!(body, "<h3 id=\"types\"><a href=\"#types\">§</a>Types</h3>").unwrap();
        for t in &top_types {
            writeln!(body, "<ul class=\"item-list\">").unwrap();
            writeln!(body, "<li><code><a href=\"types/{}.html\">{}</a></code>",
                t.name, escape_html(&t.name)).unwrap();
            if let Some(summary) = &t.summary {
                writeln!(body, " — {}", escape_html(summary)).unwrap();
            }
            writeln!(body, "</li>").unwrap();
            writeln!(body, "</ul>").unwrap();
        }
    }

    page("XCL Docs", None, model, &body)
}

/// Render a single function item in a list (rustdoc style: signature + doc on one line)
fn render_item(body: &mut String, n: &crate::model::Native, model: &DocModel, prefix: &str) {
    use std::fmt::Write;

    // Signature — XCL style with $ prefix, types are linked
    let sig = if let Some(args) = &n.args {
        let xcl_args = args.split(';')
            .map(|arg| {
                let parts: Vec<&str> = arg.trim().split(':').collect();
                if parts.len() == 2 {
                    let linked_ty = link_type(parts[1].trim(), model, prefix);
                    format!("${}: {}", parts[0].trim(), linked_ty)
                } else {
                    arg.trim().to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        let linked_ret = link_type(n.ret.as_deref().unwrap_or("none"), model, prefix);
        format!("{}({}) -> {}", n.method, xcl_args, linked_ret)
    } else {
        let linked_ret = link_type(n.ret.as_deref().unwrap_or("none"), model, prefix);
        format!("{}() -> {}", n.method, linked_ret)
    };

    writeln!(body, "<ul class=\"item-list\">").unwrap();
    writeln!(body, "<li><a href=\"{prefix}functions/{}.html\"><code>{}</code></a>",
        n.full_name, sig).unwrap();

    if let Some(summary) = &n.summary {
        writeln!(body, "<div class=\"docblock\"><p>{}</p></div>", escape_html(summary)).unwrap();
    }
    writeln!(body, "</li>").unwrap();
    writeln!(body, "</ul>").unwrap();
}
