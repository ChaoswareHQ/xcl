//! Core language axioms: `type` and `super` universal methods.
//!
//! These are registered by the standard plugin layer at VM construction time
//! as context-aware native entries (`NativeKind::Ctx`).

use xcl_vm::{
    DomainCtx, NativeEntry, NativeKind, RawPtr, TypeDescriptor, TypeKind, Value,
    SUBTYPE_ERROR, SUBTYPE_HANDLE, SUBTYPE_RESULT, SUBTYPE_USER_BASE, VARIANT_DEFAULT,
};

fn axiom_type(ctx: &mut DomainCtx<'_>) -> Value {
    let val = ctx.arg(0);
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
        let entries = ctx.type_entries();
        for desc in entries.iter() {
            let desc = *desc;
            if desc.kind == TypeKind::Record
                && let Some(map) = val.as_map()
            {
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

fn axiom_super(ctx: &mut DomainCtx<'_>) -> Value {
    let val = ctx.arg(0);
    if let Some(map) = val.as_map() {
        let entries = ctx.type_entries();
        let mut best_match: Option<&TypeDescriptor> = None;
        let mut best_field_count = 0usize;
        for desc in entries.iter() {
            let desc = *desc;
            if desc.kind == TypeKind::Record {
                let mut matches = true;
                for field in desc.fields {
                    if !map.contains_key(field.name) {
                        matches = false;
                        break;
                    }
                }
                if matches && !desc.fields.is_empty()
                    && desc.fields.len() > best_field_count
                {
                    best_field_count = desc.fields.len();
                    best_match = Some(desc);
                }
            }
        }
        if let Some(desc) = best_match
            && let Some(parent_name) = desc.parent_type
        {
            for parent_desc in entries.iter() {
                let parent_desc = *parent_desc;
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
    Value::nil()
}

pub(crate) static AXIOM_ENTRIES: [NativeEntry; 2] = [
    NativeEntry {
        domain: None,
        impl_type: None,
        override_method: None,
        method: "type",
        kind: NativeKind::Ctx,
        ptr: RawPtr(axiom_type as *const ()),
        args: None,
        ret: Some("str"),
        named: &[],
        summary: Some("Return the type name of any value."),
        arg_desc: &[],
        auto_import: false,
    },
    NativeEntry {
        domain: None,
        impl_type: None,
        override_method: None,
        method: "super",
        kind: NativeKind::Ctx,
        ptr: RawPtr(axiom_super as *const ()),
        args: None,
        ret: Some("map"),
        named: &[],
        summary: Some("Return the parent (base) struct of a composed type via super."),
        arg_desc: &[],
        auto_import: false,
    },
];
