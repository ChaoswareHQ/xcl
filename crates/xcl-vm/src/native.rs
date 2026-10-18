//! Distributed static registry for native functions, types, and implementations.
//!
//! # Overview
//!
//! XCL allows native Rust functions and types to be exposed to the scripting
//! language with zero central boilerplate using [`linkme::distributed_slice`].
//!
//! Any crate or plugin can define a native function or type and register it
//! into one of the three distributed slices:
//!
//! * [`NATIVE_REGISTRY`] - Static list of native functions (pure functions or context-aware methods).
//! * [`TYPE_REGISTRY`] - Static list of custom types and record/sum definitions.
//! * [`TRAIT_REGISTRY`] - Static list of trait interface definitions.
//! * [`IMPL_REGISTRY`] - Static list of trait implementation blocks for types.
//!
//! When a [`crate::vm::VM`] starts up, calling [`crate::vm::VM::register_all`]
//! scans these slices and populates the VM's internal dispatch tables.
//!
//! # Examples
//!
//! Inspecting registered native entries:
//!
//! ```
//! use xcl_vm::native::NATIVE_REGISTRY;
//!
//! // Iterate over all statically registered native functions:
//! for entry in NATIVE_REGISTRY.iter() {
//!     let domain = entry.domain.unwrap_or("global");
//!     println!("Found native method: {}.{}", domain, entry.method);
//! }
//! ```

pub use crate::value::{
    extract_arg, ImplDescriptor, ImplMethod, NamedParam, NativeEntry, NativeKind, RawPtr,
    TraitDescriptor, TraitMethod, TypeDescriptor, TypeField, TypeKind, TypeVariant, Value,
    SUBTYPE_ERROR, SUBTYPE_HANDLE, SUBTYPE_NONE, SUBTYPE_RESULT, SUBTYPE_USER_BASE, VARIANT_DEFAULT,
};

use linkme::distributed_slice;

/// Compile-time distributed registry of native XCL functions.
///
/// Native functions decorated with registration macros (or explicitly added via
/// `#[linkme::distributed_slice(NATIVE_REGISTRY)]`) are placed into this slice
/// by the linker.
///
/// The VM loads these entries during [`crate::vm::VM::register_all_natives`].
///
/// # Examples
///
/// ```
/// use xcl_vm::native::NATIVE_REGISTRY;
///
/// let native_count = NATIVE_REGISTRY.len();
/// println!("Total registered native functions: {}", native_count);
/// ```
#[distributed_slice]
pub static NATIVE_REGISTRY: [NativeEntry] = [..];

/// Compile-time distributed registry of custom XCL types.
///
/// Records type metadata, field definitions, and variant structures registered
/// by core domains or third-party plugins.
///
/// The VM loads these entries during [`crate::vm::VM::register_all_types`].
///
/// # Examples
///
/// ```
/// use xcl_vm::native::TYPE_REGISTRY;
///
/// for ty in TYPE_REGISTRY.iter() {
///     println!("Registered type: {}", ty.name);
/// }
/// ```
#[distributed_slice]
pub static TYPE_REGISTRY: [TypeDescriptor] = [..];

/// Compile-time distributed registry of trait interface definitions.
///
/// Describes trait contracts with method signatures, allowing XCL to check
/// trait conformance and generate documentation.
///
/// # Examples
///
/// ```
/// use xcl_vm::native::TRAIT_REGISTRY;
///
/// for trait_ in TRAIT_REGISTRY.iter() {
///     println!("Trait {} has {} methods", trait_.name, trait_.methods.len());
/// }
/// ```
#[distributed_slice]
pub static TRAIT_REGISTRY: [TraitDescriptor] = [..];

/// Compile-time distributed registry of trait implementation blocks.
///
/// Maps methods to types and traits, allowing XCL to dispatch trait methods
/// on native or script types.
///
/// # Examples
///
/// ```
/// use xcl_vm::native::IMPL_REGISTRY;
///
/// for imp in IMPL_REGISTRY.iter() {
///     println!("Type {} implements trait {}", imp.type_name, imp.trait_name);
/// }
/// ```
#[distributed_slice]
pub static IMPL_REGISTRY: [ImplDescriptor] = [..];

// — Universal methods (auto-registered, no macro needed) —

/// Universal `type` method — returns the type name of any value.
#[distributed_slice(NATIVE_REGISTRY)]
#[allow(non_upper_case_globals)]
static __XCL_UNIVERSAL_TYPE: NativeEntry = NativeEntry {
    domain: None,
    impl_type: None,
    override_method: None,
    method: "type",
    kind: NativeKind::Pure,
    ptr: RawPtr(__xcl_universal_type as *const ()),
    args: None,
    ret: Some("str"),
    named: &[],
    summary: Some("Return the type name of any value."),
    arg_desc: &[],
    auto_import: false,
};

fn __xcl_universal_type(args: &[Value]) -> Value {
    let val = extract_arg::<Value>(args, 0);
    if let Some(subtype) = val.map_subtype() {
        if subtype == SUBTYPE_RESULT {
            return Value::from_str_boxed("Result".to_string());
        }
        if subtype == SUBTYPE_ERROR {
            return Value::from_str_boxed("Error".to_string());
        }
        if subtype == SUBTYPE_HANDLE {
            return Value::from_str_boxed("Handle".to_string());
        }
    }
    if val.as_map().is_some() {
        for desc in TYPE_REGISTRY.iter() {
            if desc.kind == TypeKind::Record {
                if let Some(map) = val.as_map() {
                    let mut matches = true;
                    for field in desc.fields {
                        if !map.contains_key(field.name) {
                            matches = false;
                            break;
                        }
                    }
                    if matches && !desc.fields.is_empty() {
                        return Value::from_str_boxed(desc.name.to_string());
                    }
                }
            }
        }
    }
    if val.is_int() {
        return Value::from_str_boxed("int".to_string());
    }
    if val.is_float() {
        return Value::from_str_boxed("float".to_string());
    }
    if val.is_bool() {
        return Value::from_str_boxed("bool".to_string());
    }
    if val.is_nil() {
        return Value::from_str_boxed("none".to_string());
    }
    if val.as_str().is_some() {
        return Value::from_str_boxed("str".to_string());
    }
    if val.as_array().is_some() {
        return Value::from_str_boxed("array".to_string());
    }
    if val.as_map().is_some() {
        return Value::from_str_boxed("map".to_string());
    }
    if val.as_func().is_some() {
        return Value::from_str_boxed("function".to_string());
    }
    Value::from_str_boxed("unknown".to_string())
}

/// Universal `super` method — returns the parent (base) struct of a composed type.
#[distributed_slice(NATIVE_REGISTRY)]
#[allow(non_upper_case_globals)]
static __XCL_UNIVERSAL_SUPER: NativeEntry = NativeEntry {
    domain: None,
    impl_type: None,
    override_method: None,
    method: "super",
    kind: NativeKind::Pure,
    ptr: RawPtr(__xcl_universal_super as *const ()),
    args: None,
    ret: Some("map"),
    named: &[],
    summary: Some("Return the parent (base) struct of a composed type via super."),
    arg_desc: &[],
    auto_import: false,
};

fn __xcl_universal_super(args: &[Value]) -> Value {
    let val = extract_arg::<Value>(args, 0);
    if let Some(map) = val.as_map() {
        let mut best_match: Option<&TypeDescriptor> = None;
        let mut best_field_count = 0usize;
        for desc in TYPE_REGISTRY.iter() {
            if desc.kind == TypeKind::Record {
                let mut matches = true;
                for field in desc.fields {
                    if !map.contains_key(field.name) {
                        matches = false;
                        break;
                    }
                }
                if matches && !desc.fields.is_empty() {
                    if desc.fields.len() > best_field_count {
                        best_field_count = desc.fields.len();
                        best_match = Some(desc);
                    }
                }
            }
        }
        if let Some(desc) = best_match {
            if let Some(parent_name) = desc.parent_type {
                for parent_desc in TYPE_REGISTRY.iter() {
                    if parent_desc.name == parent_name {
                        let mut parent_map = std::collections::HashMap::new();
                        for field in parent_desc.fields {
                            if let Some(v) = map.get(field.name) {
                                parent_map.insert(field.name.to_string(), *v);
                            }
                        }
                        return Value::from_map_boxed_typed_v(
                            parent_map,
                            SUBTYPE_USER_BASE,
                            VARIANT_DEFAULT,
                        );
                    }
                }
            }
        }
    }
    Value::nil()
}
