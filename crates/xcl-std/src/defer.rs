//! Deferred execution — Go-style `defer` as a standard library function.
//!
//! Instead of try-catch, XCL uses explicit error handling with Result values
//! and `std.defer` for cleanup. This follows the Go philosophy:
//!
//! - Errors are normal values, not exceptions
//! - Cleanup code runs at the end of the current scope, even on error paths
//! - `defer` is a function call, not a keyword — no VM opcode changes needed
//!
//! # Usage
//!
//! ```xcl
//! // Basic defer — runs when the current function returns
//! $file = io.open "data.txt"
//! std.defer (fn { io.close $file })
//!
//! // Multiple defers run in reverse order (LIFO)
//! std.defer (fn { io.print "third" })
//! std.defer (fn { io.print "second" })
//! std.defer (fn { io.print "first" })
//! // Output: first second third
//!
//! // Error handling with defer — cleanup always runs
//! $result = do_something()
//! std.defer (fn {
//!     if $result.is_err {
//!         io.print "Cleaning up after error"
//!     }
//! })
//! ```

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Push a closure onto the current frame's defer stack.
/// The closure will be executed when the current function returns,
/// in reverse order of registration (LIFO, like Go's defer).
///
/// Returns nil.
#[xcl_fn(
    name = "std.defer",
    args = "f: any",
    ret = "nil",
    summary = "Schedule a closure to run when the current function returns. \
               Multiple defers execute in reverse (LIFO) order."
)]
fn std_defer(ctx: &mut DomainCtx) -> Value {
    let closure = ctx.arg(0);
    ctx.defer(closure);
    Value::nil()
}
