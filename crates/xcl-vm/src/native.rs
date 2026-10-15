pub use crate::value::{
    NativeEntry, NamedParam, NativeKind,
    TypeDescriptor, TypeKind, TypeField, TypeVariant,
    ImplDescriptor, ImplMethod,
};

use linkme::distributed_slice;

#[distributed_slice]
pub static NATIVE_REGISTRY: [NativeEntry] = [..];

#[distributed_slice]
pub static TYPE_REGISTRY: [TypeDescriptor] = [..];

#[distributed_slice]
pub static IMPL_REGISTRY: [ImplDescriptor] = [..];
