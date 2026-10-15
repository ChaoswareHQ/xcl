pub mod index;
pub mod domain;
pub mod native;
pub mod type_;
pub mod all;
pub mod search;

use std::fmt::Write;
use crate::model::DocModel;
use crate::highlight::escape_html;

/// Known primitive types that have documentation pages.
const PRIMITIVE_TYPES: &[&str] = &[
    "str", "i8", "i16", "i32", "i64", "isize",
    "u8", "u16", "u32", "u64", "usize",
    "f32", "f64", "bool", "char",
    "arr", "dict", "fn", "none", "any", "Result",
];

/// Convert a type name to a clickable link if a doc page exists for it.
/// E.g., "str" → '<a href="../types/str.html">str</a>'
/// "Result" → '<a href="../types/Result.html">Result</a>'
/// "unknown" → 'unknown' (plain text)
pub fn link_type(type_name: &str, model: &DocModel, depth: &str) -> String {
    let name = type_name.trim();
    
    // Check if it's a known primitive or a registered type
    let is_primitive = PRIMITIVE_TYPES.contains(&name);
    let is_registered = model.types.iter().any(|t| t.name == name);
    
    if is_primitive || is_registered {
        format!("<a href=\"{depth}types/{}.html\">{}</a>", name, escape_html(name))
    } else {
        escape_html(name)
    }
}

/// Get the relative path prefix for links based on page depth.
/// Root page: "" (e.g., types/Result.html)
/// Domain page: "../" (e.g., ../types/Result.html)
pub fn link_prefix(active: Option<&str>) -> &'static str {
    match active {
        None => "",
        Some(_) => "../",
    }
}

pub const CSS:       &str = include_str!("../assets/style.css");
pub const SEARCH_JS: &str = include_str!("../assets/search.js");
pub const XCL_JS:    &str = include_str!("../assets/xcl.js");

/// Phosphor SVG icons (inline, no external dependency)
mod icons {
    pub const BOLT: &str = "<svg class=\"ph\" viewBox=\"0 0 256 256\"><path d=\"M160 32H96a16 16 0 0 0-14.7 21.6l32 112A16 16 0 0 0 136 176h16a16 16 0 0 0 14.7-21.6L160 32ZM80 224h96a16 16 0 0 0 14.7-21.6l-32-112A16 16 0 0 0 144 64h-16a16 16 0 0 0-14.7 21.6L80 224Z\" fill=\"none\" stroke=\"currentColor\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"16\"/></svg>";
    pub const SEARCH: &str = "<svg class=\"ph\" viewBox=\"0 0 256 256\"><circle cx=\"96\" cy=\"96\" r=\"56\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"16\"/><path d=\"M176 176l-45.2-45.2\" fill=\"none\" stroke=\"currentColor\" stroke-linecap=\"round\" stroke-width=\"16\"/></svg>";
    pub const FOLDER: &str = "<svg class=\"ph\" viewBox=\"0 0 256 256\"><path d=\"M224 112H104.8a23.15 23.15 0 0 1-22.6-18L76.6 61.3A31.95 31.95 0 0 0 45.1 32H16a16 16 0 0 0-16 16v160a16 16 0 0 0 16 16h208a16 16 0 0 0 16-16V128a16 16 0 0 0-16-16Z\" fill=\"none\" stroke=\"currentColor\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"16\"/></svg>";
    pub const CUBE: &str = "<svg class=\"ph\" viewBox=\"0 0 256 256\"><path d=\"M128 32L32 80l96 48 96-48Z\" fill=\"none\" stroke=\"currentColor\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"16\"/><path d=\"M224 80v64a16 16 0 0 1-8 13.9L128 224l-88-66.1A16 16 0 0 1 32 144V80\" fill=\"none\" stroke=\"currentColor\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"16\"/></svg>";
    pub const MENU: &str = "<svg class=\"ph\" viewBox=\"0 0 256 256\"><path d=\"M48 64h160M48 128h160M48 192h160\" fill=\"none\" stroke=\"currentColor\" stroke-linecap=\"round\" stroke-width=\"16\"/></svg>";
}

/// HTML shell that wraps every page.
pub fn page(title: &str, active: Option<&str>, model: &DocModel, body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 4096);

    let index_path = link_prefix(active);

    // Determine if we're on a domain page — if so, filter sidebar to show only that domain's items
    let is_domain_page = active.is_some();
    let domain_filter = active;

    write!(out, r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — XCL</title>
<style>{css}</style>
</head>
<body>
<button id="menu-toggle" aria-label="Toggle menu">{menu_icon}</button>
<aside id="sidebar">
    <div class="sidebar-header">
        <a class="sidebar-logo" href="{index_path}index.html">
            {bolt_icon}XCL Docs
        </a>
    </div>
    <div id="search-wrapper">
        {search_icon}
        <input id="search" type="text" placeholder="Search (press /)">
    </div>
    <div id="search-results"></div>
"#,
        title     = crate::highlight::escape_html(title),
        css       = CSS,
        index_path = index_path,
        menu_icon = icons::MENU,
        bolt_icon = icons::BOLT,
        search_icon = icons::SEARCH,
    ).unwrap();

    // Sidebar: domains
    if !model.domains.is_empty() {
        writeln!(out, "<div class=\"sidebar-section\">").unwrap();
        writeln!(out, "<div class=\"sidebar-section-title\">{icon} Domains</div>",
            icon = icons::FOLDER).unwrap();
        writeln!(out, "<ul class=\"sidebar-list\">").unwrap();
        for d in &model.domains {
            let active_cls = if Some(d.name.as_str()) == active { " class=\"active\"" } else { "" };
            write!(out, "<li><a{active_cls} href=\"{index_path}{}/index.html\">{}</a></li>",
                d.name, d.name).unwrap();
        }
        writeln!(out, "</ul></div>").unwrap();
    }

    // Sidebar: types — on root page show only top-level types (primitives + Result, etc.); on domain page show only that domain's types
    let types_to_show: Vec<_> = if is_domain_page {
        model.types.iter().filter(|t| t.domain.as_deref() == domain_filter).collect()
    } else {
        model.types.iter().filter(|t| t.domain.is_none()).collect()
    };
    if !types_to_show.is_empty() {
        writeln!(out, "<div class=\"sidebar-section\">").unwrap();
        writeln!(out, "<div class=\"sidebar-section-title\">{icon} Types</div>",
            icon = icons::CUBE).unwrap();
        writeln!(out, "<ul class=\"sidebar-list\">").unwrap();
        for t in &types_to_show {
            write!(out, "<li><a href=\"{index_path}types/{}.html\">{}</a></li>",
                t.name, t.name).unwrap();
        }
        writeln!(out, "</ul></div>").unwrap();
    }

    // Sidebar: functions — on root page show only top-level functions; on domain page show only that domain's functions
    let funcs_to_show: Vec<_> = if is_domain_page {
        model.natives.iter().filter(|n| n.domain.as_deref() == domain_filter).collect()
    } else {
        model.natives.iter().filter(|n| n.domain.is_none()).collect()
    };
    if !funcs_to_show.is_empty() {
        writeln!(out, "<div class=\"sidebar-section\">").unwrap();
        writeln!(out, "<div class=\"sidebar-section-title\">{icon} Functions</div>",
            icon = icons::BOLT).unwrap();
        writeln!(out, "<ul class=\"sidebar-list\">").unwrap();
        for n in &funcs_to_show {
            write!(out, "<li><a href=\"{index_path}functions/{}.html\">{}</a></li>",
                n.full_name, n.method).unwrap();
        }
        writeln!(out, "</ul></div>").unwrap();
    }

    writeln!(out, "</aside>\n<main>\n{body}\n</main>").unwrap();

    // Search index + scripts
    write!(out, r#"<script src="{index_path}static/search-index.js"></script>
<script>{SEARCH_JS}</script>
<script>{XCL_JS}</script>
<script>{MENU_JS}</script>
</body></html>"#,
        index_path = index_path,
        SEARCH_JS  = SEARCH_JS,
        XCL_JS     = XCL_JS,
        MENU_JS    = MENU_JS,
    ).unwrap();

    out
}


/// Minimal mobile menu toggle script.
pub const MENU_JS: &str = r#"
(function() {
    var toggle = document.getElementById('menu-toggle');
    var sidebar = document.getElementById('sidebar');
    if (!toggle || !sidebar) return;
    toggle.addEventListener('click', function() {
        sidebar.classList.toggle('open');
    });
    document.addEventListener('click', function(e) {
        if (!sidebar.contains(e.target) && e.target !== toggle) {
            sidebar.classList.remove('open');
        }
    });
})();
"#;
