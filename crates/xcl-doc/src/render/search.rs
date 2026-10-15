use std::fmt::Write;
use crate::model::DocModel;

pub fn render_js(model: &DocModel) -> String {
    let mut items = String::new();
    let mut first = true;

    for n in &model.natives {
        if !first { items.push(','); }
        first = false;
        let path = format!("functions/{}.html", n.full_name);
        write!(items, r#"{{"name":"{}","kind":"fn","path":"{}"}}"#,
            escape_js(&n.full_name), escape_js(&path)).unwrap();
    }
    for t in &model.types {
        if !first { items.push(','); }
        first = false;
        write!(items, r#"{{"name":"{}","kind":"type","path":"types/{}.html"}}"#,
            escape_js(&t.name), escape_js(&t.name)).unwrap();
    }

    format!("window.SEARCH_INDEX = [{}];\n", items)
}

fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
