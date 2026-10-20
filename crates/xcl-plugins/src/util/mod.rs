//! `util` — the primitives every XCL script reaches for.
//!
//! Everything here is deliberately small, fast and policy-aware:
//!
//! * `util.print` / `util.input` and their variants — buffered std I/O, no
//!   `print!`-style macros, no intermediate `String` for the common types;
//! * `util.fg_color` / `util.bg_color` — ANSI foreground/background colours,
//!   named or `#rrggbb`;
//! * `util.defer` — the target of the `defer { ... }` statement;
//! * `util.panic` — Rust-style panic with a message and exit code 101;
//! * `util.metadata` / `metadata.get` — the script header (`# name: value` lines);
//! * `util.ok` / `util.err` and friends — the `Result` value;
//! * `rule.allow` / `rule.deny` — the print/input gate.
//!
//! The domain is `util` because the marker type is [`Util`]. Every module adds
//! methods to it through `#[xcl_impl]`, and the ones marked `auto_import` also
//! become bare globals: `print` is `util.print`, `input` is `util.input`, and
//! so on. Both spellings always work.
//!
//! ```xcl
//! print (util.fg_color "BrightCyan" "hello")
//! $name = input "name: "
//! rule.deny "print"
//! ```

use xcl_macros::Xcl;

mod defer;
mod input;
mod metadata;
mod panic;
mod print;
mod result;
mod rule;

pub use metadata::Metadata;
pub(crate) use metadata::install as install_metadata;
pub use result::Result;
pub use rule::Rule;

/// Marker type for the `util` domain.
///
/// It carries no data; it exists so `#[derive(Xcl)]` registers the domain and
/// `#[xcl_impl]` can attach methods to it from several modules.
#[derive(Xcl)]
pub struct Util;
