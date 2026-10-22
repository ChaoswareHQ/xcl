//! Operators.
//!
//! Integer fast paths take the tagged-integer branch; everything else falls
//! back to the generic (`*_slow`) path for floats, strings and mixed operands.

use super::*;

impl Value {
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
            // Fast path: both are strings — concatenate, leaking bytes so they
            // survive arena reset (same guarantee as from_str / from_str_boxed).
            if let (Some(a), Some(b)) = (self.as_str(), other.as_str()) {
                // Continue the left operand's cached hash instead of re-hashing the
                // whole result: FNV-1a is resumable, so this is O(len(b)).
                let left_hash = self
                    .str_hash()
                    .unwrap_or_else(|| crate::runtime::vm::fast_hash(a.as_bytes()));
                let hash = crate::runtime::vm::fast_hash_step(left_hash, b.as_bytes());
                return Self::from_str_parts(a, b, hash, arena);
            }

            // Mixed types (e.g. int + string): avoid format! macro
            let a = self.as_string();
            let b = other.as_string();
            let hash = crate::runtime::vm::fast_hash_step(
                crate::runtime::vm::fast_hash(a.as_bytes()),
                b.as_bytes(),
            );
            return Self::from_str_parts(&a, &b, hash, arena);
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
            // Strings compare by content. Functions (and other identity-only heap
            // values) are never equal unless the payloads match: formatting them
            // into a string here allocated on every callee-cache miss.
            if self.heap_kind() == Some(KIND_STR) && other.heap_kind() == Some(KIND_STR) {
                return self.as_str() == other.as_str();
            }
            if self.heap_kind() == Some(KIND_FUNC) || other.heap_kind() == Some(KIND_FUNC) {
                return false;
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
}
