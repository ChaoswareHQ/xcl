const KEYWORDS: &[&str] = &[
    "if", "else", "for", "in", "while", "break", "continue",
    "try", "catch", "return", "true", "false", "none",
    "and", "or", "not", "is", "as",
];

const TYPES: &[&str] = &[
    "arr", "str", "fn", "dict", "bool", "any",
    "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64",
    "usize", "isize", "f32", "f64", "char",
];

pub fn highlight(src: &str) -> String {
    let mut out = String::with_capacity(src.len() * 2);
    let mut chars = src.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push_str("<span class=\"str\">\"");
                let mut escaped = false;
                for n in chars.by_ref() {
                    out.push_str(&escape_html_char(n));
                    if n == '\\' && !escaped {
                        escaped = true;
                        continue;
                    }
                    if n == '"' && !escaped {
                        break;
                    }
                    escaped = false;
                }
                out.push_str("</span>");
            }
            '$' => {
                out.push_str("<span class=\"var\">$");
                while let Some(&n) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' {
                        out.push_str(&escape_html_char(n));
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str("</span>");
            }
            '#' if chars.peek() == Some(&'!') => {
                chars.next();
                out.push_str("<span class=\"meta\">#!");
                while let Some(&n) = chars.peek() {
                    if n == '\n' { break; }
                    out.push_str(&escape_html_char(n));
                    chars.next();
                }
                out.push_str("</span>");
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut word = String::new();
                word.push(c);
                while let Some(&n) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' {
                        word.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if KEYWORDS.contains(&word.as_str()) {
                    out.push_str(&format!("<span class=\"kw\">{}</span>", word));
                } else if TYPES.contains(&word.as_str()) {
                    out.push_str(&format!("<span class=\"type\">{}</span>", word));
                } else if word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    out.push_str(&format!("<span class=\"ctor\">{}</span>", word));
                } else {
                    out.push_str(&word);
                }
            }
            c if c.is_ascii_digit() => {
                let mut num = String::new();
                num.push(c);
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_digit() || n == '.' || n == '_' {
                        num.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str(&format!("<span class=\"num\">{}</span>", num));
            }
            '#' => {
                out.push_str("<span class=\"cmt\">#");
                while let Some(&n) = chars.peek() {
                    if n == '\n' { break; }
                    out.push_str(&escape_html_char(n));
                    chars.next();
                }
                out.push_str("</span>");
            }
            _ => out.push_str(&escape_html_char(c)),
        }
    }

    out
}

fn escape_html_char(c: char) -> String {
    match c {
        '<' => "&lt;".into(),
        '>' => "&gt;".into(),
        '&' => "&amp;".into(),
        '"' => "&quot;".into(),
        _   => c.to_string(),
    }
}

pub fn escape_html(s: &str) -> String {
    s.chars().map(escape_html_char).collect()
}
