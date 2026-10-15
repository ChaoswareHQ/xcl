//! XCL runtime values.
//!
//! XCL uses a NaN-boxed value representation: primitives (int, float, bool, nil)
//! are stored directly in the 64-bit word, while heap-allocated types (String, Array,
//! Map, Func, Handle) are stored as pointers in the lower 48 bits of a NaN payload.

use std::collections::HashMap;
use std::sync::Arc;

const QNAN: u64 = 0x7FF8_0000_0000_0000;
const TAG_MASK: u64 = 0x000F_0000_0000_0000;
const PAYLOAD: u64 = 0x0000_FFFF_FFFF_FFFF;

const TAG_NIL:   u64 = QNAN;
const TAG_FALSE: u64 = QNAN | (1u64 << 48);
const TAG_TRUE:  u64 = QNAN | (2u64 << 48);
const TAG_INT:   u64 = QNAN | (3u64 << 48);
const TAG_PTR:   u64 = QNAN | (4u64 << 48);

const KIND_STR:     u8 = 0;
const KIND_ARRAY:   u8 = 1;
const KIND_MAP:     u8 = 2;
const KIND_FUNC:    u8 = 3;
const KIND_HANDLE:  u8 = 6;

/// Subtype constants for typed heap maps (Result, Error, etc.)
pub const SUBTYPE_NONE:    u16 = 0;
pub const SUBTYPE_RESULT:  u16 = 1;
pub const SUBTYPE_ERROR:   u16 = 2;
pub const SUBTYPE_HANDLE:  u16 = 3;
pub const SUBTYPE_USER_BASE: u16 = 100;

/// Variant constants for sum types
pub const VARIANT_OK:      u8 = 0;
pub const VARIANT_ERR:     u8 = 1;
pub const VARIANT_DEFAULT: u8 = 0;

/// Header for all heap-allocated XCL values.
#[repr(C)]
pub struct HeapHeader { pub kind: u8, pub flags: u8, pub _pad: [u8; 6] }

#[repr(C)]
pub struct HeapStr   { pub header: HeapHeader, pub data: String }
#[repr(C)]
pub struct HeapArray { pub header: HeapHeader, pub data: Vec<Value> }
#[repr(C)]
pub struct HeapMap   {
    pub header:  HeapHeader,
    pub subtype: u16,
    pub variant: u8,
    pub _pad:    [u8; 5],
    pub data:    HashMap<String, Value>,
}
#[repr(C)]
pub struct HeapFunc  { pub header: HeapHeader, pub data: FuncValue }

/// Heap-allocated handle wrapping an OS resource.
#[repr(C)]
pub struct HeapHandle {
    pub header: HeapHeader,
    pub data:   Option<HandleKind>,
}

/// Kinds of OS handles that XCL can wrap.
#[derive(Debug)]
pub enum HandleKind {
    /// An open file with its path stored for re-opening.
    File { file: std::fs::File, path: String },
    /// Standard input stream.
    Stdin,
    /// Standard output stream.
    Stdout,
    /// Standard error stream.
    Stderr,
}

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Value(pub u64);

impl PartialEq for Value {
    #[inline] fn eq(&self, other: &Self) -> bool { Value::eq_(*self, *other) }
}
impl Eq for Value {}

impl Value {
    #[inline(always)] pub const fn nil() -> Self { Value(TAG_NIL) }
    #[inline(always)] pub fn bool(b: bool) -> Self { if b { Value(TAG_TRUE) } else { Value(TAG_FALSE) } }

    #[inline(always)]
    pub fn from_f64(f: f64) -> Self {
        let bits = if f.is_nan() { f64::NAN.to_bits() } else { f.to_bits() };
        Value(bits)
    }

    #[inline(always)]
    pub fn from_i48(i: i64) -> Self { Value(TAG_INT | ((i as u64) & PAYLOAD)) }

    #[inline(always)]
    pub fn from_str(s: String, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapStr {
            header: HeapHeader { kind: KIND_STR, flags: 0, _pad: [0; 6] },
            data: s,
        });
        Value(TAG_PTR | ((obj as *const HeapStr as u64) & PAYLOAD))
    }

    #[inline(always)]
    pub fn from_array(elements: Vec<Value>, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapArray {
            header: HeapHeader { kind: KIND_ARRAY, flags: 0, _pad: [0; 6] },
            data: elements,
        });
        Value(TAG_PTR | ((obj as *const HeapArray as u64) & PAYLOAD))
    }

    #[inline(always)]
    pub fn from_map(m: HashMap<String, Value>, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapMap {
            header:  HeapHeader { kind: KIND_MAP, flags: 0, _pad: [0; 6] },
            subtype: SUBTYPE_NONE,
            variant: VARIANT_DEFAULT,
            _pad:    [0; 5],
            data:    m,
        });
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }

    #[inline(always)]
    pub fn from_func(f: FuncValue, arena: &bumpalo::Bump) -> Self {
        let obj = arena.alloc(HeapFunc {
            header: HeapHeader { kind: KIND_FUNC, flags: 0, _pad: [0; 6] },
            data: f,
        });
        Value(TAG_PTR | ((obj as *const HeapFunc as u64) & PAYLOAD))
    }

    #[inline(always)] pub fn is_float(self) -> bool { self.0 & QNAN != QNAN }
    #[inline(always)] pub fn tag(self) -> u64 { self.0 & (QNAN | TAG_MASK) }
    #[inline(always)] pub fn is_nil(self) -> bool { self.tag() == TAG_NIL }
    #[inline(always)] pub fn is_bool(self) -> bool { self.0 == TAG_TRUE || self.0 == TAG_FALSE }
    #[inline(always)] pub fn is_int(self) -> bool { self.tag() == TAG_INT }
    #[inline(always)] pub fn is_ptr(self) -> bool { self.tag() == TAG_PTR }

    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        !matches!(self.tag(), TAG_NIL | TAG_FALSE)
    }

    #[inline(always)] pub fn as_f64(self) -> f64 { f64::from_bits(self.0) }
    #[inline(always)] pub fn as_i48(self) -> i64 { ((self.0 & PAYLOAD) as i64) << 16 >> 16 }
    #[inline(always)] pub fn as_bool(self) -> bool { self.0 == TAG_TRUE }
    #[inline(always)] pub fn as_ptr<T>(self) -> *const T { (self.0 & PAYLOAD) as *const T }

    #[inline(always)]
    pub fn heap_kind(self) -> Option<u8> {
        if self.is_ptr() {
            Some(unsafe { *((self.0 & PAYLOAD) as *const u8) })
        } else { None }
    }

    #[inline(always)]
    pub fn as_array(self) -> Option<&'static Vec<Value>> {
        if self.heap_kind() == Some(KIND_ARRAY) {
            Some(unsafe { &(*(self.as_ptr::<HeapArray>())).data })
        } else { None }
    }

    #[inline(always)]
    pub fn as_array_mut(self) -> Option<&'static mut Vec<Value>> {
        if self.heap_kind() == Some(KIND_ARRAY) {
            Some(unsafe { &mut (*(self.as_ptr::<HeapArray>() as *mut HeapArray)).data })
        } else { None }
    }

    #[inline(always)]
    pub fn as_map(self) -> Option<&'static HashMap<String, Value>> {
        if self.heap_kind() == Some(KIND_MAP) {
            Some(unsafe { &(*(self.as_ptr::<HeapMap>())).data })
        } else { None }
    }

    /// Get the subtype of a heap map (Result=1, Error=2, etc.)
    #[inline(always)]
    pub fn map_subtype(self) -> Option<u16> {
        if self.heap_kind() == Some(KIND_MAP) {
            let ptr = self.as_ptr::<HeapMap>();
            Some(unsafe { (*ptr).subtype })
        } else { None }
    }

    /// Get the variant of a heap map (Ok=0, Err=1, etc.)
    #[inline(always)]
    pub fn map_variant(self) -> Option<u8> {
        if self.heap_kind() == Some(KIND_MAP) {
            let ptr = self.as_ptr::<HeapMap>();
            Some(unsafe { (*ptr).variant })
        } else { None }
    }


    #[inline(always)]
    pub fn as_str(self) -> Option<&'static str> {
        if self.heap_kind() == Some(KIND_STR) {
            Some(unsafe { &(*(self.as_ptr::<HeapStr>())).data })
        } else { None }
    }

    #[inline(always)]
    pub fn as_func(self) -> Option<&'static FuncValue> {
        if self.heap_kind() == Some(KIND_FUNC) {
            Some(unsafe { &(*(self.as_ptr::<HeapFunc>())).data })
        } else { None }
    }

    pub fn add(self, other: Self, arena: &bumpalo::Bump) -> Self {
        if self.is_int() && other.is_int() {
            let a = self.as_i48();
            let b = other.as_i48();
            if let Some(r) = a.checked_add(b) { return Self::from_i48(r); }
            return Self::from_f64(a as f64 + b as f64);
        }
        if self.is_float() || other.is_float() {
            let a = if self.is_float() { self.as_f64() } else { self.as_i48() as f64 };
            let b = if other.is_float() { other.as_f64() } else { other.as_i48() as f64 };
            return Self::from_f64(a + b);
        }
        if self.tag() == TAG_PTR || other.tag() == TAG_PTR {
            let a = self.as_string();
            let b = other.as_string();
            return Self::from_str(format!("{a}{b}"), arena);
        }
        Self::nil()
    }

    #[doc(alias("sub"))]
    pub fn sub_(self, other: Self) -> Self {
        if self.is_int() && other.is_int() {
            let a = self.as_i48();
            let b = other.as_i48();
            if let Some(r) = a.checked_sub(b) { return Self::from_i48(r); }
            return Self::from_f64(a as f64 - b as f64);
        }
        let a = if self.is_float() { self.as_f64() } else { self.as_i48() as f64 };
        let b = if other.is_float() { other.as_f64() } else { other.as_i48() as f64 };
        Self::from_f64(a - b)
    }

    pub fn mul(self, other: Self, arena: &bumpalo::Bump) -> Self {
        if self.is_int() && other.is_int() {
            let a = self.as_i48();
            let b = other.as_i48();
            if let Some(r) = a.checked_mul(b) { return Self::from_i48(r); }
            return Self::from_f64(a as f64 * b as f64);
        }
        if self.is_float() || other.is_float() {
            let a = if self.is_float() { self.as_f64() } else { self.as_i48() as f64 };
            let b = if other.is_float() { other.as_f64() } else { other.as_i48() as f64 };
            return Self::from_f64(a * b);
        }
        if self.tag() == TAG_PTR && other.is_int() {
            return Self::from_str(self.as_string().repeat(other.as_i48() as usize), arena);
        }
        if other.tag() == TAG_PTR && self.is_int() {
            return Self::from_str(other.as_string().repeat(self.as_i48() as usize), arena);
        }
        Self::nil()
    }

    #[doc(alias("div"))]
    pub fn div_(self, other: Self) -> Self {
        if self.is_int() && other.is_int() {
            let b = other.as_i48();
            if b != 0 { return Self::from_i48(self.as_i48() / b); }
            return Self::from_f64(f64::INFINITY);
        }
        let a = if self.is_float() { self.as_f64() } else { self.as_i48() as f64 };
        let b = if other.is_float() { other.as_f64() } else { other.as_i48() as f64 };
        Self::from_f64(a / b)
    }

    pub fn mod_(self, other: Self) -> Self {
        if self.is_int() && other.is_int() {
            let b = other.as_i48();
            if b != 0 { return Self::from_i48(self.as_i48() % b); }
        }
        Self::nil()
    }

    #[doc(alias("eq"))]
    pub fn eq_(self, other: Self) -> bool {
        if self.is_int() && other.is_int() { return self.as_i48() == other.as_i48(); }
        if self.is_float() && other.is_float() { return self.as_f64() == other.as_f64(); }
        if self.is_int() && other.is_float() { return self.as_i48() as f64 == other.as_f64(); }
        if self.is_float() && other.is_int() { return self.as_f64() == other.as_i48() as f64; }
        if self.is_bool() && other.is_bool() { return self.as_bool() == other.as_bool(); }
        if self.is_nil() && other.is_nil() { return true; }
        if self.tag() == TAG_PTR || other.tag() == TAG_PTR { return self.as_string() == other.as_string(); }
        false
    }

    pub fn lt(self, other: Self) -> bool {
        if self.is_int() && other.is_int() { return self.as_i48() < other.as_i48(); }
        if self.is_float() && other.is_float() { return self.as_f64() < other.as_f64(); }
        if self.is_int() && other.is_float() { return (self.as_i48() as f64) < other.as_f64(); }
        if self.is_float() && other.is_int() { return self.as_f64() < (other.as_i48() as f64); }
        if self.tag() == TAG_PTR && other.tag() == TAG_PTR { return self.as_string() < other.as_string(); }
        false
    }

    pub fn gt(self, other: Self) -> bool {
        if self.is_int() && other.is_int() { return self.as_i48() > other.as_i48(); }
        if self.is_float() && other.is_float() { return self.as_f64() > other.as_f64(); }
        if self.is_int() && other.is_float() { return (self.as_i48() as f64) > other.as_f64(); }
        if self.is_float() && other.is_int() { return self.as_f64() > (other.as_i48() as f64); }
        if self.tag() == TAG_PTR && other.tag() == TAG_PTR { return self.as_string() > other.as_string(); }
        false
    }

    #[doc(alias("not"))]
    #[inline] pub fn not_(self) -> Self { Self::bool(!self.is_truthy()) }

    #[doc(alias("neg"))]
    #[inline]
    pub fn neg_(self) -> Self {
        if self.is_int() { Self::from_i48(-self.as_i48()) }
        else if self.is_float() { Self::from_f64(-self.as_f64()) }
        else { self }
    }

    pub fn len(self) -> usize {
        match self.heap_kind() {
            Some(KIND_STR)   => unsafe { (*(self.as_ptr::<HeapStr>())).data.chars().count() },
            Some(KIND_ARRAY) => unsafe { (*(self.as_ptr::<HeapArray>())).data.len() },
            Some(KIND_MAP)   => unsafe { (*(self.as_ptr::<HeapMap>())).data.len() },
            _ => 0,
        }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn as_string(self) -> String {
        if self.is_nil() { return "none".into(); }
        if self.is_bool() { return self.as_bool().to_string(); }
        if self.is_int() { return self.as_i48().to_string(); }
        if self.is_float() { return self.as_f64().to_string(); }
        match self.heap_kind() {
            Some(KIND_STR) => unsafe { (*(self.as_ptr::<HeapStr>())).data.clone() },
            Some(KIND_FUNC) => format!("<fn {}>", unsafe { (*(self.as_ptr::<HeapFunc>())).data.name.clone() }),
            Some(KIND_ARRAY) => {
                let v = unsafe { &(*(self.as_ptr::<HeapArray>())).data };
                let items: Vec<String> = v.iter().map(|x| x.as_string()).collect();
                format!("[{}]", items.join(", "))
            }
            Some(KIND_MAP) => {
                let m = unsafe { &(*(self.as_ptr::<HeapMap>())).data };
                let items: Vec<String> = m.iter().map(|(k, v)| format!("{k}: {}", v.as_string())).collect();
                format!("{{{}}}", items.join(", "))
            }
            _ => "unknown".into(),
        }
    }

    #[inline]
    pub fn as_int(self) -> i64 {
        if self.is_int() { self.as_i48() }
        else if self.is_float() { self.as_f64() as i64 }
        else { 0 }
    }

    #[inline]
    pub fn as_float(self) -> f64 {
        if self.is_float() { self.as_f64() }
        else if self.is_int() { self.as_i48() as f64 }
        else { 0.0 }
    }

    pub fn from_handle(h: HandleKind) -> Self {
        let obj = Box::leak(Box::new(HeapHandle {
            header: HeapHeader { kind: KIND_HANDLE, flags: 0, _pad: [0; 6] },
            data:   Some(h),
        }));
        Value(TAG_PTR | ((obj as *const HeapHandle as u64) & PAYLOAD))
    }

    pub fn as_handle(&self) -> Option<&HandleKind> {
        if self.heap_kind() == Some(KIND_HANDLE) {
            unsafe { (*(self.as_ptr::<HeapHandle>())).data.as_ref() }
        } else { None }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn as_handle_mut(&self) -> Option<&mut HandleKind> {
        if self.heap_kind() == Some(KIND_HANDLE) {
            unsafe { (*(self.as_ptr::<HeapHandle>() as *mut HeapHandle)).data.as_mut() }
        } else { None }
    }

    pub fn take_handle(self) -> Option<HandleKind> {
        if self.heap_kind() == Some(KIND_HANDLE) {
            let heap = unsafe { &mut *(self.as_ptr::<HeapHandle>() as *mut HeapHandle) };
            heap.data.take()
        } else { None }
    }

    pub fn from_str_boxed(s: String) -> Self {
        let obj = Box::leak(Box::new(HeapStr {
            header: HeapHeader { kind: KIND_STR, flags: 0, _pad: [0; 6] },
            data: s,
        }));
        Value(TAG_PTR | ((obj as *const HeapStr as u64) & PAYLOAD))
    }

    pub fn from_array_boxed(elements: Vec<Value>) -> Self {
        let obj = Box::leak(Box::new(HeapArray {
            header: HeapHeader { kind: KIND_ARRAY, flags: 0, _pad: [0; 6] },
            data: elements,
        }));
        Value(TAG_PTR | ((obj as *const HeapArray as u64) & PAYLOAD))
    }

    pub fn from_map_boxed(m: HashMap<String, Value>) -> Self {
        Self::from_map_boxed_typed_v(m, SUBTYPE_NONE, VARIANT_DEFAULT)
    }

    /// Create a heap map with subtype/variant, leaked to static lifetime.
    pub fn from_map_boxed_typed_v(m: HashMap<String, Value>, subtype: u16, variant: u8) -> Self {
        let obj = Box::leak(Box::new(HeapMap {
            header: HeapHeader { kind: KIND_MAP, flags: 0, _pad: [0; 6] },
            subtype,
            variant,
            _pad: [0; 5],
            data: m,
        }));
        Value(TAG_PTR | ((obj as *const HeapMap as u64) & PAYLOAD))
    }

    pub fn unit() -> Self { Value(TAG_NIL) }

    pub fn bool_(b: bool) -> Self { Value::bool(b) }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

// — Argument extraction for typed native functions —

/// Extract a typed Rust argument from a `&[Value]` slice at the given index.
/// This function is called by the `#[xcl_fn]` macro-generated wrappers.
pub fn extract_arg<T: NativeArg>(args: &[Value], index: usize) -> T {
    let v = args.get(index).copied().unwrap_or(Value::nil());
    T::from_value(v)
}

/// Trait for types that can be extracted from a `Value`.
pub trait NativeArg: Sized {
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

#[derive(Clone, Debug)]
pub struct FuncValue {
    pub name: String,
    pub bytecode: Option<Arc<crate::bytecode::Bytecode>>,
    pub params: Vec<String>,
    pub closure_captures: Vec<String>,
    pub closure_env: Vec<(String, Value)>,
}

impl PartialEq for FuncValue {
    fn eq(&self, other: &Self) -> bool { self.name == other.name }
}

// — Native function registry types —

pub struct NativeEntry {
    pub domain:   Option<&'static str>,
    pub method:   &'static str,
    pub kind:     NativeKind,
    pub ptr:      RawPtr,
    pub args:     Option<&'static str>,
    pub ret:      Option<&'static str>,
    pub named:    &'static [NamedParam],
    pub summary:  Option<&'static str>,
    pub arg_desc: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Copy)]
pub struct RawPtr(pub *const ());

unsafe impl Sync for RawPtr {}
unsafe impl Send for RawPtr {}

pub struct NamedParam {
    pub name:    &'static str,
    pub ty:      &'static str,
    pub default: &'static str,
}

#[derive(Clone, Copy)]
pub enum NativeKind { Pure, Ctx }

pub type BuiltinFn    = fn(&[Value]) -> Value;
pub type BuiltinCtxFn = fn(&mut DomainCtx) -> Value;

#[derive(Clone, Copy)]
pub enum BuiltinImpl {
    Pure(BuiltinFn),
    Ctx(BuiltinCtxFn),
}

impl BuiltinImpl {
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

pub struct TypeDescriptor {
    pub name:     &'static str,
    pub domain:   Option<&'static str>,
    pub kind:     TypeKind,
    pub fields:   &'static [TypeField],
    pub variants: &'static [TypeVariant],
    pub summary:  Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum TypeKind { Opaque, Record, Sum, Alias }

pub struct TypeField {
    pub name:     &'static str,
    pub ty:       &'static str,
    pub optional: bool,
}

pub struct TypeVariant {
    pub name:   &'static str,
    pub fields: &'static [TypeField],
}

// — Impl descriptor types —

pub struct ImplDescriptor {
    pub type_name:  &'static str,
    pub trait_name: &'static str,
    pub domain:     Option<&'static str>,
    pub methods:    &'static [ImplMethod],
}

pub struct ImplMethod {
    pub name:    &'static str,
    pub args:    Option<&'static str>,
    pub ret:     Option<&'static str>,
    pub summary: Option<&'static str>,
}

pub struct DomainCtx<'a> {
    pub(crate) vm:   &'a mut crate::vm::VM,
    pub(crate) args: &'a [Value],
}

impl<'a> DomainCtx<'a> {
    pub fn args(&self) -> &[Value] { self.args }
    pub fn argc(&self) -> usize { self.args.len() }
    pub fn arg(&self, i: usize) -> Value { self.args.get(i).copied().unwrap_or(Value::unit()) }
    pub fn arena(&self) -> &bumpalo::Bump { &self.vm.arena }
    pub fn unit(&self) -> Value { Value::unit() }
    pub fn boolean(&self, b: bool) -> Value { Value::bool_(b) }
    pub fn string(&self, s: String) -> Value { Value::from_str_boxed(s) }
    pub fn array(&self, v: Vec<Value>) -> Value { Value::from_array_boxed(v) }
    pub fn i64(&self, n: i64) -> Value { Value::from_i48(n) }
    pub fn u64(&self, n: u64) -> Value { Value::from_i48(n as i64) }

    /// Call an XCL closure value with the given args. Reentrant.
    pub fn call(&mut self, func: Value, args: &[Value]) -> Value {
        self.vm.call_value(func, args)
    }
}