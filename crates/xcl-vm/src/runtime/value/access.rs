//! Predicates and accessors.
//!
//! Every one is a tag check followed by at most one pointer load: no
//! allocation and no container scan.

use super::*;

impl Value {
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

    /// Returns the array's mutation version, if this value is an array.
    #[inline(always)]
    pub fn array_version(self) -> Option<u64> {
        if self.heap_kind() == Some(KIND_ARRAY) {
            Some(unsafe { (*self.as_ptr::<HeapArray>()).version })
        } else {
            None
        }
    }

    /// Marks an array as mutated so versioned caches re-validate it.
    #[inline(always)]
    pub fn bump_array_version(self) {
        if self.heap_kind() == Some(KIND_ARRAY) {
            // SAFETY: the value is a live heap array; only the version field is
            // written, and callers bump after dropping any `data` borrow.
            unsafe {
                let obj = self.as_ptr::<HeapArray>() as *mut HeapArray;
                (*obj).version = (*obj).version.wrapping_add(1);
            }
        }
    }

    /// Returns a static reference to the underlying HeapMap if this value is a map.
    #[inline(always)]
    pub fn as_map(self) -> Option<&'static HeapMap> {
        if self.heap_kind() == Some(KIND_MAP) {
            Some(unsafe { &*(self.as_ptr::<HeapMap>()) })
        } else {
            None
        }
    }

    /// Returns a reference to the underlying HeapMap if this value is a map.
    #[inline(always)]
    pub fn as_map_mut_ref(self) -> Option<&'static HeapMap> {
        if self.heap_kind() == Some(KIND_MAP) {
            Some(unsafe { &*(self.as_ptr::<HeapMap>()) })
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

    /// Fast map lookup — thin wrapper delegating to `HeapMap::get_fast`.
    #[inline(always)]
    pub fn map_get_fast(map: &HeapMap, key: &str) -> Option<Value> {
        map.get_fast(key)
    }

    /// Returns a string slice if this value is a heap-allocated string.
    #[inline(always)]
    pub fn as_str(self) -> Option<&'static str> {
        if self.heap_kind() == Some(KIND_STR) {
            Some(unsafe { (*(self.as_ptr::<HeapStr>())).data })
        } else {
            None
        }
    }

    /// Returns the cached FNV-1a hash of the string if this value is a heap-allocated string.
    #[inline(always)]
    pub fn str_hash(self) -> Option<u64> {
        if self.heap_kind() == Some(KIND_STR) {
            Some(unsafe { (*(self.as_ptr::<HeapStr>())).hash })
        } else {
            None
        }
    }

    /// Returns a reference to the function value if this value is a heap-allocated function.
    #[inline(always)]
    pub fn as_func(self) -> Option<&'static FuncValue> {
        if self.heap_kind() == Some(KIND_FUNC) {
            Some(unsafe { (*(self.as_ptr::<HeapFunc>())).data.get() })
        } else {
            None
        }
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
}
