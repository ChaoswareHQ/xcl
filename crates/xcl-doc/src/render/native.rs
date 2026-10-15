use std::fmt::Write;
use crate::model::{DocModel, Native};
use crate::highlight::{highlight, escape_html};
use super::{page, link_type, link_prefix};

pub fn render(n: &Native, model: &DocModel) -> String {
    let mut body = String::new();

    // Title — rustdoc style: "io.read" with type marker
    writeln!(body, "<h1><code>{}</code></h1>",
        escape_html(&n.full_name)).unwrap();

    // Signature block — XCL style: "read($path: str) -> Result"
    let sig = if let Some(args) = &n.args {
        // Convert args from "path: str; mode: str" to "$path: str; $mode: str"
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
        format!("{}({}) -> {}",
            n.method,
            xcl_args,
            n.ret.as_deref().unwrap_or("none"),
        )
    } else {
        format!("{}() -> {}",
            n.method,
            n.ret.as_deref().unwrap_or("none"),
        )
    };
    writeln!(body, "<div class=\"signature\"><code>{}</code></div>", highlight(&sig)).unwrap();

    // Doc block — flows naturally like rustdoc (no h2 headers)
    let has_doc = n.summary.is_some() | n.description.is_some() | !n.notes.is_empty();
    if has_doc {
        writeln!(body, "<div class=\"docblock\">").unwrap();

        // Summary
        if let Some(summary) = &n.summary {
            writeln!(body, "<p>{}</p>", render_markdown(summary)).unwrap();
        }

        // Extended description
        if let Some(description) = &n.description {
            let summary_text = n.summary.as_deref().unwrap_or("");
            if description.trim() != summary_text.trim() {
                writeln!(body, "{}", render_markdown(description)).unwrap();
            }
        }

        // Notes
        if !n.notes.is_empty() {
            for note in &n.notes {
                writeln!(body, "{}", render_markdown(note)).unwrap();
            }
        }

        writeln!(body, "</div>").unwrap();
    }

    // Arguments section — XCL style with $ prefix, types are linked
    let prefix = link_prefix(n.domain.as_deref());
    if !n.params.is_empty() {
        writeln!(body, "<h2 id=\"arguments\">Arguments</h2>").unwrap();
        for p in &n.params {
            let desc = n.arg_desc.iter()
                .find(|(name, _)| name == &p.name)
                .map(|(_, desc)| desc.as_str());
            let linked_ty = link_type(&p.ty, model, prefix);
            writeln!(body, "<div class=\"item-list\">").unwrap();
            writeln!(body, "<div class=\"item-name\"><code>${}</code>: {}",
                escape_html(&p.name), linked_ty).unwrap();
            if let Some(d) = desc {
                writeln!(body, "</div><div class=\"docblock\"><p>{}</p></div>", escape_html(d)).unwrap();
            } else {
                writeln!(body, "</div>").unwrap();
            }
            writeln!(body, "</div>").unwrap();
        }
    }

    // Named parameters — types are linked
    if !n.named.is_empty() {
        writeln!(body, "<h2 id=\"named-parameters\">Named Parameters</h2>").unwrap();
        for p in &n.named {
            let linked_ty = link_type(&p.ty, model, prefix);
            writeln!(body, "<div class=\"item-list\">").unwrap();
            writeln!(body, "<div class=\"item-name\"><code>{}</code>: {} = <code>{}</code>",
                escape_html(&p.name), linked_ty, escape_html(&p.default)).unwrap();
            writeln!(body, "</div>").unwrap();
            writeln!(body, "</div>").unwrap();
        }
    }

    // Return type — linked to type doc page
    if let Some(ret) = &n.ret {
        writeln!(body, "<h2 id=\"returns\">Returns</h2>").unwrap();
        writeln!(body, "<div class=\"item-list\">").unwrap();
        let linked_ret = link_type(ret, model, prefix);
        writeln!(body, "<div class=\"item-name\">{}</div>", linked_ret).unwrap();
        writeln!(body, "</div>").unwrap();
    }

    // Examples
    if !n.examples.is_empty() {
        writeln!(body, "<h2 id=\"examples\">Examples</h2>").unwrap();
        for example in &n.examples {
            writeln!(body, "<div class=\"example-wrap\"><pre><code>{}</code></pre></div>", highlight(example)).unwrap();
        }
    }

    // Source link
    writeln!(body, "<h2 id=\"source\">Source</h2>").unwrap();
    match &n.domain {
        Some(d) => writeln!(body, "<p>Defined in domain <a href=\"../{}/index.html\"><code>{}</code></a></p>", d, escape_html(d)).unwrap(),
        None    => writeln!(body, "<p>Built-in function</p>").unwrap(),
    }

    page(&n.full_name, n.domain.as_deref(), model, &body)
}

fn render_markdown(s: &str) -> String {
    use pulldown_cmark::{Parser, html};
    let parser = Parser::new(s);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}
