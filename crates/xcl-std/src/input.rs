use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, StdinLock, Read};

use xcl_macros::xcl_fn;
use xcl_vm::Value;

const IN_CAPACITY: usize = 64 * 1024;

pub struct FastIn {
    buf: BufReader<StdinLock<'static>>,
    scratch: String,
}

impl FastIn {
    fn new() -> Self {
        Self {
            buf: BufReader::with_capacity(IN_CAPACITY, io::stdin().lock()),
            scratch: String::with_capacity(256),
        }
    }

    /// Read one line. Strips trailing `\n` and `\r`.
    pub fn read_line(&mut self) -> io::Result<Option<String>> {
        self.scratch.clear();
        let n = self.buf.read_line(&mut self.scratch)?;
        if n == 0 {
            return Ok(None);
        }
        while self.scratch.ends_with('\n') || self.scratch.ends_with('\r') {
            self.scratch.pop();
        }
        Ok(Some(std::mem::take(&mut self.scratch)))
    }

    /// Read one whitespace-delimited token.
    fn read_token(&mut self) -> io::Result<bool> {
        self.scratch.clear();

        // Skip leading whitespace
        loop {
            let consumed = {
                let buf = self.buf.fill_buf()?;
                if buf.is_empty() {
                    return Ok(false);
                }
                let mut i = 0;
                while i < buf.len() && buf[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < buf.len() {
                    break;
                }
                i
            };
            self.buf.consume(consumed);
        }

        // Read until whitespace
        loop {
            let (consumed, done) = {
                let buf = self.buf.fill_buf()?;
                if buf.is_empty() {
                    break;
                }
                let mut j = 0;
                while j < buf.len() && !buf[j].is_ascii_whitespace() {
                    self.scratch.push(buf[j] as char);
                    j += 1;
                }
                (j, j < buf.len())
            };
            self.buf.consume(consumed);
            if done {
                break;
            }
        }
        Ok(true)
    }

    pub fn read_i64(&mut self) -> io::Result<Option<i64>> {
        if !self.read_token()? {
            return Ok(None);
        }
        Ok(self.scratch.parse::<i64>().ok())
    }

    pub fn read_f64(&mut self) -> io::Result<Option<f64>> {
        if !self.read_token()? {
            return Ok(None);
        }
        Ok(self.scratch.parse::<f64>().ok())
    }

    pub fn read_bool(&mut self) -> io::Result<Option<bool>> {
        if !self.read_token()? {
            return Ok(None);
        }
        match self.scratch.as_str() {
            "true" | "yes" | "y" | "1" => Ok(Some(true)),
            "false" | "no" | "n" | "0" => Ok(Some(false)),
            _ => Ok(None),
        }
    }

    pub fn read_str(&mut self) -> io::Result<Option<String>> {
        if !self.read_token()? {
            return Ok(None);
        }
        Ok(Some(std::mem::take(&mut self.scratch)))
    }

    pub fn read_all(&mut self) -> io::Result<String> {
        let mut s = String::new();
        self.buf.read_to_string(&mut s)?;
        Ok(s)
    }
}

thread_local! {
    static IN: RefCell<FastIn> = RefCell::new(FastIn::new());
}

#[inline]
pub fn with_in<R>(f: impl FnOnce(&mut FastIn) -> R) -> R {
    IN.with(|i| f(&mut i.borrow_mut()))
}

fn emit_prompt(prompt: &str) {
    if prompt.is_empty() {
        return;
    }
    crate::output::with_out(|w| {
        let _ = w.write_str(prompt);
    });
    crate::output::flush_out();
}

#[xcl_fn(domain = "io", name = "input", ret = "str?")]
pub fn io_input_fn(args: &[Value]) -> Value {
    let prompt = args.first().and_then(|v| v.as_str()).unwrap_or("");
    emit_prompt(prompt);

    with_in(|r| match r.read_line() {
        Ok(Some(s)) => Value::from_str_boxed(s),
        _ => Value::unit(),
    })
}

#[xcl_fn(domain = "io", name = "input_line", ret = "str?")]
pub fn io_input_line_fn(args: &[Value]) -> Value {
    io_input_fn(args)
}

#[xcl_fn(domain = "io", name = "input_int", ret = "i64?")]
pub fn io_input_int_fn(args: &[Value]) -> Value {
    let prompt = args.first().and_then(|v| v.as_str()).unwrap_or("");
    emit_prompt(prompt);

    with_in(|r| match r.read_i64() {
        Ok(Some(n)) => Value::from_i48(n),
        _ => Value::unit(),
    })
}

#[xcl_fn(domain = "io", name = "input_float", ret = "f64?")]
pub fn io_input_float_fn(args: &[Value]) -> Value {
    let prompt = args.first().and_then(|v| v.as_str()).unwrap_or("");
    emit_prompt(prompt);

    with_in(|r| match r.read_f64() {
        Ok(Some(f)) => Value::from_f64(f),
        _ => Value::unit(),
    })
}

#[xcl_fn(domain = "io", name = "input_bool", ret = "bool?")]
pub fn io_input_bool_fn(args: &[Value]) -> Value {
    let prompt = args.first().and_then(|v| v.as_str()).unwrap_or("");
    emit_prompt(prompt);

    with_in(|r| match r.read_bool() {
        Ok(Some(b)) => Value::bool_(b),
        _ => Value::unit(),
    })
}

#[xcl_fn(domain = "io", name = "input_word", ret = "str?")]
pub fn io_input_word_fn(args: &[Value]) -> Value {
    let prompt = args.first().and_then(|v| v.as_str()).unwrap_or("");
    emit_prompt(prompt);

    with_in(|r| match r.read_str() {
        Ok(Some(s)) => Value::from_str_boxed(s),
        _ => Value::unit(),
    })
}

#[xcl_fn(domain = "io", name = "input_all", ret = "str")]
pub fn io_input_all_fn(_args: &[Value]) -> Value {
    with_in(|r| match r.read_all() {
        Ok(s) => Value::from_str_boxed(s),
        Err(_) => Value::unit(),
    })
}
