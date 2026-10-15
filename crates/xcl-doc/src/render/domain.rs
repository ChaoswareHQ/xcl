use std::fmt::Write;
use crate::model::DocModel;
use crate::highlight::escape_html;
use super::{page, link_type};

pub fn render(domain: &str, model: &DocModel) -> String {
    let mut body = String::new();
    let prefix = "../";

    // Title — rustdoc module style
    writeln!(body, "<h1><span class=\"mod\">module</span> <code>{}</code></h1>",
        escape_html(domain)).unwrap();

    let natives: Vec<_> = model.natives.iter()
        .filter(|n| n.domain.as_deref() == Some(domain))
        .collect();

    // Summary section
    writeln!(body, "<h2 id=\"summary\">Summary</h2>").unwrap();

    // Types subsection
    let domain_types: Vec<_> = model.types.iter()
        .filter(|t| t.domain.as_deref() == Some(domain))
        .collect();
    if !domain_types.is_empty() {
        writeln!(body, "<h3 id=\"types\"><a href=\"#types\">§</a>Types</h3>").unwrap();
        for t in &domain_types {
            writeln!(body, "<ul class=\"item-list\">").unwrap();
            writeln!(body, "<li><code><a href=\"{prefix}types/{}.html\">{}</a></code>",
                t.name, escape_html(&t.name)).unwrap();
            if let Some(summary) = &t.summary {
                writeln!(body, " — {}", escape_html(summary)).unwrap();
            }
            writeln!(body, "</li>").unwrap();
            writeln!(body, "</ul>").unwrap();
        }
    }

    // Functions subsection
    if !natives.is_empty() {
        writeln!(body, "<h3 id=\"functions\"><a href=\"#functions\">§</a>Functions</h3>").unwrap();

        for n in &natives {
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
    }

    page(domain, Some(domain), model, &body)
}
