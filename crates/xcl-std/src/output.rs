use std::cell::RefCell;
use std::io::{self, BufWriter, StdoutLock, StderrLock, Write};

use xcl_macros::xcl_fn;
use xcl_vm::Value;

const OUT_CAPACITY: usize = 128 * 1024;

// ═══════════════════════════════════════════════════════════════
//  FastOut
// ═══════════════════════════════════════════════════════════════

pub struct FastOut<W: Write> {
    buf: BufWriter<W>,
}

impl<W: Write> FastOut<W> {
    fn new(inner: W) -> Self {
        Self {
            buf: BufWriter::with_capacity(OUT_CAPACITY, inner),
        }
    }

    #[inline]
    pub fn flush(&mut self) -> io::Result<()> {
        self.buf.flush()
    }

    #[inline]
    pub fn write_byte(&mut self, b: u8) -> io::Result<()> {
        self.buf.write_all(&[b])
    }

    #[inline]
    pub fn write_bytes(&mut self, b: &[u8]) -> io::Result<()> {
        self.buf.write_all(b)
    }

    #[inline]
    pub fn write_str(&mut self, s: &str) -> io::Result<()> {
        self.buf.write_all(s.as_bytes())
    }

    /// Write a `Value` with zero allocation for primitives.
    pub fn write_value(&mut self, v: Value) -> io::Result<()> {
        if v.is_nil() {
            return self.buf.write_all(b"()");
        }
        if v.is_bool() {
            return self.buf.write_all(if v.as_bool() { b"true" } else { b"false" });
        }
        if v.is_int() {
            let mut b = itoa::Buffer::new();
            return self.buf.write_all(b.format(v.as_i48()).as_bytes());
        }
        if v.is_float() {
            let mut b = ryu::Buffer::new();
            return self.buf.write_all(b.format(v.as_f64()).as_bytes());
        }
        if let Some(s) = v.as_str() {
            return self.buf.write_all(s.as_bytes());
        }
        // Fallback for arr / dict / fn — allocates via as_string()
        let s = v.as_string();
        self.buf.write_all(s.as_bytes())
    }
}

impl<W: Write> Drop for FastOut<W> {
    fn drop(&mut self) {
        let _ = self.buf.flush();
    }
}

// ═══════════════════════════════════════════════════════════════
//  Thread-local instances
// ═══════════════════════════════════════════════════════════════

thread_local! {
    static OUT: RefCell<FastOut<StdoutLock<'static>>> =
        RefCell::new(FastOut::new(io::stdout().lock()));
    static ERR: RefCell<FastOut<StderrLock<'static>>> =
        RefCell::new(FastOut::new(io::stderr().lock()));
}

#[inline]
pub fn with_out<R>(f: impl FnOnce(&mut FastOut<StdoutLock<'static>>) -> R) -> R {
    OUT.with(|o| f(&mut o.borrow_mut()))
}

#[inline]
pub fn with_err<R>(f: impl FnOnce(&mut FastOut<StderrLock<'static>>) -> R) -> R {
    ERR.with(|e| f(&mut e.borrow_mut()))
}

pub fn flush_out() {
    OUT.with(|o| {
        let _ = o.borrow_mut().flush();
    });
    ERR.with(|e| {
        let _ = e.borrow_mut().flush();
    });
}

// ═══════════════════════════════════════════════════════════════
//  ANSI color helper
// ═══════════════════════════════════════════════════════════════

#[inline]
fn fg_code(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "black" => b"\x1b[30m",
        "red" => b"\x1b[31m",
        "green" => b"\x1b[32m",
        "yellow" => b"\x1b[33m",
        "blue" => b"\x1b[34m",
        "magenta" => b"\x1b[35m",
        "cyan" => b"\x1b[36m",
        "white" => b"\x1b[37m",
        "bright_black" => b"\x1b[90m",
        "bright_red" => b"\x1b[91m",
        "bright_green" => b"\x1b[92m",
        "bright_yellow" => b"\x1b[93m",
        "bright_blue" => b"\x1b[94m",
        "bright_magenta" => b"\x1b[95m",
        "bright_cyan" => b"\x1b[96m",
        "bright_white" => b"\x1b[97m",
        _ => return None,
    })
}

#[inline]
fn bg_code(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "black" => b"\x1b[40m",
        "red" => b"\x1b[41m",
        "green" => b"\x1b[42m",
        "yellow" => b"\x1b[43m",
        "blue" => b"\x1b[44m",
        "magenta" => b"\x1b[45m",
        "cyan" => b"\x1b[46m",
        "white" => b"\x1b[47m",
        "bright_black" => b"\x1b[100m",
        "bright_red" => b"\x1b[101m",
        "bright_green" => b"\x1b[102m",
        "bright_yellow" => b"\x1b[103m",
        "bright_blue" => b"\x1b[104m",
        "bright_magenta" => b"\x1b[105m",
        "bright_cyan" => b"\x1b[106m",
        "bright_white" => b"\x1b[107m",
        _ => return None,
    })
}

const RESET: &[u8] = b"\x1b[0m";

// ═══════════════════════════════════════════════════════════════
//  Named arg parsing helpers
// ═══════════════════════════════════════════════════════════════

/// Parse named args for `io.print`: foreground, background, no_newline, to_stderr.
fn parse_print_named_args(
    args: &[Value],
) -> (&[Value], Option<&str>, Option<&str>, bool, bool) {
    let known = ["foreground", "background", "no_newline", "to_stderr"];
    let (positional, named) = xcl_vm::split_named_args(args, &known);

    let fg = named.get("foreground").and_then(|v| v.as_str());
    let bg = named.get("background").and_then(|v| v.as_str());
    let no_newline = named.get("no_newline").map(|v| v.is_truthy()).unwrap_or(false);
    let to_stderr = named.get("to_stderr").map(|v| v.is_truthy()).unwrap_or(false);

    (positional, fg, bg, no_newline, to_stderr)
}

// ═══════════════════════════════════════════════════════════════
//  io.print — single print function, routes to stdout or stderr via `to_stderr`
// ═══════════════════════════════════════════════════════════════

#[xcl_fn(domain = "io", name = "print", ret = "none")]
pub fn io_print_fn(args: &[Value]) -> Value {
    let (values, fg, bg, no_newline, to_stderr) = parse_print_named_args(args);

    let fg_code = fg.and_then(fg_code);
    let bg_code = bg.and_then(bg_code);
    let colored = fg_code.is_some() || bg_code.is_some();

    if to_stderr {
        with_err(|w| {
            if let Some(c) = bg_code {
                let _ = w.write_bytes(c);
            }
            if let Some(c) = fg_code {
                let _ = w.write_bytes(c);
            }
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    let _ = w.write_byte(b' ');
                }
                let _ = w.write_value(*v);
            }
            if colored {
                let _ = w.write_bytes(RESET);
            }
            if !no_newline {
                let _ = w.write_byte(b'\n');
            }
        });
    } else {
        with_out(|w| {
            if let Some(c) = bg_code {
                let _ = w.write_bytes(c);
            }
            if let Some(c) = fg_code {
                let _ = w.write_bytes(c);
            }
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    let _ = w.write_byte(b' ');
                }
                let _ = w.write_value(*v);
            }
            if colored {
                let _ = w.write_bytes(RESET);
            }
            if !no_newline {
                let _ = w.write_byte(b'\n');
            }
        });
    }

    Value::unit()
}

// ═══════════════════════════════════════════════════════════════
//  io.format — returns a String instead of writing
// ═══════════════════════════════════════════════════════════════

#[xcl_fn(domain = "io", name = "format", ret = "str")]
pub fn io_format_fn(args: &[Value]) -> Value {
    let mut out = String::new();
    for (i, v) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if v.is_nil() {
            out.push_str("()");
        } else if v.is_bool() {
            out.push_str(if v.as_bool() { "true" } else { "false" });
        } else if v.is_int() {
            let mut b = itoa::Buffer::new();
            out.push_str(b.format(v.as_i48()));
        } else if v.is_float() {
            let mut b = ryu::Buffer::new();
            out.push_str(b.format(v.as_f64()));
        } else if let Some(s) = v.as_str() {
            out.push_str(s);
        } else {
            out.push_str(&v.as_string());
        }
    }
    Value::from_str_boxed(out)
}
