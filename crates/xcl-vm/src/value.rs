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

const KIND_STR:   u8 = 0;
const KIND_ARRAY: u8 = 1;
const KIND_MAP:   u8 = 2;
const KIND_FUNC:  u8 = 3;

#[repr(C)]
pub struct HeapHeader { pub kind: u8, pub flags: u8, pub _pad: [u8; 6] }

#[repr(C)]
pub struct HeapStr   { pub header: HeapHeader, pub data: String }
#[repr(C)]
pub struct HeapArray { pub header: HeapHeader, pub data: Vec<Value> }
#[repr(C)]
pub struct HeapMap   { pub header: HeapHeader, pub data: HashMap<String, Value> }
#[repr(C)]
pub struct HeapFunc  { pub header: HeapHeader, pub data: FuncValue }

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Value(pub u64);

impl PartialEq for Value {
    #[inline] fn eq(&self, other: &Self) -> bool { Value::eq(*self, *other) }
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
            header: HeapHeader { kind: KIND_MAP, flags: 0, _pad: [0; 6] },
            data: m,
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

    pub fn sub(self, other: Self) -> Self {
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

    pub fn div(self, other: Self) -> Self {
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

    pub fn eq(self, other: Self) -> bool {
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

    #[inline] pub fn not(self) -> Self { Self::bool(!self.is_truthy()) }

    #[inline]
    pub fn neg(self) -> Self {
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
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
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