//! Fast console output and ANSI colouring.
//!
//! `print`, `println`, `eprint` and `eprintln` accept any number of values and
//! two named style arguments:
//!
//! ```xcl
//! print "hello"
//! println bg_color: "red", fg_color: "black" "hello"
//! print fg_color: "#00ff88" "green"
//! ```
//!
//! Values are written directly to one locked stdout handle; strings, ints,
//! booleans and nil do not allocate an intermediate `String`.

use std::io::Write;

use xcl_macros::{xcl_fn, xcl_impl};
use xcl_vm::Value;

use super::{Util, rule};

/// The named arguments `print` and friends understand.
const STYLE_PARAMS: &[&str] = &["fg_color", "bg_color"];

/// Writes `value` with exactly the text [`Value::as_string`] would produce, but
/// without allocating for the common cases (strings, integers, booleans, nil).
///
/// Floats deliberately go through `as_string`: `f64::to_string` is the
/// language's number formatting, and ryu/itoa-style shortcuts do not reproduce
/// its exponent rules exactly.
pub(crate) fn write_value<W: Write>(out: &mut W, value: Value) -> std::io::Result<()> {
    if value.is_nil() {
        return out.write_all(b"none");
    }
    if let Some(s) = value.as_str() {
        return out.write_all(s.as_bytes());
    }
    if value.is_int() {
        let mut buf = itoa::Buffer::new();
        return out.write_all(buf.format(value.as_i48()).as_bytes());
    }
    if value.is_bool() {
        return out.write_all(if value.as_bool() { b"true" } else { b"false" });
    }
    out.write_all(value.as_string().as_bytes())
}

fn gate_out(op: &str) -> bool {
    rule::gate_family("print", op)
}

fn gate_err(op: &str) -> bool {
    rule::gate_family("eprint", op)
}

/// Splits style options from the printed values.
///
/// A single map argument is treated as a value, not as options, so
/// `print {fg_color: "red"}` prints the map. The trailing-map convention only
/// applies when there is at least one positional value before it.
fn split_style_args(args: &[Value]) -> (&[Value], hashbrown::HashMap<&str, Value>) {
    if args.len() == 1 && args[0].as_map().is_some() {
        return (args, hashbrown::HashMap::new());
    }
    crate::split_named_args(args, STYLE_PARAMS)
}

fn style_prefix(named: &hashbrown::HashMap<&str, Value>) -> Option<String> {
    let mut out = String::new();
    if let Some(value) = named.get("fg_color") {
        push_sgr_value(&mut out, *value, false);
    }
    if let Some(value) = named.get("bg_color") {
        push_sgr_value(&mut out, *value, true);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn push_sgr_value(out: &mut String, value: Value, background: bool) {
    let owned;
    let name: &str = match value.as_str() {
        Some(s) => s,
        None => {
            owned = value.as_string();
            &owned
        }
    };
    if let Some(code) = sgr(name, background) {
        code.write_to(out, background);
    }
}

fn write_values<W: Write>(out: &mut W, values: &[Value], prefix: Option<&str>, newline: bool) {
    for value in values {
        if let Some(prefix) = prefix {
            let _ = out.write_all(prefix.as_bytes());
        }
        let _ = write_value(out, *value);
        if prefix.is_some() {
            let _ = out.write_all(b"\x1b[0m");
        }
    }
    if newline {
        let _ = out.write_all(b"\n");
    }
}

fn print_all(op: &str, args: &[Value], newline: bool, stderr: bool) -> Value {
    let (values, named) = split_style_args(args);
    let allowed = if stderr { gate_err(op) } else { gate_out(op) };
    if !allowed {
        return Value::nil();
    }
    let prefix = style_prefix(&named);
    if stderr {
        let handle = std::io::stderr();
        let mut out = handle.lock();
        write_values(&mut out, values, prefix.as_deref(), newline);
    } else {
        let handle = std::io::stdout();
        let mut out = handle.lock();
        write_values(&mut out, values, prefix.as_deref(), newline);
    }
    Value::nil()
}

/// Print any number of values to stdout without a trailing newline.
#[xcl_fn(name = "util.print", auto_import)]
pub fn print(args: &[Value]) -> Value {
    print_all("print", args, false, false)
}

/// Print any number of values to stdout followed by a newline.
#[xcl_fn(name = "util.println", auto_import)]
pub fn println(args: &[Value]) -> Value {
    print_all("println", args, true, false)
}

/// Print any number of values to stderr followed by a newline.
#[xcl_fn(name = "util.eprintln", auto_import)]
pub fn eprintln(args: &[Value]) -> Value {
    print_all("eprintln", args, true, true)
}

/// Print any number of values to stderr without a trailing newline.
#[xcl_fn(name = "util.eprint", auto_import)]
pub fn eprint(args: &[Value]) -> Value {
    print_all("eprint", args, false, true)
}

/// Write to stderr with a newline. Alias kept for the old `io.error` name.
#[xcl_fn(name = "util.error", auto_import)]
pub fn error(args: &[Value]) -> Value {
    print_all("error", args, true, true)
}

#[xcl_impl(auto_import)]
impl Util {
    /// Write raw text to stdout. Returns the number of bytes written.
    pub fn write(text: String) -> i64 {
        if !gate_out("write") {
            return 0;
        }
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        match out.write_all(text.as_bytes()) {
            Ok(()) => text.len() as i64,
            Err(_) => 0,
        }
    }

    /// Flush buffered stdout.
    pub fn flush() -> bool {
        if !gate_out("flush") {
            return false;
        }
        std::io::stdout().flush().is_ok()
    }

    /// Wrap `text` in a foreground colour.
    ///
    /// `color` is a name (`Red`, `BrightCyan`, `DarkGray`, …) or a hex code
    /// (`#ff8800`, `ff8800`, `#f80`). An unknown colour returns the text
    /// unchanged.
    pub fn fg_color(color: String, text: Value) -> String {
        colour(color.as_str(), text, false)
    }

    /// Wrap `text` in a background colour.
    ///
    /// Accepts the same names and hex codes as [`Util::fg_color`].
    pub fn bg_color(color: String, text: Value) -> String {
        colour(color.as_str(), text, true)
    }
}

fn colour(color: &str, text: Value, background: bool) -> String {
    let mut out = String::with_capacity(24);
    match sgr(color, background) {
        Some(code) => {
            code.write_to(&mut out, background);
            push_value_text(&mut out, text);
            out.push_str("\x1b[0m");
        }
        None => push_value_text(&mut out, text),
    }
    out
}

/// A parsed SGR sequence.
enum Sgr {
    /// A named/bright colour: one parameter.
    Named(u16),
    /// 24-bit colour.
    Rgb(u8, u8, u8),
}

impl Sgr {
    fn write_to(&self, out: &mut String, background: bool) {
        out.push('\x1b');
        out.push('[');
        match *self {
            Sgr::Named(code) => push_u16(out, code),
            Sgr::Rgb(r, g, b) => {
                out.push_str(if background { "48;2;" } else { "38;2;" });
                push_u16(out, r as u16);
                out.push(';');
                push_u16(out, g as u16);
                out.push(';');
                push_u16(out, b as u16);
            }
        }
        out.push('m');
    }
}

fn sgr(color: &str, background: bool) -> Option<Sgr> {
    let raw = color.trim();
    let base = if background { 10 } else { 0 };
    if let Some((r, g, b)) = parse_hex(raw) {
        return Some(Sgr::Rgb(r, g, b));
    }
    let mut normalised = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            continue;
        }
        for lower in ch.to_lowercase() {
            normalised.push(lower);
        }
    }
    let code = match normalised.as_str() {
        "black" => 30,
        "red" | "darkred" => 31,
        "green" | "darkgreen" => 32,
        "yellow" | "darkyellow" | "yellowblack" => 33,
        "blue" | "darkblue" => 34,
        "magenta" | "darkmagenta" => 35,
        "cyan" | "darkcyan" => 36,
        "white" => 37,
        "brightblack" | "gray" | "grey" | "darkgray" | "darkgrey" => 90,
        "brightred" => 91,
        "brightgreen" => 92,
        "brightyellow" => 93,
        "brightblue" => 94,
        "brightmagenta" => 95,
        "brightcyan" => 96,
        "brightwhite" => 97,
        _ => return None,
    };
    // Background SGR codes are the foreground code + 10 (40..47, 100..107).
    Some(Sgr::Named(code + base))
}

fn parse_hex(raw: &str) -> Option<(u8, u8, u8)> {
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn push_u16(out: &mut String, mut n: u16) {
    let mut buf = [0u8; 5];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    // SAFETY: every byte written above is an ASCII digit.
    out.push_str(unsafe { core::str::from_utf8_unchecked(&buf[i..]) });
}

fn push_value_text(out: &mut String, value: Value) {
    if value.is_nil() {
        out.push_str("none");
    } else if let Some(s) = value.as_str() {
        out.push_str(s);
    } else if value.is_int() {
        let mut buf = itoa::Buffer::new();
        out.push_str(buf.format(value.as_i48()));
    } else if value.is_bool() {
        out.push_str(if value.as_bool() { "true" } else { "false" });
    } else {
        out.push_str(&value.as_string());
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn named_colours() {
        assert_eq!(
            evaluate_new("util.fg_color \"Red\" \"x\"").as_str(),
            Some("\x1b[31mx\x1b[0m")
        );
        assert_eq!(
            evaluate_new("util.fg_color \"bright_cyan\" \"x\"").as_str(),
            Some("\x1b[96mx\x1b[0m")
        );
        assert_eq!(
            evaluate_new("util.fg_color \"DarkGray\" \"x\"").as_str(),
            Some("\x1b[90mx\x1b[0m")
        );
        assert_eq!(
            evaluate_new("util.bg_color \"Blue\" \"x\"").as_str(),
            Some("\x1b[44mx\x1b[0m")
        );
    }

    #[test]
    fn hex_colours() {
        assert_eq!(
            evaluate_new("util.fg_color \"#ff8800\" \"x\"").as_str(),
            Some("\x1b[38;2;255;136;0mx\x1b[0m")
        );
        assert_eq!(
            evaluate_new("util.bg_color \"#0f0\" \"x\"").as_str(),
            Some("\x1b[48;2;0;255;0mx\x1b[0m")
        );
        assert_eq!(
            evaluate_new("util.fg_color \"no-such-colour\" \"x\"").as_str(),
            Some("x")
        );
    }
}
