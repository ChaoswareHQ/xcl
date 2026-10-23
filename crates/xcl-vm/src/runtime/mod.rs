//! VM execution engine: virtual machine, values, and error types.
//!
//! This module handles the final phase of the XCL compilation pipeline:
//! executing compiled bytecode on a stack-based virtual machine with NaN-boxed values.

pub mod opcode;
pub mod primitives;
pub mod value;
pub mod vm;

pub use opcode::{ColdModule, OpcodeEntry, OpcodeFn, OpcodeResult, opcode_name};
pub use primitives::{
    Cap, CapSet, PRIMITIVES, PrimKind, Primitive, capability_name, capability_of_name, caps,
    is_primitive_of, primitive_of_name, primitive_of_value,
};
pub use value::{
    BuiltinImpl, DomainCtx, FeatureDescriptor, FuncValue, GlobalOverrideGuard, HandleKind,
    ImplDescriptor, ImplMethod, IntoValue, NamedParam, NativeArg, NativeEntry, NativeKind, RawPtr,
    SUBTYPE_ERROR, SUBTYPE_HANDLE, SUBTYPE_NONE, SUBTYPE_RANGE, SUBTYPE_RESULT, SUBTYPE_USER_BASE,
    TraitDescriptor, TraitMethod, TypeDescriptor, TypeField, TypeKind, TypeVariant,
    VARIANT_DEFAULT, VARIANT_ERR, VARIANT_OK, Value, XclType, extract_arg,
};
pub use vm::{DomainAlias, DomainSlot, HotNative, HotTable, PluginDescriptor, VM};
