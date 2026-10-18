//! Deferred execution — Go-style `defer` as a standard library function.
//!
//! Instead of try-catch, XCL uses explicit error handling with Result values
//! and `defer` for cleanup. This follows the Go philosophy:
//!
//! - Errors are normal values, not exceptions
//! - Cleanup code runs at the end of the current scope, even on error paths
//! - `defer` is a built-in function — no VM opcode changes needed
//!
//! # Usage
//!
//! ```xcl
//! // Block-lambda syntax — clean and concise
//! defer { print "cleanup" }
//!
//! // Multiple defers run in reverse order (LIFO)
//! defer { print "third" }
//! defer { print "second" }
//! defer { print "first" }
//! // Output: first second third
//!
//! // Block-lambda with multiple statements
//! $f = {
//!     print "step 1"
//!     print "step 2"
//! }
//! $f ()
//!
//! // Error handling with defer — cleanup always runs
//! $result = do_something()
//! defer {
//!     if $result.is_err {
//!         print "Cleaning up after error"
//!     }
//! }
//! ```

use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

/// Push a closure onto the current frame's defer stack.
/// The closure will be executed when the current function returns,
/// in reverse order of registration (LIFO, like Go's defer).
///
/// Returns nil.
#[xcl_fn(name = "defer")]
fn std_defer(ctx: &mut DomainCtx) -> Value {
    let closure = ctx.arg(0);
    ctx.defer(closure);
    Value::nil()
}
