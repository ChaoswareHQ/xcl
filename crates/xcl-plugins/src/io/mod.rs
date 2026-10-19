//! I/O module — Standard input/output operations for XCL.
//!
//! Provides console I/O, file handles, and stream operations.
//!
//! # Usage in XCL
//!
//! ```xcl
//! # Print to stdout
//! io.print "Hello, world!"
//!
//! # Print with newline
//! io.println "Line of text"
//!
//! # Read a line from stdin
//! $input = io.readline
//!
//! # Read all from stdin
//! $data = io.read
//!
//! # Write to stdout
//! io.write "No newline"
//!
//! # Flush output
//! io.flush
//!
//! # Prompt user
//! $name = io.prompt "Enter name: "
//!
//! # Print to stderr
//! io.error "Something went wrong"
//! ```

use std::io::{Read, Write};
use xcl_macros::{Xcl, xcl_impl};
use xcl_vm::Value;

/// I/O operations type.
#[derive(Xcl)]
pub struct Io;

#[xcl_impl(auto_import)]
impl Io {
    /// Print any value to stdout without a trailing newline.
    pub fn print(value: Value) {
        let text = value.as_string();
        let _ = std::io::stdout().write_all(text.as_bytes());
    }

    /// Print any value to stdout with a trailing newline.
    pub fn println(value: Value) {
        let text = value.as_string();
        let _ = std::io::stdout().write_all(text.as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
    }

    /// Read a single line from stdin.
    pub fn readline() -> String {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                line
            }
            Err(_) => String::new(),
        }
    }

    /// Read all remaining input from stdin.
    pub fn read() -> String {
        let mut buf = String::new();
        match std::io::stdin().read_to_string(&mut buf) {
            Ok(_) => buf,
            Err(_) => String::new(),
        }
    }

    /// Write raw text to stdout without flushing. Returns bytes written.
    pub fn write(text: String) -> i64 {
        match std::io::stdout().write_all(text.as_bytes()) {
            Ok(_) => text.len() as i64,
            Err(_) => 0,
        }
    }

    /// Flush the stdout buffer.
    pub fn flush() -> bool {
        std::io::stdout().flush().is_ok()
    }

    /// Display a prompt and read a line from stdin.
    pub fn prompt(prompt: String) -> String {
        let _ = std::io::stdout().write_all(prompt.as_bytes());
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                line
            }
            Err(_) => String::new(),
        }
    }

    /// Print text to stderr with a trailing newline.
    pub fn error(text: String) {
        let _ = std::io::stderr().write_all(text.as_bytes());
        let _ = std::io::stderr().write_all(b"\n");
    }
}
