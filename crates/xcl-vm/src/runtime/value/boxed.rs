//! Values that outlive the arena.
//!
//! `*_boxed` constructors leak their allocation so the value survives an arena
//! reset; `escape` rebuilds an arena value the same way before `run` returns it
//! to the host.

use super::*;
use super::heap::new_heap_str;

impl Value {
    /// Allocates a string with static lifetime (leaked Box) suitable for constants.
    pub fn from_str_boxed(s: String) -> Self {
        let obj = Box::leak(Box::new(new_heap_str(
            HeapHeader {
                kind: KIND_STR,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            Box::leak(s.into_boxed_str()),
        )));
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
            version: 0,
        }));
        Value(TAG_PTR | ((obj as *const HeapArray as u64) & PAYLOAD))
    }

    /// Allocates a map with static lifetime (leaked Box).
    pub fn from_map_boxed(m: crate::HashMap<String, Value>) -> Self {
        Self::from_map_boxed_typed_v(m, SUBTYPE_NONE, VARIANT_DEFAULT)
    }

    /// Allocates a function value with static lifetime (leaked Box).
    ///
    /// Used by [`Value::escape`] to move a returned function out of the VM
    /// arena.
    pub fn from_func_boxed(f: FuncValue) -> Self {
        let obj = Box::leak(Box::new(HeapFunc {
            header: HeapHeader {
                kind: KIND_FUNC,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: FuncData::Owned(f),
        }));
        Value(TAG_PTR | ((obj as *const HeapFunc as u64) & PAYLOAD))
    }

    /// Like [`Value::from_func_boxed`], but keeps the descriptor shared.
    ///
    /// Used by [`Value::escape`] for a declared function with no captured
    /// environment: there is nothing to rewrite, so the `Arc` is bumped instead
    /// of copying the bytecode and parameter lists.
    pub fn from_func_shared_boxed(f: Arc<FuncValue>) -> Self {
        let obj = Box::leak(Box::new(HeapFunc {
            header: HeapHeader {
                kind: KIND_FUNC,
                flags: 0,
                subtype: SUBTYPE_NONE,
                variant: VARIANT_DEFAULT,
            },
            data: FuncData::Shared(f),
        }));
        Value(TAG_PTR | ((obj as *const HeapFunc as u64) & PAYLOAD))
    }

    /// Deep-copies a value out of the VM arena.
    ///
    /// Values returned by [`VM::run`](crate::runtime::VM::run) outlive the VM
    /// (the benchmark harness reads a result after dropping its VM), so string
    /// bytes, arrays, maps and functions the arena owns must be rebuilt in
    /// leaked heap memory before the result leaves the VM. This is the escape
    /// barrier that lets everything else stay arena-allocated and be reclaimed
    /// with the VM.
    ///
    /// Arrays can contain themselves (in-place `push`), so they are rebuilt
    /// through a memo to preserve shared/cyclic structure. Maps are immutable
    /// after construction, so they cannot form cycles.
    pub fn escape(v: Value) -> Value {
        let mut memo: crate::HashMap<u64, Value> = Default::default();
        Self::escape_inner(v, &mut memo)
    }

    fn escape_inner(v: Value, memo: &mut crate::HashMap<u64, Value>) -> Value {
        match v.heap_kind() {
            Some(KIND_STR) => match v.as_str() {
                Some(s) => Self::from_str_boxed(s.to_string()),
                None => v,
            },
            Some(KIND_ARRAY) => {
                if let Some(&copy) = memo.get(&v.0) {
                    return copy;
                }
                let out = Self::from_array_boxed(Vec::new());
                memo.insert(v.0, out);
                let mut elements = Vec::new();
                if let Some(src) = v.as_array() {
                    elements.reserve(src.len());
                    for &e in src {
                        elements.push(Self::escape_inner(e, memo));
                    }
                }
                if let Some(dst) = out.as_array_mut() {
                    dst.extend_from_slice(&elements);
                }
                out.bump_array_version();
                out
            }
            Some(KIND_MAP) => {
                if let Some(&copy) = memo.get(&v.0) {
                    return copy;
                }
                let mut entries = crate::HashMap::new();
                if let Some(src) = v.as_map() {
                    entries.reserve(src.len());
                    for (k, val) in src.iter() {
                        entries.insert(k.to_string(), Self::escape_inner(*val, memo));
                    }
                }
                let out = Self::from_map_boxed_typed_v(
                    entries,
                    v.map_subtype().unwrap_or(SUBTYPE_NONE),
                    {
                        // Preserve the variant tag (result/error/handle).
                        unsafe { (*v.as_ptr::<HeapMap>()).header.variant }
                    },
                );
                memo.insert(v.0, out);
                out
            }
            Some(KIND_FUNC) => {
                // A declared function is immutable and shared: only its captured
                // environment can point into the arena, so share it when there is
                // nothing to rewrite. A runtime closure is copied either way.
                let data = unsafe { &(*v.as_ptr::<HeapFunc>()).data };
                match data {
                    FuncData::Shared(f) if f.closure_env.is_empty() => {
                        Self::from_func_shared_boxed(Arc::clone(f))
                    }
                    _ => {
                        let mut owned = data.get().clone();
                        for value in owned.closure_env.iter_mut() {
                            *value = Self::escape_inner(*value, memo);
                        }
                        Self::from_func_boxed(owned)
                    }
                }
            }
            _ => v,
        }
    }

    /// Creates a typed heap map with specified subtype and variant tags, leaked to static lifetime.
    pub fn from_map_boxed_typed_v(
        m: crate::HashMap<String, Value>,
        subtype: u16,
        variant: u8,
    ) -> Self {
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
        let obj = Box::leak(Box::new(HeapMap {
            header: HeapHeader {
                kind: KIND_MAP,
                flags: 0,
                subtype,
                variant,
            },
            repr: MapRepr::Large {
                data,
                key_hashes,
                key_values,
                index_dirty: false,
            },
        }));
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }
}
