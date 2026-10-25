//! XCL runtime values, NaN-boxed memory representation, and native bindings.
//!
//! # Overview
//!
//! XCL represents all dynamic runtime values with a 64-bit word ([`Value`]) using **NaN-boxing**.
//!
//! Primitive types (integers, floats, booleans, nil) require zero heap allocation and fit
//! directly inside the 8-byte word. Heap-allocated types (strings, arrays, dictionaries,
//! functions, and OS resource handles) store their pointer addresses in the lower 48 bits
//! of a quiet NaN payload.
//!
//! # NaN-Boxing Layout
//!
//! In the IEEE-754 standard, double-precision quiet NaN (`qNaN`) bit patterns have all
//! exponent bits set to 1 and the highest fraction bit set to 1:
//!
//! ```text
//! 63 62       52 51 50 49 48 47                                              0
//! +-+-----------+--+--+--+--+-------------------------------------------------+
//! |s|11111111111| 1| Tag (4b)|              Payload / Pointer (48 bits)        |
//! +-+-----------+--+--+--+--+-------------------------------------------------+
//! ```
//!
//! * **Floats**: Any standard IEEE-754 64-bit float that does not match the quiet NaN mask.
//!   NaN values are canonicalized.
//! * **Integers (`TAG_INT`)**: Stored as 48-bit two's complement integers (`as_i48` / `from_i48`),
//!   providing exact arithmetic for values in \([-2^{47}, 2^{47}-1]\). If arithmetic exceeds
//!   48 bits, it automatically falls back to 64-bit floating point without throwing an error.
//! * **Booleans**: Encoded with distinct tag bits: `TAG_TRUE` and `TAG_FALSE`.
//! * **Nil**: Encoded with `TAG_NIL` (representing `none` or unit `()`).
//! * **Pointers (`TAG_PTR`)**: Lower 48 bits contain a pointer to a heap-allocated object
//!   beginning with a [`HeapHeader`](crate::value::HeapHeader).
//!
//! # Heap Allocation & Bump Arenas
//!
//! Temporary objects generated during execution (such as intermediate strings, slices,
//! and maps) are allocated from a [`bumpalo::Bump`] arena owned by [`crate::runtime::vm::VM`].
//! This provides microsecond allocation speed and batch cleanup on loop or VM reset.
//!
//! Long-lived objects (such as exported plugins or leaked constants) can be created with
//! static boxed constructors such as [`Value::from_str_boxed`] or [`Value::from_map_boxed`].
//!
//! # Examples
//!
//! Creating and inspecting primitive values:
//!
//! ```
//! use xcl_rt::Value;
//!
//! let num = Value::from_i48(42);
//! assert!(num.is_int());
//! assert_eq!(num.as_i48(), 42);
//!
//! let pi = Value::from_f64(3.14159);
//! assert!(pi.is_float());
//! assert!((pi.as_f64() - 3.14159).abs() < 1e-5);
//!
//! let b = Value::bool(true);
//! assert!(b.is_bool());
//! assert!(b.as_bool());
//!
//! let nil = Value::nil();
//! assert!(nil.is_nil());
//! assert!(!nil.is_truthy());
//! ```

/// Internal map type using hashbrown's fast hasher (always hashbrown, regardless of std feature).
pub type XclMap = hashbrown::HashMap<String, Value>;

#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::format;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::string::String;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::string::ToString;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::vec::Vec;

const QNAN: u64 = 0x7FF8_0000_0000_0000;
const TAG_MASK: u64 = 0x000F_0000_0000_0000;
const PAYLOAD: u64 = 0x0000_FFFF_FFFF_FFFF;

const TAG_NIL: u64 = QNAN;
const TAG_FALSE: u64 = QNAN | (1u64 << 48);
const TAG_TRUE: u64 = QNAN | (2u64 << 48);
/// Bitmask tag indicating a 48-bit integer value in NaN-boxing.
pub const TAG_INT: u64 = QNAN | (3u64 << 48);
const TAG_PTR: u64 = QNAN | (4u64 << 48);

const KIND_STR: u8 = 0;
const KIND_ARRAY: u8 = 1;
const KIND_MAP: u8 = 2;
const KIND_FUNC: u8 = 3;
const KIND_HANDLE: u8 = 6;

/// Subtype constant indicating an untyped or default map.
pub const SUBTYPE_NONE: u16 = 0;
/// Subtype constant indicating a `Result` sum type.
pub const SUBTYPE_RESULT: u16 = 1;
/// Subtype constant indicating an `Error` record type.
pub const SUBTYPE_ERROR: u16 = 2;
/// Subtype constant indicating an OS resource handle.
pub const SUBTYPE_HANDLE: u16 = 3;
/// Subtype constant indicating a numeric range value (`start..end`).
pub const SUBTYPE_RANGE: u16 = 4;
/// Base offset for user-defined custom record and sum types.
pub const SUBTYPE_USER_BASE: u16 = 100;

/// Sum type variant tag for `Ok`.
pub const VARIANT_OK: u8 = 0;
/// Sum type variant tag for `Err`.
pub const VARIANT_ERR: u8 = 1;
/// Default variant tag for types without variants.
pub const VARIANT_DEFAULT: u8 = 0;

/// An 8-byte NaN-boxed dynamic value.
///
/// Encodes primitives inline and heap objects as 48-bit pointers.
/// Transparent wrapper around `u64`.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Value(pub u64);

impl PartialEq for Value {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Value::eq_(*self, *other)
    }
}
impl Eq for Value {}

mod access;
mod arith;
mod boxed;
mod construct;
mod convert;
mod ctx;
mod desc;
mod display;
mod heap;
mod map;
mod native;

pub use self::convert::{IntoValue, NativeArg, extract_arg};
pub use self::ctx::{DomainCtx, GlobalOverrideGuard};
pub use self::desc::*;
pub use self::heap::*;
pub use self::map::*;
pub use self::native::*;
