use std::fmt::Write;
use serde_json::json;
use crate::model::DocModel;
use super::link_prefix;

/// Build a hierarchical tree structure from the DocModel.
/// The tree groups functions and types by their domain/module.
pub fn build_tree(model: &DocModel) -> serde_json::Value {
    let mut children: Vec<serde_json::Value> = Vec::new();

    // Group natives by domain
    let mut domain_functions: std::collections::HashMap<String, Vec<serde_json::Value>> = 
        std::collections::HashMap::new();
    
    // Top-level functions (no domain)
    let mut top_functions: Vec<serde_json::Value> = Vec::new();

    for n in &model.natives {
        let func = json!({
            "name": n.method.clone(),
            "type": "function",
            "signature": n.args.as_deref().unwrap_or(""),
            "link": format!("functions/{}.html", n.full_name),
        });

        match &n.domain {
            Some(domain) => {
                domain_functions.entry(domain.clone())
                    .or_insert_with(Vec::new)
                    .push(func);
            }
            None => {
                top_functions.push(func);
            }
        }
    }

    // Group types by domain
    let mut domain_types: std::collections::HashMap<String, Vec<serde_json::Value>> = 
        std::collections::HashMap::new();
    
    let mut top_types: Vec<serde_json::Value> = Vec::new();

    for t in &model.types {
        let ty = json!({
            "name": t.name.clone(),
            "type": "type",
            "kind": t.kind.clone(),
            "link": format!("types/{}.html", t.name),
        });

        match &t.domain {
            Some(domain) => {
                domain_types.entry(domain.clone())
                    .or_insert_with(Vec::new)
                    .push(ty);
            }
            None => {
                top_types.push(ty);
            }
        }
    }

    // Build domain nodes
    for domain in &model.domains {
        let mut domain_children: Vec<serde_json::Value> = Vec::new();

        // Add functions for this domain
        if let Some(funcs) = domain_functions.get(&domain.name) {
            if !funcs.is_empty() {
                domain_children.push(json!({
                    "name": "Functions",
                    "type": "category",
                    "children": funcs
                }));
            }
        }

        // Add types for this domain
        if let Some(types) = domain_types.get(&domain.name) {
            if !types.is_empty() {
                domain_children.push(json!({
                    "name": "Types",
                    "type": "category",
                    "children": types
                }));
            }
        }

        if !domain_children.is_empty() {
            children.push(json!({
                "name": domain.name.clone(),
                "type": "module",
                "link": format!("{}/index.html", domain.name),
                "children": domain_children
            }));
        }
    }

    // Add top-level functions if any
    if !top_functions.is_empty() {
        children.push(json!({
            "name": "Built-in Functions",
            "type": "module",
            "children": top_functions
        }));
    }

    // Add top-level types if any
    if !top_types.is_empty() {
        children.push(json!({
            "name": "Built-in Types",
            "type": "module",
            "children": top_types
        }));
    }

    json!({
        "name": "xcl-std",
        "type": "root",
        "children": children
    })
}

pub fn render(model: &DocModel) -> String {
    let tree_json = build_tree(model);
    let tree_json_str = serde_json::to_string(&tree_json).unwrap();

    let mut body = String::new();

    writeln!(body, r#"
<h1>XCL Standard Library <span class="kind-badge">Tree View</span></h1>
<div class="docblock">
<p>Interactive visualization of the XCL standard library structure. Click on nodes to expand or collapse branches.</p>
<p>The tree shows all modules, types, and functions organized hierarchically, making it easy to explore the full capabilities of the standard library.</p>
</div>

<div id="tree-container"></div>
<script id="tree-data" type="application/json">{}</script>
    "#, tree_json_str).unwrap();

    let mut html = String::new();
    let prefix = link_prefix(None);

    write!(html, r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Tree View — XCL</title>
<style>{}</style>
</head>
<body>
<button id="menu-toggle" aria-label="Toggle menu">{}</button>
<aside id="sidebar">
    <div class="sidebar-header">
        <a class="sidebar-logo" href="{}index.html">
            {}XCL Docs
        </a>
    </div>
    <div id="search-wrapper">
        {}
        <input id="search" type="text" placeholder="Search (press /)">
    </div>
    <div id="search-results"></div>
"#,
        super::CSS,
        super::icons::MENU,
        prefix,
        super::icons::BOLT,
        super::icons::SEARCH,
    ).unwrap();

    // Sidebar: domains
    if !model.domains.is_empty() {
        writeln!(html, "<div class=\"sidebar-section\">").unwrap();
        writeln!(html, "<div class=\"sidebar-section-title\">{icon} Domains</div>",
            icon = super::icons::FOLDER).unwrap();
        writeln!(html, "<ul class=\"sidebar-list\">").unwrap();
        for d in &model.domains {
            write!(html, "<li><a href=\"{}/index.html\">{}</a></li>",
                d.name, d.name).unwrap();
        }
        writeln!(html, "</ul></div>").unwrap();
    }

    // Sidebar: types
    let top_types: Vec<_> = model.types.iter().filter(|t| t.domain.is_none()).collect();
    if !top_types.is_empty() {
        writeln!(html, "<div class=\"sidebar-section\">").unwrap();
        writeln!(html, "<div class=\"sidebar-section-title\">{icon} Types</div>",
            icon = super::icons::CUBE).unwrap();
        writeln!(html, "<ul class=\"sidebar-list\">").unwrap();
        for t in &top_types {
            write!(html, "<li><a href=\"types/{}.html\">{}</a></li>",
                t.name, t.name).unwrap();
        }
        writeln!(html, "</ul></div>").unwrap();
    }

    writeln!(html, "</aside>\n<main>\n{}\n</main>", body).unwrap();

    // Scripts
    write!(html, r#"
<script src="https://d3js.org/d3.v7.min.js"></script>
<script src="{}static/search-index.js"></script>
<script>{}</script>
<script>{}</script>
<script>{}</script>
<script>{}</script>
</body></html>"#,
        prefix,
        super::SEARCH_JS,
        super::XCL_JS,
        super::MENU_JS,
        TREE_JS,
    ).unwrap();

    html
}

pub const TREE_JS: &str = include_str!("../assets/tree.js");
