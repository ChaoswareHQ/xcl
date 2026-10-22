//! Rust-style panic: report the message and terminate with exit code 101.
//!
//! Unlike `defer`, a panic does not run deferred closures — that matches Rust,
//! where unwinding/aborting does not execute Go-style defers.

use std::io::Write;

use xcl_macros::xcl_impl;
use xcl_vm::Value;

use super::Util;

/// Terminates the process with a Rust-style panic message.
///
/// Flushes stdout first so buffered `print` output is not lost, writes
/// `panic: <message>` to stderr, and exits with code 101 (the code a Rust panic
/// uses).
pub(crate) fn panic_now(message: Value) -> ! {
    let _ = std::io::stdout().flush();
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    let _ = err.write_all(b"panic: ");
    let _ = super::print::write_value(&mut err, message);
    let _ = err.write_all(b"\n");
    let _ = err.flush();
    std::process::exit(101)
}

#[xcl_impl(auto_import)]
impl Util {
    /// Abort the script with `message`, in the style of a Rust panic.
    pub fn panic(message: Value) -> Value {
        panic_now(message)
    }
}
