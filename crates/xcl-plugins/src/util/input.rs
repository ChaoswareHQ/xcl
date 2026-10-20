//! Fast standard-input helpers.

use std::io::{BufRead, Read, Write};

use xcl_macros::xcl_impl;
use xcl_vm::Value;

use super::{Util, rule};

fn gate(op: &str) -> bool {
    rule::gate_family("input", op)
}

fn read_line() -> Option<String> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Some(line)
        }
    }
}

fn show_prompt(prompt: Value) {
    if prompt.is_nil() {
        return;
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = super::print::write_value(&mut out, prompt);
    let _ = out.flush();
}

fn prompt_and_read(prompt: Value) -> Value {
    if !gate("input") {
        return Value::nil();
    }
    show_prompt(prompt);
    match read_line() {
        Some(line) => Value::from_str_boxed(line),
        None => Value::nil(),
    }
}

#[xcl_impl(auto_import)]
impl Util {
    /// Read one line from stdin, optionally showing a prompt first.
    ///
    /// Returns `none` at end-of-input. The prompt is written without a newline
    /// and stdout is flushed before reading, so it appears immediately.
    pub fn input(prompt: Value) -> Value {
        prompt_and_read(prompt)
    }

    /// Read one line from stdin without a prompt.
    pub fn readline() -> Value {
        if !gate("readline") {
            return Value::nil();
        }
        match read_line() {
            Some(line) => Value::from_str_boxed(line),
            None => Value::nil(),
        }
    }

    /// Read all remaining input from stdin. Returns `none` on error.
    pub fn read() -> Value {
        if !gate("read") {
            return Value::nil();
        }
        let stdin = std::io::stdin();
        let mut buf = String::new();
        match stdin.lock().read_to_string(&mut buf) {
            Ok(_) => Value::from_str_boxed(buf),
            Err(_) => Value::nil(),
        }
    }

    /// Show a prompt and read one line. Alias of `util.input` with a required prompt.
    pub fn prompt(text: String) -> Value {
        if !gate("prompt") {
            return Value::nil();
        }
        show_prompt(Value::from_str_boxed(text));
        match read_line() {
            Some(line) => Value::from_str_boxed(line),
            None => Value::nil(),
        }
    }
}
