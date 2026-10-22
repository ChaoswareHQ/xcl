//! Length and string form.

use super::*;

impl Value {
    /// Returns the length of the string, array, or map; or 0 for primitive values.
    pub fn len(self) -> usize {
        match self.heap_kind() {
            Some(KIND_STR) => unsafe { (*(self.as_ptr::<HeapStr>())).data.chars().count() },
            Some(KIND_ARRAY) => unsafe { (*(self.as_ptr::<HeapArray>())).data.len() },
            Some(KIND_MAP) => unsafe { (*(self.as_ptr::<HeapMap>())).len() },
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
            Some(KIND_STR) => unsafe { (*(self.as_ptr::<HeapStr>())).data.to_string() },
            Some(KIND_FUNC) => {
                let f = unsafe { &(*(self.as_ptr::<HeapFunc>())).data };
                format!("<fn {}>", f.get().name)
            }
            Some(KIND_ARRAY) => {
                let v = unsafe { &(*(self.as_ptr::<HeapArray>())).data };
                let items: Vec<String> = v.iter().map(|x| x.as_string()).collect();
                format!("[{}]", items.join(", "))
            }
            Some(KIND_MAP) => {
                let m = unsafe { &(*(self.as_ptr::<HeapMap>())).repr };
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
}

impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

// — From implementations for common types (used by #[xcl_impl] generated wrappers) —
