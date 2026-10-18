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
//! and maps) are allocated from a [`bumpalo::Bump`] arena owned by [`crate::vm::VM`].
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
//! use xcl_vm::Value;
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

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

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
/// Base offset for user-defined custom record and sum types.
pub const SUBTYPE_USER_BASE: u16 = 100;

/// Sum type variant tag for `Ok`.
pub const VARIANT_OK: u8 = 0;
/// Sum type variant tag for `Err`.
pub const VARIANT_ERR: u8 = 1;
/// Default variant tag for types without variants.
pub const VARIANT_DEFAULT: u8 = 0;

/// Header prepended to every heap-allocated XCL value.
///
/// Uses a packed memory layout (`#[repr(C, packed)]`) of 8 bytes to eliminate
/// padding and ensure instant subtype/variant inspection.
#[repr(C, packed)]
pub struct HeapHeader {
    /// Object kind tag (e.g., string, array, map, func, handle).
    pub kind: u8,
    /// Internal GC/status flags.
    pub flags: u8,
    /// Type subtype identifier (e.g. [`SUBTYPE_RESULT`], [`SUBTYPE_ERROR`]).
    pub subtype: u16,
    /// Variant identifier (e.g. [`VARIANT_OK`], [`VARIANT_ERR`]).
    pub variant: u8,
}

/// Heap-allocated UTF-8 string object.
#[repr(C)]
pub struct HeapStr {
    /// Common object header.
    pub header: HeapHeader,
    /// UTF-8 string buffer.
    pub data: String,
}

/// Heap-allocated array of dynamic values.
#[repr(C)]
pub struct HeapArray {
    /// Common object header.
    pub header: HeapHeader,
    /// Contiguous element buffer.
    pub data: Vec<Value>,
}

/// Heap-allocated key-value dictionary map.
#[repr(C)]
pub struct HeapMap {
    /// Common object header.
    pub header: HeapHeader,
    /// Hash map storage for string keys and dynamic values.
    pub data: HashMap<String, Value>,
}

/// Heap-allocated function or closure value.
#[repr(C)]
pub struct HeapFunc {
    /// Common object header.
    pub header: HeapHeader,
    /// Function execution payload and environment captures.
    pub data: FuncValue,
}

/// Heap-allocated wrapper around an external OS resource handle.
#[repr(C)]
pub struct HeapHandle {
    /// Common object header.
    pub header: HeapHeader,
    /// Optional underlying handle resource (empty if already closed).
    pub data: Option<HandleKind>,
}

/// Kinds of operating system resources that can be wrapped by an XCL handle.
#[derive(Debug)]
pub enum HandleKind {
    /// An open file with its path recorded for diagnostics or reopening.
    #[cfg(feature = "std")]
    File {
        /// Standard library file handle.
        file: std::fs::File,
        /// Original filesystem path.
        path: String,
    },
    /// An open file descriptor (raw OS handle) for `no_std` environments.
    #[cfg(not(feature = "std"))]
    File {
        /// Raw file descriptor pointer.
        fd: RawPtr,
        /// Original filesystem path.
        path: String,
    },
    /// Standard input stream.
    Stdin,
    /// Standard output stream.
    Stdout,
    /// Standard error stream.
    Stderr,
    /// A spawned child process.
    #[cfg(feature = "std")]
    Process {
        /// Standard library child process handle.
        child: std::process::Child,
        /// Process identifier (PID).
        pid: u32,
    },
    /// A spawned child process for `no_std` environments.
    #[cfg(not(feature = "std"))]
    Process {
        /// Raw process handle.
        handle: RawPtr,
        /// Process identifier (PID).
        pid: u32,
    },
}

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

impl Value {
    /// Constructs a `nil` (`none` / unit) value.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Value;
    ///
    /// let v = Value::nil();
    /// assert!(v.is_nil());
    /// ```
    #[inline(always)]
    pub const fn nil() -> Self {
        Value(TAG_NIL)
    }

    /// Constructs a boolean value (`true` or `false`).
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Value;
    ///
    /// let t = Value::bool(true);
    /// let f = Value::bool(false);
    /// assert_eq!(t.as_bool(), true);
    /// assert_eq!(f.as_bool(), false);
    /// ```
    #[inline(always)]
    pub fn bool(b: bool) -> Self {
        if b { Value(TAG_TRUE) } else { Value(TAG_FALSE) }
    }

    /// Constructs a value from a 64-bit float.
    ///
    /// If `f` is NaN, canonicalizes it to avoid collision with quiet NaN pointer tags.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Value;
    ///
    /// let v = Value::from_f64(2.718);
    /// assert!(v.is_float());
    /// assert!((v.as_f64() - 2.718).abs() < 1e-6);
    /// ```
    #[inline(always)]
    pub fn from_f64(f: f64) -> Self {
        Value(f.to_bits())
    }

    /// Constructs a value from a 48-bit signed integer.
    ///
    /// Truncates the high 16 bits to fit within the lower 48-bit payload.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Value;
    ///
    /// let v = Value::from_i48(-12345);
    /// assert!(v.is_int());
    /// assert_eq!(v.as_i48(), -12345);
    /// ```
    #[inline(always)]
    pub fn from_i48(i: i64) -> Self {
        Value(TAG_INT | ((i as u64) & PAYLOAD))
    }

    /// Allocates a string in the provided bump arena and returns a [`Value`] reference to it.
    ///
    /// # Arguments
    ///
    /// * `s` - String to store on the heap.
    /// * `arena` - Target bump arena for allocation.
    #[inline(always)]
    pub fn from_str(s: String, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapStr {
            header: HeapHeader {
                kind: KIND_STR,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: s,
        });
        Value(TAG_PTR | ((obj as *const HeapStr as u64) & PAYLOAD))
    }

    /// Allocates an array in the provided bump arena and returns a [`Value`] reference to it.
    ///
    /// # Arguments
    ///
    /// * `elements` - Vector of values to store on the heap.
    /// * `arena` - Target bump arena for allocation.
    #[inline(always)]
    pub fn from_array(elements: Vec<Value>, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapArray {
            header: HeapHeader {
                kind: KIND_ARRAY,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: elements,
        });
        Value(TAG_PTR | ((obj as *const HeapArray as u64) & PAYLOAD))
    }

    /// Allocates a dictionary map in the provided bump arena and returns a [`Value`] reference to it.
    ///
    /// # Arguments
    ///
    /// * `m` - Hash map of keys and values.
    /// * `arena` - Target bump arena for allocation.
    #[inline(always)]
    pub fn from_map(m: HashMap<String, Value>, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapMap {
            header: HeapHeader {
                kind: KIND_MAP,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: m,
        });
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }

    /// Allocates a function/closure in the provided bump arena and returns a [`Value`] reference to it.
    ///
    /// # Arguments
    ///
    /// * `f` - Function descriptor and closure environment.
    /// * `arena` - Target bump arena for allocation.
    #[inline(always)]
    pub fn from_func(f: FuncValue, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapFunc {
            header: HeapHeader {
                kind: KIND_FUNC,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: f,
        });
        Value(TAG_PTR | ((obj as *const HeapFunc as u64) & PAYLOAD))
    }

    /// Returns `true` if this value is an IEEE-754 64-bit float.
    #[inline(always)]
    pub fn is_float(self) -> bool {
        self.0 & QNAN != QNAN
    }

    /// Extracts the quiet NaN tag mask bits from the raw representation.
    #[inline(always)]
    pub fn tag(self) -> u64 {
        self.0 & (QNAN | TAG_MASK)
    }

    /// Returns `true` if this value is `nil` (`none`).
    #[inline(always)]
    pub fn is_nil(self) -> bool {
        self.tag() == TAG_NIL
    }

    /// Returns `true` if this value is a boolean (`true` or `false`).
    #[inline(always)]
    pub fn is_bool(self) -> bool {
        self.0 == TAG_TRUE || self.0 == TAG_FALSE
    }

    /// Returns `true` if this value is a 48-bit integer.
    #[inline(always)]
    pub fn is_int(self) -> bool {
        self.tag() == TAG_INT
    }

    /// Returns `true` if this value is a pointer to a heap-allocated object.
    #[inline(always)]
    pub fn is_ptr(self) -> bool {
        self.tag() == TAG_PTR
    }

    /// Determines whether this value is considered "truthy" in conditional expressions.
    ///
    /// In XCL, only `nil` and `false` are falsy; all other values (including `0`, `""`, and `[]`) are truthy.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::Value;
    ///
    /// assert!(!Value::nil().is_truthy());
    /// assert!(!Value::bool(false).is_truthy());
    /// assert!(Value::bool(true).is_truthy());
    /// assert!(Value::from_i48(0).is_truthy());
    /// ```
    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        !matches!(self.tag(), TAG_NIL | TAG_FALSE)
    }

    /// Extracts the floating point number from this value.
    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// Decodes the 48-bit signed integer value with sign extension.
    #[inline(always)]
    pub fn as_i48(self) -> i64 {
        (self.0 as i64) << 16 >> 16
    }

    /// Returns the boolean value.
    #[inline(always)]
    pub fn as_bool(self) -> bool {
        self.0 == TAG_TRUE
    }

    /// Extracts the raw heap pointer address encoded in the lower 48 bits.
    #[inline(always)]
    pub fn as_ptr<T>(self) -> *const T {
        (self.0 & PAYLOAD) as *const T
    }

    /// Returns the object kind byte from the heap header if this value is a heap pointer.
    #[inline(always)]
    pub fn heap_kind(self) -> Option<u8> {
        if self.is_ptr() {
            Some(unsafe { *((self.0 & PAYLOAD) as *const u8) })
        } else {
            None
        }
    }

    /// Returns a static reference to the underlying array vector if this value is an array.
    #[inline(always)]
    pub fn as_array(self) -> Option<&'static Vec<Value>> {
        if self.heap_kind() == Some(KIND_ARRAY) {
            Some(unsafe { &(*(self.as_ptr::<HeapArray>())).data })
        } else {
            None
        }
    }

    /// Returns a mutable static reference to the underlying array vector if this value is an array.
    #[inline(always)]
    pub fn as_array_mut(self) -> Option<&'static mut Vec<Value>> {
        if self.heap_kind() == Some(KIND_ARRAY) {
            Some(unsafe { &mut (*(self.as_ptr::<HeapArray>() as *mut HeapArray)).data })
        } else {
            None
        }
    }

    /// Returns a static reference to the underlying dictionary map if this value is a map.
    #[inline(always)]
    pub fn as_map(self) -> Option<&'static HashMap<String, Value>> {
        if self.heap_kind() == Some(KIND_MAP) {
            Some(unsafe { &(*(self.as_ptr::<HeapMap>())).data })
        } else {
            None
        }
    }

    /// Retrieves the subtype identifier from the heap map's header (e.g. [`SUBTYPE_RESULT`]).
    #[inline(always)]
    pub fn map_subtype(self) -> Option<u16> {
        if self.heap_kind() == Some(KIND_MAP) {
            let ptr = self.as_ptr::<HeapMap>();
            Some(unsafe { (*ptr).header.subtype })
        } else {
            None
        }
    }

    /// Retrieves the variant tag from the heap map's header (e.g. [`VARIANT_OK`], [`VARIANT_ERR`]).
    #[inline(always)]
    pub fn map_variant(self) -> Option<u8> {
        if self.heap_kind() == Some(KIND_MAP) {
            let ptr = self.as_ptr::<HeapMap>();
            Some(unsafe { (*ptr).header.variant })
        } else {
            None
        }
    }

    /// Returns a string slice if this value is a heap-allocated string.
    #[inline(always)]
    pub fn as_str(self) -> Option<&'static str> {
        if self.heap_kind() == Some(KIND_STR) {
            Some(unsafe { &(*(self.as_ptr::<HeapStr>())).data })
        } else {
            None
        }
    }

    /// Returns a reference to the function value if this value is a heap-allocated function.
    #[inline(always)]
    pub fn as_func(self) -> Option<&'static FuncValue> {
        if self.heap_kind() == Some(KIND_FUNC) {
            Some(unsafe { &(*(self.as_ptr::<HeapFunc>())).data })
        } else {
            None
        }
    }

    /// Fast integer addition when both operands are known to be integers.
    ///
    /// Performs 48-bit addition.
    #[inline(always)]
    pub fn add_int(self, other: Self) -> Self {
        Self::from_i48(self.as_i48().wrapping_add(other.as_i48()))
    }

    /// Fast integer multiplication when both operands are known to be integers.
    #[inline(always)]
    pub fn mul_int(self, other: Self) -> Self {
        let a = self.as_i48();
        let b = other.as_i48();
        if let Some(r) = a.checked_mul(b) {
            Self::from_i48(r)
        } else {
            Self::from_f64(a as f64 * b as f64)
        }
    }

    /// Fast integer subtraction when both operands are known to be integers.
    #[inline(always)]
    pub fn sub_int(self, other: Self) -> Self {
        Self::from_i48(self.as_i48().wrapping_sub(other.as_i48()))
    }

    /// Fast integer division when both operands are known to be integers.
    #[inline(always)]
    pub fn div_int(self, other: Self) -> Self {
        let a = self.as_i48();
        let b = other.as_i48();
        if b != 0 {
            Self::from_i48(a.wrapping_div(b))
        } else {
            Self::from_f64(f64::INFINITY)
        }
    }

    /// Fast integer modulo when both operands are known to be integers.
    #[inline(always)]
    pub fn mod_int(self, other: Self) -> Self {
        let a = self.as_i48();
        let b = other.as_i48();
        if b != 0 {
            Self::from_i48(a.wrapping_rem(b))
        } else {
            Self::nil()
        }
    }

    /// Fast integer equality comparison without tag inspection.
    #[inline(always)]
    pub fn eq_int(self, other: Self) -> bool {
        self.0 == other.0
    }

    /// Fast integer less-than comparison without tag inspection.
    #[inline(always)]
    pub fn lt_int(self, other: Self) -> bool {
        ((self.0 as i64) << 16) < ((other.0 as i64) << 16)
    }

    /// Fast integer greater-than comparison without tag inspection.
    #[inline(always)]
    pub fn gt_int(self, other: Self) -> bool {
        ((self.0 as i64) << 16) > ((other.0 as i64) << 16)
    }

    /// Generic addition operator with dynamic type dispatch.
    ///
    /// Supports integers, floats, array concatenation, and string concatenation.
    #[inline(always)]
    pub fn add(self, other: Self, arena: &bumpalo::Bump) -> Self {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.add_int(other);
        }
        if self.is_float() && other.is_float() {
            return Self::from_f64(self.as_f64() + other.as_f64());
        }
        if (self.is_float() && other.is_int()) || (self.is_int() && other.is_float()) {
            return Self::from_f64(self.as_float() + other.as_float());
        }
        Self::add_slow(self, other, arena)
    }

    #[cold]
    #[inline(never)]
    fn add_slow(self, other: Self, arena: &bumpalo::Bump) -> Self {
        if self.heap_kind() == Some(KIND_ARRAY) && other.heap_kind() == Some(KIND_ARRAY) {
            let a = unsafe { &(*(self.as_ptr::<HeapArray>())).data };
            let b = unsafe { &(*(other.as_ptr::<HeapArray>())).data };
            let mut v = Vec::with_capacity(a.len() + b.len());
            v.extend_from_slice(a);
            v.extend_from_slice(b);
            return Self::from_array(v, arena);
        }
        if self.tag() == TAG_PTR || other.tag() == TAG_PTR {
            let a = self.as_string();
            let b = other.as_string();
            return Self::from_str(format!("{a}{b}"), arena);
        }
        Self::nil()
    }

    /// Generic subtraction operator with dynamic type dispatch.
    #[doc(alias("sub"))]
    #[inline(always)]
    pub fn sub_(self, other: Self) -> Self {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.sub_int(other);
        }
        if self.is_float() && other.is_float() {
            return Self::from_f64(self.as_f64() - other.as_f64());
        }
        Self::from_f64(self.as_float() - other.as_float())
    }

    /// Generic multiplication operator with dynamic type dispatch.
    ///
    /// Supports numeric multiplication and string repetition (e.g. `"a" * 3`).
    #[inline(always)]
    pub fn mul(self, other: Self, arena: &bumpalo::Bump) -> Self {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.mul_int(other);
        }
        if self.is_float() && other.is_float() {
            return Self::from_f64(self.as_f64() * other.as_f64());
        }
        if (self.is_float() && other.is_int()) || (self.is_int() && other.is_float()) {
            return Self::from_f64(self.as_float() * other.as_float());
        }
        Self::mul_slow(self, other, arena)
    }

    #[cold]
    #[inline(never)]
    fn mul_slow(self, other: Self, arena: &bumpalo::Bump) -> Self {
        if self.tag() == TAG_PTR && other.is_int() {
            return Self::from_str(self.as_string().repeat(other.as_i48() as usize), arena);
        }
        if other.tag() == TAG_PTR && self.is_int() {
            return Self::from_str(other.as_string().repeat(self.as_i48() as usize), arena);
        }
        Self::nil()
    }

    /// Generic division operator with dynamic type dispatch.
    #[doc(alias("div"))]
    #[inline(always)]
    pub fn div_(self, other: Self) -> Self {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.div_int(other);
        }
        if self.is_float() && other.is_float() {
            return Self::from_f64(self.as_f64() / other.as_f64());
        }
        Self::from_f64(self.as_float() / other.as_float())
    }

    /// Generic modulo remainder operator with dynamic type dispatch.
    #[inline(always)]
    pub fn mod_(self, other: Self) -> Self {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.mod_int(other);
        }
        if self.is_float() || other.is_float() {
            return Self::from_f64(self.as_float() % other.as_float());
        }
        Self::nil()
    }

    /// Generic equality operator with tag-based early exit and string comparison.
    #[doc(alias("eq"))]
    #[inline(always)]
    pub fn eq_(self, other: Self) -> bool {
        if self.0 == other.0 {
            return true;
        }
        if self.is_float() && other.is_float() {
            return self.as_f64() == other.as_f64();
        }
        Self::eq_slow(self, other)
    }

    #[cold]
    #[inline(never)]
    fn eq_slow(self, other: Self) -> bool {
        if self.is_int() && other.is_float() {
            return self.as_i48() as f64 == other.as_f64();
        }
        if self.is_float() && other.is_int() {
            return self.as_f64() == other.as_i48() as f64;
        }
        if self.is_bool() && other.is_bool() {
            return self.as_bool() == other.as_bool();
        }
        if self.tag() == TAG_PTR && other.tag() == TAG_PTR {
            if (self.0 & PAYLOAD) == (other.0 & PAYLOAD) {
                return true;
            }
            return self.as_string() == other.as_string();
        }
        false
    }

    /// Generic less-than operator with dynamic type dispatch.
    #[inline(always)]
    pub fn lt(self, other: Self) -> bool {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.lt_int(other);
        }
        if self.is_float() && other.is_float() {
            return self.as_f64() < other.as_f64();
        }
        Self::lt_slow(self, other)
    }

    #[cold]
    #[inline(never)]
    fn lt_slow(self, other: Self) -> bool {
        if self.is_int() && other.is_float() {
            return (self.as_i48() as f64) < other.as_f64();
        }
        if self.is_float() && other.is_int() {
            return self.as_f64() < (other.as_i48() as f64);
        }
        if self.tag() == TAG_PTR && other.tag() == TAG_PTR {
            return self.as_string() < other.as_string();
        }
        false
    }

    /// Generic greater-than operator with dynamic type dispatch.
    #[inline(always)]
    pub fn gt(self, other: Self) -> bool {
        if self.tag() == TAG_INT && other.tag() == TAG_INT {
            return self.gt_int(other);
        }
        if self.is_float() && other.is_float() {
            return self.as_f64() > other.as_f64();
        }
        Self::gt_slow(self, other)
    }

    #[cold]
    #[inline(never)]
    fn gt_slow(self, other: Self) -> bool {
        if self.is_int() && other.is_float() {
            return (self.as_i48() as f64) > other.as_f64();
        }
        if self.is_float() && other.is_int() {
            return self.as_f64() > (other.as_i48() as f64);
        }
        if self.tag() == TAG_PTR && other.tag() == TAG_PTR {
            return self.as_string() > other.as_string();
        }
        false
    }

    /// Logical NOT operator (`!`).
    #[doc(alias("not"))]
    #[inline(always)]
    pub fn not_(self) -> Self {
        Self::bool(!self.is_truthy())
    }

    /// Arithmetic negation operator (`-x`).
    #[doc(alias("neg"))]
    #[inline(always)]
    pub fn neg_(self) -> Self {
        if self.is_int() {
            Self::from_i48(-self.as_i48())
        } else if self.is_float() {
            Self::from_f64(-self.as_f64())
        } else {
            self
        }
    }

    /// Returns the length of the string, array, or map; or 0 for primitive values.
    pub fn len(self) -> usize {
        match self.heap_kind() {
            Some(KIND_STR) => unsafe { (*(self.as_ptr::<HeapStr>())).data.chars().count() },
            Some(KIND_ARRAY) => unsafe { (*(self.as_ptr::<HeapArray>())).data.len() },
            Some(KIND_MAP) => unsafe { (*(self.as_ptr::<HeapMap>())).data.len() },
            _ => 0,
        }
    }

    /// Returns `true` if the value has a length of 0.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Formats the value into a human-readable string representation.
    pub fn as_string(self) -> String {
        if self.is_nil() {
            return "none".into();
        }
        if self.is_bool() {
            return self.as_bool().to_string();
        }
        if self.is_int() {
            return self.as_i48().to_string();
        }
        if self.is_float() {
            return self.as_f64().to_string();
        }
        match self.heap_kind() {
            Some(KIND_STR) => unsafe { (*(self.as_ptr::<HeapStr>())).data.clone() },
            Some(KIND_FUNC) => {
                format!("<fn {}>", unsafe {
                    (*(self.as_ptr::<HeapFunc>())).data.name.clone()
                })
            }
            Some(KIND_ARRAY) => {
                let v = unsafe { &(*(self.as_ptr::<HeapArray>())).data };
                let items: Vec<String> = v.iter().map(|x| x.as_string()).collect();
                format!("[{}]", items.join(", "))
            }
            Some(KIND_MAP) => {
                let m = unsafe { &(*(self.as_ptr::<HeapMap>())).data };
                let items: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.as_string()))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            _ => "unknown".into(),
        }
    }

    /// Converts the value to an integer if possible; otherwise returns 0.
    #[inline(always)]
    pub fn as_int(self) -> i64 {
        if self.is_int() {
            self.as_i48()
        } else if self.is_float() {
            self.as_f64() as i64
        } else {
            0
        }
    }

    /// Converts the value to a float if possible; otherwise returns 0.0.
    #[inline(always)]
    pub fn as_float(self) -> f64 {
        if self.is_float() {
            self.as_f64()
        } else if self.is_int() {
            self.as_i48() as f64
        } else {
            0.0
        }
    }

    /// Wraps an OS resource handle in a static leaked heap object.
    pub fn from_handle(h: HandleKind) -> Self {
        let obj = Box::leak(Box::new(HeapHandle {
            header: HeapHeader {
                kind: KIND_HANDLE,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: Some(h),
        }));
        Value(TAG_PTR | ((obj as *const HeapHandle as u64) & PAYLOAD))
    }

    /// Returns a reference to the wrapped OS handle if this value is a handle.
    pub fn as_handle(&self) -> Option<&HandleKind> {
        if self.heap_kind() == Some(KIND_HANDLE) {
            unsafe { (*(self.as_ptr::<HeapHandle>())).data.as_ref() }
        } else {
            None
        }
    }

    /// Returns a mutable reference to the wrapped OS handle.
    #[allow(clippy::mut_from_ref)]
    pub fn as_handle_mut(&self) -> Option<&mut HandleKind> {
        if self.heap_kind() == Some(KIND_HANDLE) {
            unsafe {
                (*(self.as_ptr::<HeapHandle>() as *mut HeapHandle))
                    .data
                    .as_mut()
            }
        } else {
            None
        }
    }

    /// Consumes and extracts the wrapped OS handle from the heap value.
    pub fn take_handle(self) -> Option<HandleKind> {
        if self.heap_kind() == Some(KIND_HANDLE) {
            let heap = unsafe { &mut *(self.as_ptr::<HeapHandle>() as *mut HeapHandle) };
            heap.data.take()
        } else {
            None
        }
    }

    /// Allocates a string with static lifetime (leaked Box) suitable for constants.
    pub fn from_str_boxed(s: String) -> Self {
        let obj = Box::leak(Box::new(HeapStr {
            header: HeapHeader {
                kind: KIND_STR,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: s,
        }));
        Value(TAG_PTR | ((obj as *const HeapStr as u64) & PAYLOAD))
    }

    /// Allocates an array with static lifetime (leaked Box).
    pub fn from_array_boxed(elements: Vec<Value>) -> Self {
        let obj = Box::leak(Box::new(HeapArray {
            header: HeapHeader {
                kind: KIND_ARRAY,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: elements,
        }));
        Value(TAG_PTR | ((obj as *const HeapArray as u64) & PAYLOAD))
    }

    /// Allocates a map with static lifetime (leaked Box).
    pub fn from_map_boxed(m: HashMap<String, Value>) -> Self {
        Self::from_map_boxed_typed_v(m, SUBTYPE_NONE, VARIANT_DEFAULT)
    }

    /// Creates a typed heap map with specified subtype and variant tags, leaked to static lifetime.
    pub fn from_map_boxed_typed_v(m: HashMap<String, Value>, subtype: u16, variant: u8) -> Self {
        let obj = Box::leak(Box::new(HeapMap {
            header: HeapHeader {
                kind: KIND_MAP,
                flags: 0,
                subtype,
                variant,
            },
            data: m,
        }));
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }

    /// Returns a unit / nil value.
    pub fn unit() -> Self {
        Value(TAG_NIL)
    }

    /// Alias for [`Value::bool`].
    pub fn bool_(b: bool) -> Self {
        Value::bool(b)
    }
}

impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

// — Argument extraction for typed native functions —

/// Extracts a typed Rust argument from a `&[Value]` slice at the given index.
///
/// If `index` is out of bounds, returns the default value for `T`.
///
/// # Examples
///
/// ```
/// use xcl_vm::value::{extract_arg, Value};
///
/// let args = [Value::from_i48(100), Value::bool(true)];
/// let first: i64 = extract_arg(&args, 0);
/// let second: bool = extract_arg(&args, 1);
///
/// assert_eq!(first, 100);
/// assert_eq!(second, true);
/// ```
pub fn extract_arg<T: NativeArg>(args: &[Value], index: usize) -> T {
    let v = args.get(index).copied().unwrap_or(Value::nil());
    T::from_value(v)
}

/// Trait for types that can be automatically converted from a [`Value`].
pub trait NativeArg: Sized {
    /// Converts a [`Value`] into `Self`.
    fn from_value(v: Value) -> Self;
}

impl NativeArg for &str {
    fn from_value(v: Value) -> Self {
        v.as_str().unwrap_or("")
    }
}

impl NativeArg for String {
    fn from_value(v: Value) -> Self {
        v.as_str().unwrap_or("").to_string()
    }
}

impl NativeArg for i64 {
    fn from_value(v: Value) -> Self {
        v.as_i48()
    }
}

impl NativeArg for i32 {
    fn from_value(v: Value) -> Self {
        v.as_i48() as i32
    }
}

impl NativeArg for u64 {
    fn from_value(v: Value) -> Self {
        v.as_i48() as u64
    }
}

impl NativeArg for f64 {
    fn from_value(v: Value) -> Self {
        v.as_f64()
    }
}

impl NativeArg for bool {
    fn from_value(v: Value) -> Self {
        v.as_bool()
    }
}

impl NativeArg for Value {
    fn from_value(v: Value) -> Self {
        v
    }
}

impl NativeArg for usize {
    fn from_value(v: Value) -> Self {
        v.as_i48() as usize
    }
}

/// Executable function descriptor containing bytecode and closure environment.
#[derive(Clone, Debug)]
pub struct FuncValue {
    /// Function identifier name.
    pub name: String,
    /// Compiled bytecode chunk for user-defined functions (`None` for native functions).
    pub bytecode: Option<Arc<crate::bytecode::Bytecode>>,
    /// Formal parameter names.
    pub params: Vec<String>,
    /// Captured variable names for closures.
    pub closure_captures: Vec<String>,
    /// Captured environment bindings mapping name to value.
    pub closure_env: Vec<(String, Value)>,
}

impl PartialEq for FuncValue {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

// — Native function registry types —

/// Static descriptor for a registered native function.
pub struct NativeEntry {
    /// Domain name (e.g., `"fs"`, `"math"`), or `None` for global functions.
    pub domain: Option<&'static str>,
    /// Impl type name this function belongs to (e.g., `"CloudflareProvider"`), or `None` for generic functions.
    pub impl_type: Option<&'static str>,
    /// Trait method name this function overrides (e.g., `"bucket_config"`), or `None` if not overriding.
    pub override_method: Option<&'static str>,
    /// Method identifier name.
    pub method: &'static str,
    /// Function execution style ([`NativeKind::Pure`] or [`NativeKind::Ctx`]).
    pub kind: NativeKind,
    /// Function pointer address.
    pub ptr: RawPtr,
    /// Human-readable argument signature.
    pub args: Option<&'static str>,
    /// Return type signature string.
    pub ret: Option<&'static str>,
    /// Named parameter specifications.
    pub named: &'static [NamedParam],
    /// Documentation summary string.
    pub summary: Option<&'static str>,
    /// Description pairs for each argument: `(name, description)`.
    pub arg_desc: &'static [(&'static str, &'static str)],
}

/// A wrapper around a raw function pointer that implements [`Sync`] and [`Send`].
#[derive(Debug, Clone, Copy)]
pub struct RawPtr(pub *const ());

unsafe impl Sync for RawPtr {}
unsafe impl Send for RawPtr {}

/// Descriptor for a function parameter that accepts named arguments with a default value.
pub struct NamedParam {
    /// Parameter identifier name.
    pub name: &'static str,
    /// Type name string.
    pub ty: &'static str,
    /// Default value representation as code.
    pub default: &'static str,
}

/// Calling convention kind for native functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeKind {
    /// Pure function receiving an argument slice: `fn(&[Value]) -> Value`.
    Pure,
    /// Context-aware function receiving mutable [`DomainCtx`]: `fn(&mut DomainCtx) -> Value`.
    Ctx,
}

/// Function pointer type for pure native functions.
pub type BuiltinFn = fn(&[Value]) -> Value;
/// Function pointer type for context-aware native functions.
pub type BuiltinCtxFn = fn(&mut DomainCtx<'_>) -> Value;

/// Function implementation wrapper storing either a pure or context-aware function pointer.
#[derive(Clone, Copy)]
pub enum BuiltinImpl {
    /// Pure function pointer.
    Pure(BuiltinFn),
    /// Context-aware function pointer.
    Ctx(BuiltinCtxFn),
}

impl BuiltinImpl {
    /// Executes the underlying native function using the provided VM and arguments.
    #[inline]
    pub fn call(&self, vm: &mut crate::vm::VM, args: &[Value]) -> Value {
        match self {
            BuiltinImpl::Pure(f) => f(args),
            BuiltinImpl::Ctx(f) => {
                let mut ctx = DomainCtx { vm, args };
                f(&mut ctx)
            }
        }
    }
}

// — Type descriptor types —

/// Static metadata describing a registered custom type.
pub struct TypeDescriptor {
    /// Type identifier name.
    pub name: &'static str,
    /// Domain owning this type, or `None` if globally scoped.
    pub domain: Option<&'static str>,
    /// Kind of type ([`TypeKind`]).
    pub kind: TypeKind,
    /// Field specifications.
    pub fields: &'static [TypeField],
    /// Variant specifications for sum types.
    pub variants: &'static [TypeVariant],
    /// Documentation summary string.
    pub summary: Option<&'static str>,
}

/// Classification of custom types in the type registry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeKind {
    /// Opaque handle type.
    Opaque,
    /// Record struct with fields.
    Record,
    /// Tagged sum type / enum.
    Sum,
    /// Type alias.
    Alias,
}

/// Field descriptor in a custom type definition.
pub struct TypeField {
    /// Field identifier name.
    pub name: &'static str,
    /// Type name string.
    pub ty: &'static str,
    /// Whether the field is optional.
    pub optional: bool,
}

/// Variant descriptor in a sum type definition.
pub struct TypeVariant {
    /// Variant identifier name.
    pub name: &'static str,
    /// Fields contained within this variant.
    pub fields: &'static [TypeField],
}

// — Impl descriptor types —

/// Static metadata describing a registered trait interface.
pub struct TraitDescriptor {
    /// Trait identifier name.
    pub name: &'static str,
    /// Domain namespace, or `None` for global traits.
    pub domain: Option<&'static str>,
    /// List of method signatures required by this trait.
    pub methods: &'static [TraitMethod],
    /// Documentation summary string.
    pub summary: Option<&'static str>,
}

/// Method signature in a trait definition.
pub struct TraitMethod {
    /// Method identifier name.
    pub name: &'static str,
    /// Argument signature string (e.g., `"key: str; value: str"`).
    pub args: Option<&'static str>,
    /// Return type signature string (e.g., `"Result"`).
    pub ret: Option<&'static str>,
    /// Method documentation summary.
    pub summary: Option<&'static str>,
}

/// Metadata describing a trait implementation block.
pub struct ImplDescriptor {
    /// Name of the implementing type.
    pub type_name: &'static str,
    /// Name of the implemented trait.
    pub trait_name: &'static str,
    /// Domain namespace.
    pub domain: Option<&'static str>,
    /// List of implemented methods.
    pub methods: &'static [ImplMethod],
}

/// Method descriptor within a trait implementation block.
pub struct ImplMethod {
    /// Method name.
    pub name: &'static str,
    /// Argument signature string.
    pub args: Option<&'static str>,
    /// Return type signature string.
    pub ret: Option<&'static str>,
    /// Method documentation summary.
    pub summary: Option<&'static str>,
}

/// Context passed to context-aware native functions ([`NativeKind::Ctx`]).
///
/// Provides access to the VM instance, arguments, memory arena, context stack, and defer queue.
pub struct DomainCtx<'a> {
    pub(crate) vm: &'a mut crate::vm::VM,
    pub(crate) args: &'a [Value],
}

impl<'a> DomainCtx<'a> {
    /// Returns the slice of arguments passed to the function.
    pub fn args(&self) -> &[Value] {
        self.args
    }

    /// Returns the number of arguments passed.
    pub fn argc(&self) -> usize {
        self.args.len()
    }

    /// Retrieves the argument at index `i`, or unit/nil if out of bounds.
    pub fn arg(&self, i: usize) -> Value {
        self.args.get(i).copied().unwrap_or(Value::unit())
    }

    /// Returns a reference to the VM's bump allocation arena.
    pub fn arena(&self) -> &bumpalo::Bump {
        &self.vm.arena
    }

    /// Constructs a unit/nil value.
    pub fn unit(&self) -> Value {
        Value::unit()
    }

    /// Constructs a boolean value.
    pub fn boolean(&self, b: bool) -> Value {
        Value::bool_(b)
    }

    /// Allocates a string with static lifetime.
    pub fn string(&self, s: String) -> Value {
        Value::from_str_boxed(s)
    }

    /// Allocates an array with static lifetime.
    pub fn array(&self, v: Vec<Value>) -> Value {
        Value::from_array_boxed(v)
    }

    /// Constructs an integer value from `i64`.
    pub fn i64(&self, n: i64) -> Value {
        Value::from_i48(n)
    }

    /// Constructs an integer value from `u64`.
    pub fn u64(&self, n: u64) -> Value {
        Value::from_i48(n as i64)
    }

    /// Reentrantly calls an XCL closure or function value with the given arguments.
    pub fn call(&mut self, func: Value, args: &[Value]) -> Value {
        self.vm.call_value(func, args)
    }

    /// Pushes a deferred closure onto the current execution frame's defer stack.
    pub fn defer(&mut self, closure: Value) {
        self.vm.push_defer(closure);
    }

    /// Pushes a value onto the domain context stack for the given key.
    ///
    /// Enables nested domain blocks: `browser "chrome" { browser "firefox" { ... } }`.
    pub fn push_context(&mut self, key: &str, value: Value) {
        self.vm
            .context_stack
            .entry(key.to_string())
            .or_insert_with(Vec::new)
            .push(value);
    }

    /// Retrieves the active context value for the given key (top of the stack).
    pub fn get_context(&self, key: &str) -> Option<Value> {
        self.vm
            .context_stack
            .get(key)
            .and_then(|stack| stack.last().copied())
    }

    /// Pops the active context value for the given key.
    pub fn pop_context(&mut self, key: &str) -> Option<Value> {
        self.vm
            .context_stack
            .get_mut(key)
            .and_then(|stack| stack.pop())
    }

    /// Checks if a context key currently has values on its stack.
    pub fn has_context(&self, key: &str) -> bool {
        self.vm
            .context_stack
            .get(key)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Clears all context entries for the given key.
    pub fn clear_context(&mut self, key: &str) {
        self.vm.context_stack.remove(key);
    }

    /// Returns a list of all currently active context keys.
    pub fn context_keys(&self) -> Vec<String> {
        self.vm
            .context_stack
            .iter()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Creates a snapshot map of the top active value for each context key.
    pub fn context_snapshot(&self) -> std::collections::HashMap<String, Value> {
        let mut map = std::collections::HashMap::new();
        for (key, stack) in &self.vm.context_stack {
            if let Some(top) = stack.last() {
                map.insert(key.clone(), *top);
            }
        }
        map
    }

    /// Temporarily override global function names with new implementations.
    /// Returns a guard that restores the originals when dropped.
    /// Used by cloud provider wrappers to swap in provider-specific functions.
    pub fn override_globals(
        &mut self,
        overrides: &[(&'static str, BuiltinImpl)],
    ) -> GlobalOverrideGuard<'_> {
        let saved = self.vm.save_and_override_globals(overrides);
        GlobalOverrideGuard { vm: self.vm, saved }
    }

    /// Override global function names manually (without a guard).
    /// Returns the saved state for later restoration via `restore_globals_manual`.
    /// Use this when you need to call other `ctx` methods between override and restore.
    pub fn override_globals_manual(
        &mut self,
        overrides: &[(&'static str, BuiltinImpl)],
    ) -> Vec<(Box<str>, Option<u32>)> {
        self.vm.save_and_override_globals(overrides)
    }

    /// Restore global function mappings from a saved state returned by `override_globals_manual`.
    pub fn restore_globals_manual(&mut self, saved: &[(Box<str>, Option<u32>)]) {
        self.vm.restore_globals(saved);
    }

    /// Scan NATIVE_REGISTRY for entries matching the given impl type name,
    /// and override globals with their implementations. Returns the saved state for later restoration.
    ///
    /// Each `#[xcl_fn]` declares `impl = "CloudflareProvider"` to link to its provider type.
    /// The method looks up the impl descriptor in IMPL_REGISTRY, gets its method names,
    /// and matches them against native entries with the same `impl_type`.
    pub fn override_by_trait(
        &mut self,
        impl_type_name: &str,
    ) -> Vec<(Box<str>, Option<u32>)> {
        use crate::native::{NATIVE_REGISTRY, IMPL_REGISTRY};

        // Find all impl descriptors for this type — collect trait names
        let impl_descs = IMPL_REGISTRY.iter().filter(|desc| {
            desc.type_name == impl_type_name
        }).collect::<Vec<_>>();

        if impl_descs.is_empty() {
            return Vec::new();
        }

        // Collect method names from all impl descriptors for this type
        let method_names: Vec<&str> = impl_descs.iter().flat_map(|desc| {
            desc.methods.iter().map(|m| m.name)
        }).collect();

        // Collect (owned_name, BuiltinImpl) pairs for matching entries
        let mut pairs: Vec<(String, BuiltinImpl)> = Vec::new();
        for entry in NATIVE_REGISTRY.iter() {
            if entry.impl_type == Some(impl_type_name) {
                // Check if this entry's method name matches any method in the impl
                if method_names.contains(&entry.method) {
                    let impl_ = match entry.kind {
                        NativeKind::Pure => {
                            let f: BuiltinFn = unsafe { std::mem::transmute(entry.ptr.0) };
                            BuiltinImpl::Pure(f)
                        }
                        NativeKind::Ctx => {
                            let f: BuiltinCtxFn = unsafe { std::mem::transmute(entry.ptr.0) };
                            BuiltinImpl::Ctx(f)
                        }
                    };
                    // Use the full dotted name (domain.method) to match how register_all_natives
                    // registers functions in global_index, e.g., "bucket.list" not just "list"
                    let full_name = if let Some(domain) = entry.domain {
                        format!("{}.{}", domain, entry.method)
                    } else {
                        entry.method.to_string()
                    };
                    pairs.push((full_name, impl_));
                }
            }
        }
        // Convert to static refs for save_and_override_globals
        let mut overrides: Vec<(&'static str, BuiltinImpl)> = Vec::with_capacity(pairs.len());
        for (name, impl_) in pairs {
            overrides.push((name.leak(), impl_));
        }
        self.vm.save_and_override_globals(&overrides)
    }

    /// Restore global function mappings from a saved state returned by `override_by_trait`.
    pub fn restore_by_trait(&mut self, saved: &[(Box<str>, Option<u32>)]) {
        self.vm.restore_globals(saved);
    }
}

/// Guard that restores global function overrides when dropped.
pub struct GlobalOverrideGuard<'a> {
    vm: &'a mut crate::vm::VM,
    saved: Vec<(Box<str>, Option<u32>)>,
}

impl Drop for GlobalOverrideGuard<'_> {
    fn drop(&mut self) {
        self.vm.restore_globals(&self.saved);
    }
}
