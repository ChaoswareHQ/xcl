use std::fmt::Write;
use crate::model::DocModel;
use super::page;

pub fn render(model: &DocModel) -> String {
    let mut body = String::new();

    writeln!(body, "<h1>All items</h1>").unwrap();
    writeln!(body, "<ul>").unwrap();

    for n in &model.natives {
        writeln!(body, "<li><a href=\"functions/{}.html\"><code>{}</code></a></li>",
            n.full_name, n.full_name).unwrap();
    }
    for t in &model.types {
        writeln!(body, "<li><a href=\"types/{}.html\"><code>{}</code></a></li>",
            t.name, t.name).unwrap();
    }

    writeln!(body, "</ul>").unwrap();
    page("All items", None, model, &body)
}
