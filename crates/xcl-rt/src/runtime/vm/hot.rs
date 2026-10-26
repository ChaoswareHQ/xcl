//! Hot native call slots.
//!
//! A native whose registration says `hot` gets a numbered slot when the VM
//! starts. The compiler emits `OP_CALL_HOT slot` for a call site whose callee
//! name has a slot, so dispatch becomes one table index and one indirect call —
//! no name hash, no global table probe, no string table read.
//!
//! # Why the slot table lives in the VM
//!
//! A slot number is only meaningful to the VM that assigned it, and the VM is
//! built from the linked plugin registries. The SDK's evaluate path therefore
//! compiles with the same VM it then runs on, which is the only combination
//! that can bind slots. `xcl_toolchain::bytecode::compile` has no VM and emits
//! `OP_CALL_NAME` for every call.
//!
//! Nothing else is per-VM: a slot binds only names that ordinary resolution
//! already reaches the same function for, so `OP_CALL_HOT` and `OP_CALL_NAME`
//! call the same code. The instruction is an optimisation, not a new calling
//! convention.

use super::*;
use crate::runtime::value::{BuiltinCtxFn, BuiltinFn, NativeKind};

/// One entry of [`VM`]'s hot-native table.
#[derive(Clone, Copy)]
pub struct HotNative {
    /// Flat domain key (`"util"`), or `None` for a domain-less native.
    pub domain: Option<&'static str>,
    /// Method name as registered (`"print"`, `"metadata.get"`).
    pub method: &'static str,
    /// The registered implementation.
    pub call: BuiltinImpl,
    /// Capabilities declared by the registering macro.
    pub caps: crate::runtime::primitives::CapSet,
}

impl HotNative {
    /// The canonical spelling (`"util.print"`, or `"defer"` without a domain).
    pub fn name(&self) -> String {
        match self.domain {
            Some(domain) => format!("{domain}.{}", self.method),
            None => self.method.into(),
        }
    }
}

/// Callable spelling → slot in the VM's hot-native table.
///
/// Built once per VM from the registered natives. Spellings include the bare
/// method (for auto-imported natives), the flat `domain.method` name, and every
/// plugin path that publishes the domain (`std.util.print`), so a program gets
/// the hot call whichever way it names the function.
#[derive(Default, Clone)]
pub struct HotTable {
    slots: HashMap<Box<str>, u16>,
    /// Capabilities per slot, indexed like `VM::hot_natives`.
    caps: Vec<crate::runtime::primitives::CapSet>,
}

impl HotTable {
    /// Slot for a callable spelling, or `None` when the name is not hot.
    #[inline]
    pub fn slot(&self, name: &str) -> Option<u16> {
        self.slots.get(name).copied()
    }

    /// Whether the native in `slot` is declared pure and deterministic.
    ///
    /// That is the licence for the optimizer to evaluate it once instead of
    /// once per loop iteration: the result must not depend on VM state and
    /// must not be observable. `print` never qualifies; a side-effect-free
    /// accessor does.
    pub fn is_pure(&self, slot: u16) -> bool {
        use crate::runtime::primitives::Cap;
        self.caps
            .get(slot as usize)
            .is_some_and(|caps| caps.has(Cap::Pure) && caps.has(Cap::Deterministic))
    }

    /// Number of bound spellings.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether any native is hot.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Binds `name` to `slot` unless another entry already claimed it.
    fn bind(&mut self, name: Box<str>, slot: u16) {
        self.slots.entry(name).or_insert(slot);
    }
}

/// Interprets a registered entry's pointer as its calling convention.
pub(crate) fn native_impl(entry: &NativeEntry) -> BuiltinImpl {
    match entry.kind {
        NativeKind::Pure => {
            let f: BuiltinFn = unsafe { core::mem::transmute(entry.ptr.0) };
            BuiltinImpl::Pure(f)
        }
        NativeKind::Ctx => {
            let f: BuiltinCtxFn = unsafe { core::mem::transmute(entry.ptr.0) };
            BuiltinImpl::Ctx(f)
        }
    }
}

impl VM {
    /// The slot table the compiler uses to decide between `OP_CALL_HOT` and
    /// `OP_CALL_NAME`.
    pub fn hot_table(&self) -> &HotTable {
        &self.cold.hot_table
    }

    /// Registered hot natives, indexed by instruction operand `b`.
    pub fn hot_natives(&self) -> &[HotNative] {
        &self.cold.hot_natives
    }

    /// The native bound to `slot`, if the slot is populated.
    #[inline]
    pub fn hot_native(&self, slot: usize) -> Option<HotNative> {
        self.cold.hot_natives.get(slot).copied()
    }

    /// Resolves a callable name to its native implementation.
    ///
    /// This is how a named call finds a builtin: the exact spelling first
    /// (`print`, `util.print`, `std.util.print`), then each shorter domain
    /// prefix so a dotted method under a qualified path (`std.util.print`)
    /// still lands on `util` + `print`.
    pub fn native_entry(&self, name: &str) -> Option<BuiltinImpl> {
        if let Some(native) = self.resolve_builtin(name) {
            return Some(*native);
        }
        let mut end = name.len();
        while let Some(pos) = name[..end].rfind('.') {
            let (domain, method) = (&name[..pos], &name[pos + 1..]);
            if !domain.is_empty()
                && !method.is_empty()
                && let Some(native) = self.resolve_domain_method(domain, method)
            {
                return Some(*native);
            }
            end = pos;
        }
        None
    }

    /// Rebuilds [`VM::hot_table`] and [`VM::hot_natives`] from the registered
    /// entries.
    ///
    /// Runs after every [`VM::register_natives`] call, so it is incremental:
    /// the tables are always a pure function of the entries registered so far.
    ///
    /// A spelling is bound only when [`VM::native_entry`] already resolves it to
    /// the same function, which keeps `OP_CALL_HOT` behaviourally identical to
    /// `OP_CALL_NAME` even when two plugins publish the same method name.
    pub(super) fn rebuild_hot_table(&mut self) {
        let mut table = HotTable::default();
        let mut natives = Vec::new();
        for i in 0..self.cold.native_entries.len() {
            let entry = self.cold.native_entries[i];
            if !entry.hot {
                continue;
            }
            let call = native_impl(entry);
            let names: Vec<Box<str>> = self
                .callable_spellings(entry)
                .into_iter()
                .filter(|name| self.native_entry(name).map(BuiltinImpl::ptr) == Some(entry.ptr.0))
                .collect();
            if names.is_empty() {
                continue;
            }
            let slot = natives.len() as u16;
            for name in names {
                table.bind(name, slot);
            }
            table.caps.push(entry.caps);
            natives.push(HotNative {
                domain: entry.domain,
                method: entry.method,
                call,
                caps: entry.caps,
            });
        }
        self.cold.hot_table = table;
        self.cold.hot_natives = natives;
    }

    /// Every spelling a call site can use to reach `entry`.
    fn callable_spellings(&self, entry: &NativeEntry) -> Vec<Box<str>> {
        let mut out = Vec::with_capacity(3);
        if entry.auto_import || entry.domain.is_none() {
            out.push(entry.method.into());
        }
        if let Some(domain) = entry.domain {
            out.push(format!("{domain}.{}", entry.method).into());
            for &path in self.cold.domain_alias.keys() {
                if self.cold.domain_alias.get(path).copied() == Some(domain) {
                    out.push(format!("{path}.{}", entry.method).into());
                }
            }
        }
        out
    }
}
