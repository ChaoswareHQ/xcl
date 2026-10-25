//! Arena-backed constructors.
//!
//! Each allocates its heap object inside the VM arena, so the object dies with
//! the next `run`. Values that must outlive the arena use `boxed.rs`.

use super::heap::new_heap_str;
use super::*;

impl Value {
    /// Constructs a `nil` (`none` / unit) value.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_rt::Value;
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
    /// use xcl_rt::Value;
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
    /// use xcl_rt::Value;
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
    /// use xcl_rt::Value;
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
        let header = HeapHeader {
            kind: KIND_STR,
            flags: 0,
            subtype: SUBTYPE_NONE,
            variant: VARIANT_DEFAULT,
        };
        // The bytes live in the arena, so they are reclaimed with the VM.
        // `from_str_boxed` is the escape hatch for constants that must
        // outlive every VM.
        let data: &'static str = unsafe { &*(arena.alloc_str(&s) as *const str) };
        let obj = arena.alloc(new_heap_str(header, data));
        Value(TAG_PTR | ((obj as *const HeapStr as u64) & PAYLOAD))
    }

    /// Allocates a string in the arena from a borrowed `&str`.
    ///
    /// Same as [`Value::from_str`] without taking ownership of a `String`, for
    /// callers that already have the bytes (the VM's `$0` cache, for example).
    #[inline(always)]
    pub fn from_str_ref(s: &str, arena: &bumpalo::Bump) -> Self {
        let header = HeapHeader {
            kind: KIND_STR,
            flags: 0,
            subtype: SUBTYPE_NONE,
            variant: VARIANT_DEFAULT,
        };
        let data: &'static str = unsafe { &*(arena.alloc_str(s) as *const str) };
        let obj = arena.alloc(new_heap_str(header, data));
        Value(TAG_PTR | ((obj as *const HeapStr as u64) & PAYLOAD))
    }

    /// Allocates a `HeapStr` from two parts, written once into the arena.
    ///
    /// Concatenation used to build a `String` (one malloc, one copy of each
    /// part) that was then moved into the arena and never freed.
    #[inline]
    pub(super) fn from_str_parts(a: &str, b: &str, hash: u64, arena: &bumpalo::Bump) -> Self {
        let mut s = bumpalo::collections::String::with_capacity_in(a.len() + b.len(), arena);
        s.push_str(a);
        s.push_str(b);
        let data: &'static str = unsafe { &*(s.into_bump_str() as *const str) };
        let obj = arena.alloc(HeapStr {
            header: HeapHeader {
                kind: KIND_STR,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            hash,
            data,
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
            version: 0,
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
    pub fn from_map(m: crate::HashMap<String, Value>, arena: &bumpalo::Bump) -> Self {
        // Build indexed arrays for fast field access
        let len = m.len();
        let mut key_hashes = Vec::with_capacity(len);
        let mut key_values = Vec::with_capacity(len);
        let mut data = XclMap::with_capacity(len);
        for (k, v) in &m {
            let hash = crate::runtime::vm::fast_hash(k.as_bytes());
            key_hashes.push((hash, k.clone()));
            key_values.push(*v);
            data.insert(k.clone(), *v);
        }
        let obj = arena.alloc(HeapMap {
            header: HeapHeader {
                kind: KIND_MAP,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            repr: MapRepr::Large {
                data,
                key_hashes,
                key_values,
                index_dirty: false,
            },
        });
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }

    /// Internal fast constructor for maps already built with XclMap and pre-computed key hashes.
    /// Used by op_make_dict to avoid double hashing and allocation.
    #[inline(always)]
    pub(crate) fn from_map_xcl(
        data: XclMap,
        key_hashes: Vec<(u64, String)>,
        key_values: Vec<Value>,
        arena: &bumpalo::Bump,
    ) -> Self {
        let obj = arena.alloc(HeapMap {
            header: HeapHeader {
                kind: KIND_MAP,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            repr: MapRepr::Large {
                data,
                key_hashes,
                key_values,
                index_dirty: false,
            },
        });
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }

    /// Constructs a small map (≤8 entries) directly from key-value pairs with precomputed hashes.
    /// Each entry is (key, value, fast_hash(key)). Keys must be string `Value`s. No extra heap allocation.
    #[inline(always)]
    pub fn from_small_map(
        entries: smallvec::SmallVec<[(Value, Value, u64); SMALL_MAP_MAX]>,
        arena: &bumpalo::Bump,
    ) -> Self {
        Self::from_small_map_typed(entries, SUBTYPE_NONE, VARIANT_DEFAULT, arena)
    }

    /// Like [`Value::from_small_map`], but stores a subtype/variant tag in the
    /// header so the value can be recognised without a field lookup (for
    /// example [`SUBTYPE_RANGE`]).
    #[inline(always)]
    pub fn from_small_map_typed(
        entries: smallvec::SmallVec<[(Value, Value, u64); SMALL_MAP_MAX]>,
        subtype: u16,
        variant: u8,
        arena: &bumpalo::Bump,
    ) -> Self {
        let obj = arena.alloc(HeapMap {
            header: HeapHeader {
                kind: KIND_MAP,
                flags: 0,
                subtype,
                variant,
            },
            repr: MapRepr::Small(entries),
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
            data: FuncData::Owned(f),
        });
        Value(TAG_PTR | ((obj as *const HeapFunc as u64) & PAYLOAD))
    }

    /// Allocates a function value without cloning the payload.
    ///
    /// Used for a bytecode's declared functions, whose `FuncValue` is shared
    /// through the [`Bytecode`](crate::bytecode::Bytecode).
    #[inline(always)]
    pub fn from_func_shared(f: Arc<FuncValue>, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapFunc {
            header: HeapHeader {
                kind: KIND_FUNC,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: FuncData::Shared(f),
        });
        Value(TAG_PTR | ((obj as *const HeapFunc as u64) & PAYLOAD))
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

    /// Returns a unit / nil value.
    pub fn unit() -> Self {
        Value(TAG_NIL)
    }

    /// Alias for [`Value::bool`].
    pub fn bool_(b: bool) -> Self {
        Value::bool(b)
    }
}
