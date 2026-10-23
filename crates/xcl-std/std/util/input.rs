//! Fast standard-input helpers.

use std::io::{BufRead, Write};

use xcl_macros::xcl_impl;
use xcl_vm::Value;

use super::Util;

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

#[xcl_impl(auto_import, hot)]
impl Util {
    /// Read one line from stdin, optionally showing a prompt first.
    ///
    /// Returns `none` at end-of-input. The prompt is written without a newline
    /// and stdout is flushed before reading, so it appears immediately.
    pub fn input(prompt: Value) -> Value {
        show_prompt(prompt);
        match read_line() {
            Some(line) => Value::from_str_boxed(line),
            None => Value::nil(),
        }
    }

    /// Read one line from stdin without a prompt.
    pub fn readline() -> Value {
        match read_line() {
            Some(line) => Value::from_str_boxed(line),
            None => Value::nil(),
        }
    }
}
