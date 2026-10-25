//! Fast console output.
//!
//! ```xcl
//! print "hello"
//! print "a" "b" 1
//! ```
//!
//! Values are written directly to one locked stdout handle; strings, ints,
//! booleans and nil do not allocate an intermediate `String`.

use std::io::Write;

use xcl_macros::{xcl_fn, xcl_impl};

use super::Util;
use crate::Value;

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

fn write_values<W: Write>(out: &mut W, values: &[Value]) {
    for value in values {
        let _ = write_value(out, *value);
    }
}

/// Print any number of values to stdout without a trailing newline.
#[xcl_fn(name = "util.print", auto_import, hot)]
pub fn print(args: &[Value]) -> Value {
    let handle = std::io::stdout();
    let mut out = handle.lock();
    write_values(&mut out, args);
    Value::nil()
}

#[xcl_impl(auto_import, hot)]
impl Util {
    /// Write raw text to stdout. Returns the number of bytes written.
    pub fn write(text: String) -> i64 {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        match out.write_all(text.as_bytes()) {
            Ok(()) => text.len() as i64,
            Err(_) => 0,
        }
    }

    /// Flush buffered stdout.
    pub fn flush() -> bool {
        std::io::stdout().flush().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::write_value;
    use crate::Value;

    fn render(value: Value) -> String {
        let mut out = Vec::new();
        write_value(&mut out, value).expect("writing to a Vec cannot fail");
        String::from_utf8(out).expect("ascii output")
    }

    #[test]
    fn primitives_render_without_allocating_a_string_value() {
        assert_eq!(render(Value::nil()), "none");
        assert_eq!(render(Value::bool(true)), "true");
        assert_eq!(render(Value::from_i48(-7)), "-7");
        assert_eq!(render(Value::from_str_boxed("hi".to_string())), "hi");
    }
}
