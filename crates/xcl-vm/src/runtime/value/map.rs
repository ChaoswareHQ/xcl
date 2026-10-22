//! Map representation.
//!
//! Up to [`SMALL_MAP_MAX`] entries are an inline `SmallVec` scanned linearly;
//! larger maps use a hash map with pre-hashed key arrays. Both are exposed
//! through [`MapIter`].

use super::*;

/// Maximum inline entries for small-map representation.
pub const SMALL_MAP_MAX: usize = 8;

/// Internal representation of a heap map.
pub enum MapRepr {
    /// ≤ SMALL_MAP_MAX entries. Each entry is `(key, value, fast_hash(key))`.
    ///
    /// Linear scan wins here: entries are contiguous, the identity compare hits
    /// early for interned keys, and an index array costs an extra dependent load
    /// (measured: open addressing was 15-40% slower).
    Small(smallvec::SmallVec<[(Value, Value, u64); SMALL_MAP_MAX]>),
    /// Larger maps: hash storage plus the pre-hashed index arrays.
    Large {
        data: XclMap,
        key_hashes: Vec<(u64, String)>,
        key_values: Vec<Value>,
        index_dirty: bool,
    },
}

impl MapRepr {
    /// Returns an iterator over all `(key, value)` pairs.
    #[inline(always)]
    pub fn iter(&self) -> MapIter<'_> {
        match self {
            MapRepr::Small(entries) => MapIter::Small(entries.iter()),
            MapRepr::Large { data, .. } => MapIter::Large(data.iter()),
        }
    }
}

impl<'a> IntoIterator for &'a MapRepr {
    type Item = (&'a str, &'a Value);
    type IntoIter = MapIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Heap-allocated key-value dictionary map.
#[repr(C)]
pub struct HeapMap {
    /// Common object header.
    pub header: HeapHeader,
    /// Internal representation (small inline or large hashed).
    pub repr: MapRepr,
}

/// Iterator over map entries, yielding `(&str, &Value)`.
pub enum MapIter<'a> {
    Small(core::slice::Iter<'a, (Value, Value, u64)>),
    Large(hashbrown::hash_map::Iter<'a, String, Value>),
}

impl<'a> Iterator for MapIter<'a> {
    type Item = (&'a str, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MapIter::Small(iter) => {
                let (k, v, _) = iter.next()?;
                // Safety: k is a string Value inside the same HeapMap; the &str
                // points into the HeapStr data which lives in the same arena.
                let key_str: &'a str = k.as_str().unwrap_or("");
                Some((key_str, v))
            }
            MapIter::Large(iter) => {
                let (k, v) = iter.next()?;
                Some((k.as_str(), v))
            }
        }
    }
}

impl HeapMap {
    /// Returns the number of entries in the map.
    #[inline(always)]
    pub fn len(&self) -> usize {
        match &self.repr {
            MapRepr::Small(entries) => entries.len(),
            MapRepr::Large { data, .. } => data.len(),
        }
    }

    /// Returns `true` if the map has no entries.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lookup by key using hashing (for Large) or linear scan (for Small).
    /// Returns `Option<&Value>`.
    #[inline(always)]
    pub fn get(&self, key: &str) -> Option<&Value> {
        match &self.repr {
            MapRepr::Small(entries) => {
                let h = crate::runtime::vm::fast_hash(key.as_bytes());
                for (k, v, kh) in entries.iter() {
                    if *kh == h && small_key_matches(*k, key) {
                        return Some(v);
                    }
                }
                None
            }
            MapRepr::Large { data, .. } => data.get(key),
        }
    }

    /// Fast lookup using pre-computed key hashes (for Large) or linear scan (for Small).
    /// Returns `Option<Value>` (copied).
    #[inline(always)]
    pub fn get_fast(&self, key: &str) -> Option<Value> {
        match &self.repr {
            MapRepr::Small(entries) => {
                let h = crate::runtime::vm::fast_hash(key.as_bytes());
                for (k, v, kh) in entries.iter() {
                    if *kh == h && small_key_matches(*k, key) {
                        return Some(*v);
                    }
                }
                None
            }
            MapRepr::Large {
                data,
                key_hashes,
                key_values,
                index_dirty,
            } => {
                if !index_dirty {
                    let h = crate::runtime::vm::fast_hash(key.as_bytes());
                    for (i, (kh, k)) in key_hashes.iter().enumerate() {
                        if *kh == h && k == key {
                            return Some(key_values[i]);
                        }
                    }
                    None
                } else {
                    data.get(key).copied()
                }
            }
        }
    }

    /// Lookup by interned string `Value` key. Falls back to a string compare when the
    /// entry key is not the same interned `Value` (e.g. dynamically built keys).
    #[inline(always)]
    pub fn get_fast_value(&self, key: Value) -> Option<Value> {
        match &self.repr {
            MapRepr::Small(entries) => {
                // Interned keys (map literals, field reads) match on the raw
                // `Value` first. Hashing the query would dereference the string
                // object twice before the loop even starts, which costs more
                // than the scan it precedes. Only pay for it when identity
                // fails, i.e. when a dynamically built key is used.
                for (k, v, _) in entries.iter() {
                    if k.0 == key.0 {
                        return Some(*v);
                    }
                }
                let kh = key.str_hash().unwrap_or(0);
                if let Some(ks) = key.as_str() {
                    for (k, v, entry_h) in entries.iter() {
                        if *entry_h == kh && k.as_str() == Some(ks) {
                            return Some(*v);
                        }
                    }
                }
                None
            }
            MapRepr::Large {
                data,
                key_hashes,
                key_values,
                index_dirty,
            } => {
                if !index_dirty {
                    if let Some(key_str) = key.as_str() {
                        // Use cached hash from the key Value
                        let h = key
                            .str_hash()
                            .unwrap_or_else(|| crate::runtime::vm::fast_hash(key_str.as_bytes()));
                        for (i, (kh, k)) in key_hashes.iter().enumerate() {
                            if *kh == h && k == key_str {
                                return Some(key_values[i]);
                            }
                        }
                    }
                    None
                } else {
                    key.as_str().and_then(|k| data.get(k)).copied()
                }
            }
        }
    }

    /// Inserts or overwrites `key` with `value`.
    ///
    /// `key` must be a string value; returns `false` when it is not, so the
    /// caller can report the failed write instead of silently dropping it.
    ///
    /// A small map that is already full is promoted to the hashed
    /// representation. The hash caches stay in sync on every write, which keeps
    /// `get_fast_value` on its index path instead of forcing it to fall back to
    /// a string hash after the first assignment.
    pub fn set_fast_value(&mut self, key: Value, value: Value) -> bool {
        let Some(key_str) = key.as_str() else {
            return false;
        };
        let hash = key
            .str_hash()
            .unwrap_or_else(|| crate::runtime::vm::fast_hash(key_str.as_bytes()));

        match &mut self.repr {
            MapRepr::Small(entries) => {
                // Interned keys (field writes) match on the raw `Value` first,
                // the same order `get_fast_value` uses: no hashing at all for
                // the common case.
                for (k, v, _) in entries.iter_mut() {
                    if k.0 == key.0 {
                        *v = value;
                        return true;
                    }
                }
                for (k, v, entry_hash) in entries.iter_mut() {
                    if *entry_hash == hash && k.as_str() == Some(key_str) {
                        *v = value;
                        return true;
                    }
                }
                if entries.len() < SMALL_MAP_MAX {
                    entries.push((key, value, hash));
                    return true;
                }

                // Full: promote to the hashed representation. The old entries
                // move into the new map, so no field is lost.
                let len = entries.len();
                let mut data = XclMap::with_capacity(len + 1);
                let mut key_hashes = Vec::with_capacity(len + 1);
                let mut key_values = Vec::with_capacity(len + 1);
                for (k, v, h) in entries.iter() {
                    let s = k.as_str().unwrap_or("").to_string();
                    data.insert(s.clone(), *v);
                    key_hashes.push((*h, s));
                    key_values.push(*v);
                }
                data.insert(key_str.to_string(), value);
                key_hashes.push((hash, key_str.to_string()));
                key_values.push(value);
                self.repr = MapRepr::Large {
                    data,
                    key_hashes,
                    key_values,
                    index_dirty: false,
                };
                true
            }
            MapRepr::Large {
                data,
                key_hashes,
                key_values,
                index_dirty,
            } => {
                if *index_dirty {
                    key_hashes.clear();
                    key_values.clear();
                    for (k, v) in data.iter() {
                        key_hashes.push((crate::runtime::vm::fast_hash(k.as_bytes()), k.clone()));
                        key_values.push(*v);
                    }
                    *index_dirty = false;
                }
                match data.insert(key_str.to_string(), value) {
                    Some(_) => {
                        match key_hashes
                            .iter()
                            .position(|(kh, k)| *kh == hash && k == key_str)
                        {
                            Some(i) => key_values[i] = value,
                            // The index was incomplete (a map built outside this
                            // path); adding the entry restores the invariant
                            // instead of leaving a stale fast-path value.
                            None => {
                                key_hashes.push((hash, key_str.to_string()));
                                key_values.push(value);
                            }
                        }
                    }
                    None => {
                        key_hashes.push((hash, key_str.to_string()));
                        key_values.push(value);
                    }
                }
                true
            }
        }
    }

    /// Applies `f` to the value stored at `key`, inserting `f(nil)` when the key
    /// is absent.
    ///
    /// The fused field updates read and write the same entry, so doing it in one
    /// scan halves the work of a read-modify-write over `get_fast_value` plus
    /// `set_fast_value`.
    #[inline(always)]
    pub fn update_fast_value<F>(&mut self, key: Value, f: F) -> bool
    where
        F: FnOnce(Value) -> Value,
    {
        match &mut self.repr {
            MapRepr::Small(entries) => {
                // Interned key identity first, exactly like `get_fast_value`.
                for (k, v, _) in entries.iter_mut() {
                    if k.0 == key.0 {
                        *v = f(*v);
                        return true;
                    }
                }
                let Some(key_str) = key.as_str() else {
                    return false;
                };
                let hash = key
                    .str_hash()
                    .unwrap_or_else(|| crate::runtime::vm::fast_hash(key_str.as_bytes()));
                for (k, v, entry_hash) in entries.iter_mut() {
                    if *entry_hash == hash && k.as_str() == Some(key_str) {
                        *v = f(*v);
                        return true;
                    }
                }
            }
            MapRepr::Large {
                data,
                key_hashes,
                key_values,
                ..
            } => {
                let Some(key_str) = key.as_str() else {
                    return false;
                };
                let hash = key
                    .str_hash()
                    .unwrap_or_else(|| crate::runtime::vm::fast_hash(key_str.as_bytes()));
                for (i, (kh, k)) in key_hashes.iter().enumerate() {
                    if *kh == hash && k == key_str {
                        if let Some(slot) = data.get_mut(key_str) {
                            let new = f(*slot);
                            *slot = new;
                            key_values[i] = new;
                            return true;
                        }
                    }
                }
            }
        }
        // Absent: `set_fast_value` inserts or promotes, keeping the caches in
        // sync with the new entry.
        self.set_fast_value(key, f(Value::nil()))
    }

    /// Returns `true` if the map contains the given key.
    #[inline(always)]
    pub fn contains_key(&self, key: &str) -> bool {
        match &self.repr {
            MapRepr::Small(entries) => {
                let h = crate::runtime::vm::fast_hash(key.as_bytes());
                entries
                    .iter()
                    .any(|(k, _, kh)| *kh == h && small_key_matches(*k, key))
            }
            MapRepr::Large { data, .. } => data.contains_key(key),
        }
    }

    /// Returns an iterator over all `(key, value)` pairs.
    #[inline(always)]
    pub fn iter(&self) -> MapIter<'_> {
        match &self.repr {
            MapRepr::Small(entries) => MapIter::Small(entries.iter()),
            MapRepr::Large { data, .. } => MapIter::Large(data.iter()),
        }
    }

    /// Returns an iterator over all keys as `&str`.
    #[inline(always)]
    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.iter().map(|(k, _)| k)
    }
}

impl<'a> IntoIterator for &'a HeapMap {
    type Item = (&'a str, &'a Value);
    type IntoIter = MapIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[inline(always)]
fn small_key_matches(k: Value, key: &str) -> bool {
    k.as_str().is_some_and(|s| s == key)
}
