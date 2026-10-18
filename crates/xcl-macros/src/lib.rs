//! XCL Macros — Derive and impl attribute macros for registering types and methods with the VM.
//!
//! # Supported macros
//!
//! - `#[derive(Xcl)]` — Register a struct or enum in `TYPE_REGISTRY`.
//! - `#[xcl_impl]` — Register an impl block in `IMPL_REGISTRY` (trait impl) or `NATIVE_REGISTRY` (inherent impl).
//! - `xcl_universal_methods!()` — Register universal methods (`type`, `super`) available on all values.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemImpl, LitStr, Type, parse_macro_input};

/// Extract the first line of `///` doc comments as a summary string.
fn extract_doc_summary(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let syn::Meta::NameValue(nv) = &attr.meta {
                if let syn::Expr::Lit(el) = &nv.value {
                    if let syn::Lit::Str(lit) = &el.lit {
                        let line = lit.value().trim().to_string();
                        if !line.is_empty() {
                            return Some(line);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Parse helper attributes `#[xcl(domain = "...", kind = "...", summary = "...", super = "...")]`
/// from a DeriveInput's attrs.
fn parse_xcl_helper_attrs(input: &syn::DeriveInput) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let mut domain = None;
    let mut kind = None;
    let mut summary = None;
    let mut super_type = None;

    for attr in &input.attrs {
        if attr.path().is_ident("xcl") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let parsed = meta_list.parse_args_with(|input: syn::parse::ParseStream<'_>| {
                    let mut d = None;
                    let mut k = None;
                    let mut s = None;
                    let mut sup = None;
                    while !input.is_empty() {
                        let lookahead = input.lookahead1();
                        let ident: syn::Ident = if lookahead.peek(syn::Token![super]) {
                            let _kw: syn::Token![super] = input.parse()?;
                            syn::Ident::new("super", proc_macro2::Span::call_site())
                        } else {
                            input.parse()?
                        };
                        let _eq: syn::Token![=] = input.parse()?;
                        let lit: LitStr = input.parse()?;
                        match ident.to_string().as_str() {
                            "domain" => d = Some(lit.value()),
                            "kind" => k = Some(lit.value()),
                            "summary" => s = Some(lit.value()),
                            "super" => sup = Some(lit.value()),
                            _ => {}
                        }
                        if input.peek(syn::Token![,]) {
                            input.parse::<syn::Token![,]>()?;
                        }
                    }
                    Ok((d, k, s, sup))
                });
                if let Ok((d, k, s, sup)) = parsed {
                    domain = d.or(domain);
                    kind = k.or(kind);
                    summary = s.or(summary);
                    super_type = sup.or(super_type);
                }
            }
        }
    }

    // Auto-detect summary from doc comments if not provided
    if summary.is_none() {
        summary = extract_doc_summary(&input.attrs);
    }

    (domain, kind, summary, super_type)
}

/// Convert a Rust type to an XCL type string.
fn rust_type_to_xcl_type(ty: &Type) -> String {
    match ty {
        Type::Path(type_path) => {
            if let Some(seg) = type_path.path.segments.last() {
                let name = seg.ident.to_string();
                match name.as_str() {
                    "String" => "str".to_string(),
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => "int".to_string(),
                    "f32" | "f64" => "float".to_string(),
                    "bool" => "bool".to_string(),
                    "Value" => "any".to_string(),
                    _ => name,
                }
            } else {
                "any".to_string()
            }
        }
        Type::Reference(type_ref) => rust_type_to_xcl_type(&type_ref.elem),
        _ => "any".to_string(),
    }
}

/// `#[derive(Xcl)]` — Auto-registers a struct or enum in `TYPE_REGISTRY`.
///
/// Supports optional `#[xcl(...)]` helper attributes:
/// - `domain = "..."` — override the auto-detected domain.
/// - `kind = "Record" | "Opaque" | "Sum"` — override the type kind.
/// - `summary = "..."` — override the auto-detected summary.
/// - `super = "..."` — set the parent type for inheritance.
///
/// # Example
///
/// ```ignore
/// #[derive(Xcl)]
/// pub struct MyType;
///
/// #[derive(Xcl)]
/// #[xcl(kind = "Sum")]
/// pub enum ResultType {
///     Ok(Value),
///     Err(Error),
/// }
/// ```
#[proc_macro_derive(Xcl, attributes(xcl))]
pub fn xcl_derive(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    let (domain, kind, summary, super_type) = parse_xcl_helper_attrs(&input);

    let domain_expr = match domain {
        Some(d) => quote!(Some(#d)),
        None => quote!(None),
    };

    let summary_expr = match summary {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    let parent_type_expr = match super_type {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    match &input.data {
        syn::Data::Struct(data_struct) => {
            let statik = syn::Ident::new(
                &format!("__XCL_TYPE_{}", name_str.to_uppercase()),
                proc_macro2::Span::call_site(),
            );

            let (kind_expr, fields_tokens) = match &data_struct.fields {
                syn::Fields::Named(fields) => {
                    let kind_expr = match kind.as_deref() {
                        Some("Record") => quote!(::xcl_vm::TypeKind::Record),
                        Some("Opaque") => quote!(::xcl_vm::TypeKind::Opaque),
                        _ => quote!(::xcl_vm::TypeKind::Record),
                    };

                    if kind.as_deref() == Some("Opaque") {
                        (kind_expr, quote!(&[]))
                    } else {
                        let fs_entries = fields.named.iter().map(|f| {
                            let field_name = f.ident.as_ref().unwrap().to_string();
                            let field_ty = rust_type_to_xcl_type(&f.ty);
                            quote! {
                                ::xcl_vm::TypeField {
                                    name: #field_name,
                                    ty: #field_ty,
                                    optional: false,
                                }
                            }
                        });
                        (kind_expr, quote!(&[#(#fs_entries),*]))
                    }
                }
                syn::Fields::Unnamed(_) => {
                    (quote!(::xcl_vm::TypeKind::Opaque), quote!(&[]))
                }
                syn::Fields::Unit => {
                    (quote!(::xcl_vm::TypeKind::Opaque), quote!(&[]))
                }
            };

            let expanded = quote! {
                #[::linkme::distributed_slice(::xcl_vm::TYPE_REGISTRY)]
                #[allow(non_upper_case_globals)]
                static #statik: ::xcl_vm::TypeDescriptor = ::xcl_vm::TypeDescriptor {
                    name: #name_str,
                    domain: #domain_expr,
                    kind: #kind_expr,
                    fields: #fields_tokens,
                    variants: &[],
                    summary: #summary_expr,
                    parent_type: #parent_type_expr,
                };

                #[allow(dead_code)]
                const _: Option<#name> = None;
            };
            expanded.into()
        }
        syn::Data::Enum(data_enum) => {
            let statik = syn::Ident::new(
                &format!("__XCL_TYPE_{}", name_str.to_uppercase()),
                proc_macro2::Span::call_site(),
            );

            let kind_expr = match kind.as_deref() {
                Some("Sum") => quote!(::xcl_vm::TypeKind::Sum),
                _ => quote!(::xcl_vm::TypeKind::Sum),
            };

            let variants_tokens = if data_enum.variants.is_empty() {
                quote!(&[])
            } else {
                let entries = data_enum.variants.iter().map(|v| {
                    let variant_name = v.ident.to_string();
                    match &v.fields {
                        syn::Fields::Named(fields) => {
                            let fs_entries = fields.named.iter().map(|f| {
                                let field_name = f.ident.as_ref().unwrap().to_string();
                                let field_ty = rust_type_to_xcl_type(&f.ty);
                                quote! {
                                    ::xcl_vm::TypeField {
                                        name: #field_name,
                                        ty: #field_ty,
                                        optional: false,
                                    }
                                }
                            });
                            quote! {
                                ::xcl_vm::TypeVariant {
                                    name: #variant_name,
                                    fields: &#[#(#fs_entries),*],
                                }
                            }
                        }
                        syn::Fields::Unnamed(fields) => {
                            let fs_entries = fields.unnamed.iter().map(|f| {
                                let field_ty = rust_type_to_xcl_type(&f.ty);
                                quote! {
                                    ::xcl_vm::TypeField {
                                        name: "value",
                                        ty: #field_ty,
                                        optional: false,
                                    }
                                }
                            });
                            quote! {
                                ::xcl_vm::TypeVariant {
                                    name: #variant_name,
                                    fields: &#[#(#fs_entries),*],
                                }
                            }
                        }
                        syn::Fields::Unit => {
                            quote! {
                                ::xcl_vm::TypeVariant {
                                    name: #variant_name,
                                    fields: &[],
                                }
                            }
                        }
                    }
                });
                quote!(&[#(#entries),*])
            };

            let expanded = quote! {
                #[::linkme::distributed_slice(::xcl_vm::TYPE_REGISTRY)]
                #[allow(non_upper_case_globals)]
                static #statik: ::xcl_vm::TypeDescriptor = ::xcl_vm::TypeDescriptor {
                    name: #name_str,
                    domain: #domain_expr,
                    kind: #kind_expr,
                    fields: &[],
                    variants: #variants_tokens,
                    summary: #summary_expr,
                    parent_type: #parent_type_expr,
                };

                #[allow(dead_code)]
                const _: Option<#name> = None;
            };
            expanded.into()
        }
        syn::Data::Union(_) => {
            syn::Error::new_spanned(
                name,
                "#[derive(Xcl)] is not supported for unions yet",
            )
            .to_compile_error()
            .into()
        }
    }
}

/// `#[xcl_impl]` — Auto-registers a Rust impl block in `IMPL_REGISTRY` and/or `NATIVE_REGISTRY`.
///
/// Supports two modes:
///
/// 1. **Trait impl** (`impl Trait for Type`): Registers in `IMPL_REGISTRY` with method names.
/// 2. **Inherent impl** (`impl Type`): Registers each method in `NATIVE_REGISTRY` as a native
///    method bound to the type's domain.
///
/// # Example — Trait impl
///
/// ```ignore
/// #[xcl_impl]
/// impl CloudService for CloudflareProvider {
///     fn bucket_create() { }
///     fn bucket_list() { }
/// }
/// ```
///
/// # Example — Inherent impl with `&self` and `Self`
///
/// ```ignore
/// #[xcl_impl]
/// impl Player {
///     pub fn get_id(&self) -> i64 { self.id }
///     pub fn rename(&self, new_name: String) -> Self {
///         Player { base: self.base.clone(), username: new_name }
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn xcl_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let impl_item = parse_macro_input!(item as ItemImpl);

    let type_name = match impl_item.self_ty.as_ref() {
        syn::Type::Path(type_path) => {
            if let Some(seg) = type_path.path.segments.last() {
                seg.ident.to_string()
            } else {
                return syn::Error::new_spanned(&impl_item.self_ty, "Cannot extract type name from impl block")
                    .to_compile_error()
                    .into();
            }
        }
        _ => {
            return syn::Error::new_spanned(&impl_item.self_ty, "Cannot extract type name from impl block")
                .to_compile_error()
                .into();
        }
    };

    let _type_path: syn::Type = match impl_item.self_ty.as_ref() {
        syn::Type::Path(type_path) => {
            let segments = type_path.path.segments.iter().collect::<Vec<_>>();
            let mut new_segments = syn::punctuated::Punctuated::new();
            new_segments.push(syn::PathSegment::from(syn::Ident::new("self", proc_macro2::Span::call_site())));
            for seg in segments {
                new_segments.push(seg.clone());
            }
            syn::Type::Path(syn::TypePath {
                qself: None,
                path: syn::Path {
                    leading_colon: None,
                    segments: new_segments,
                },
                attrs: Vec::new(),
            })
        }
        _ => syn::Type::Path(syn::TypePath {
            qself: None,
            path: syn::Path::from(syn::Ident::new(&type_name, proc_macro2::Span::call_site())),
            attrs: Vec::new(),
        }),
    };
    let _ = _type_path;

    // Check if this is a trait impl or inherent impl
    if let Some((trait_path, _)) = &impl_item.trait_ {
        // Trait impl mode: register in IMPL_REGISTRY
        let trait_name = if let Some(seg) = trait_path.segments.last() {
            seg.ident.to_string()
        } else {
            return syn::Error::new_spanned(trait_path, "Cannot extract trait name")
                .to_compile_error()
                .into();
        };

        let mut method_names: Vec<String> = Vec::new();
        for item in &impl_item.items {
            if let syn::ImplItem::Fn(iif) = item {
                method_names.push(iif.sig.ident.to_string());
            }
        }

        let methods_tokens = if method_names.is_empty() {
            quote!(&[])
        } else {
            let entries = method_names.iter().map(|m_name| {
                quote! {
                    ::xcl_vm::ImplMethod {
                        name: #m_name,
                    }
                }
            });
            quote!(&[#(#entries),*])
        };

        let statik = syn::Ident::new(
            &format!("__XCL_IMPL_{}_{}", type_name.to_uppercase(), trait_name.to_uppercase()),
            proc_macro2::Span::call_site(),
        );

        let expanded = quote! {
            #impl_item

            #[::linkme::distributed_slice(::xcl_vm::IMPL_REGISTRY)]
            #[allow(non_upper_case_globals)]
            static #statik: ::xcl_vm::ImplDescriptor = ::xcl_vm::ImplDescriptor {
                type_name: #type_name,
                trait_name: #trait_name,
                methods: #methods_tokens,
            };
        };

        expanded.into()
    } else {
        // Inherent impl mode: register each method in NATIVE_REGISTRY
        let mut native_entries: Vec<proc_macro2::TokenStream> = Vec::new();

        for item in &impl_item.items {
            if let syn::ImplItem::Fn(iif) = item {
                let method_name = &iif.sig.ident;
                let method_name_str = method_name.to_string();

                let summary = extract_doc_summary(&iif.attrs);
                let summary_expr = match summary {
                    Some(s) => quote!(Some(#s)),
                    None => quote!(None),
                };

                // Check if method takes &self
                let has_self = iif.sig.inputs.first().map_or(false, |arg| {
                    if let syn::FnArg::Typed(pt) = arg {
                        matches!(pt.ty.as_ref(), syn::Type::Reference(_))
                    } else {
                        false
                    }
                });

                // Check return type
                let ret_type = match &iif.sig.output {
                    syn::ReturnType::Type(_, ty) => {
                        let ty_str = rust_type_to_xcl_type(ty);
                        quote!(Some(#ty_str))
                    }
                    syn::ReturnType::Default => quote!(None),
                };

                let statik = syn::Ident::new(
                    &format!("__XCL_NATIVE_{}_{}", type_name.to_uppercase(), method_name_str.to_uppercase()),
                    proc_macro2::Span::call_site(),
                );

                if has_self {
                    // Method takes &self — generate wrapper
                    let wrapper_name = syn::Ident::new(
                        &format!("__xcl_wrapper_{}_{}", type_name.to_lowercase(), method_name_str),
                        proc_macro2::Span::call_site(),
                    );

                    let arg_indices: Vec<usize> = (1..iif.sig.inputs.len()).collect();

                    let arg_extracts: Vec<proc_macro2::TokenStream> = arg_indices.iter().enumerate().map(|(i, idx)| {
                        let input = &iif.sig.inputs[*idx];
                        if let syn::FnArg::Typed(pt) = input {
                            let arg_type = &pt.ty;
                            let arg_idx = syn::Index::from(i + 1);
                            quote! {
                                let _arg = ::xcl_vm::extract_arg::<#arg_type>(args, #arg_idx);
                            }
                        } else {
                            quote! {}
                        }
                    }).collect();

                    native_entries.push(quote! {
                        #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
                        #[allow(non_upper_case_globals)]
                        static #statik: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
                            domain: None,
                            impl_type: Some(#type_name),
                            override_method: None,
                            method: #method_name_str,
                            kind: ::xcl_vm::NativeKind::Pure,
                            ptr: ::xcl_vm::RawPtr(#wrapper_name as *const ()),
                            args: None,
                            ret: #ret_type,
                            named: &[],
                            summary: #summary_expr,
                            arg_desc: &[],
                        };

                        fn #wrapper_name(args: &[::xcl_vm::Value]) -> ::xcl_vm::Value {
                            let self_val = ::xcl_vm::extract_arg::<::xcl_vm::Value>(args, 0);
                            #(#arg_extracts)*
                            // TODO: deserialize self_val and call the method
                            ::xcl_vm::Value::nil()
                        }
                    });
                } else {
                    // Static method — no self
                    native_entries.push(quote! {
                        #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
                        #[allow(non_upper_case_globals)]
                        static #statik: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
                            domain: None,
                            impl_type: Some(#type_name),
                            override_method: None,
                            method: #method_name_str,
                            kind: ::xcl_vm::NativeKind::Pure,
                            ptr: ::xcl_vm::RawPtr(#method_name as *const ()),
                            args: None,
                            ret: #ret_type,
                            named: &[],
                            summary: #summary_expr,
                            arg_desc: &[],
                        };
                    });
                }
            }
        }

        let expanded = quote! {
            #impl_item

            #(#native_entries)*
        };

        expanded.into()
    }
}

/// `xcl_universal_methods!()` — Registers universal methods available on all XCL types.
///
/// Universal methods include:
/// - `type` — Returns the type name of the value as a string.
/// - `super` — Returns the parent (base) struct of a composed type.
///
/// # Example
///
/// ```ignore
/// xcl_macros::xcl_universal_methods!();
/// ```
#[proc_macro]
pub fn xcl_universal_methods(_item: TokenStream) -> TokenStream {
    let expanded = quote! {
        /// Universal `type` method — returns the type name of any value.
        #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static __XCL_UNIVERSAL_TYPE: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
            domain: None,
            impl_type: None,
            override_method: None,
            method: "type",
            kind: ::xcl_vm::NativeKind::Pure,
            ptr: ::xcl_vm::RawPtr(__xcl_universal_type as *const ()),
            args: None,
            ret: Some("str"),
            named: &[],
            summary: Some("Return the type name of any value."),
            arg_desc: &[],
        };

        fn __xcl_universal_type(args: &[::xcl_vm::Value]) -> ::xcl_vm::Value {
            let val = ::xcl_vm::extract_arg::<::xcl_vm::Value>(args, 0);
            if let Some(subtype) = val.map_subtype() {
                if subtype == ::xcl_vm::SUBTYPE_RESULT {
                    return ::xcl_vm::Value::from_str_boxed("Result".to_string());
                }
                if subtype == ::xcl_vm::SUBTYPE_ERROR {
                    return ::xcl_vm::Value::from_str_boxed("Error".to_string());
                }
                if subtype == ::xcl_vm::SUBTYPE_HANDLE {
                    return ::xcl_vm::Value::from_str_boxed("Handle".to_string());
                }
            }
            if val.as_map().is_some() {
                for desc in ::xcl_vm::TYPE_REGISTRY.iter() {
                    if desc.kind == ::xcl_vm::TypeKind::Record {
                        if let Some(map) = val.as_map() {
                            let mut matches = true;
                            for field in desc.fields {
                                if !map.contains_key(field.name) {
                                    matches = false;
                                    break;
                                }
                            }
                            if matches && !desc.fields.is_empty() {
                                return ::xcl_vm::Value::from_str_boxed(desc.name.to_string());
                            }
                        }
                    }
                }
            }
            if val.is_int() {
                return ::xcl_vm::Value::from_str_boxed("int".to_string());
            }
            if val.is_float() {
                return ::xcl_vm::Value::from_str_boxed("float".to_string());
            }
            if val.is_bool() {
                return ::xcl_vm::Value::from_str_boxed("bool".to_string());
            }
            if val.is_nil() {
                return ::xcl_vm::Value::from_str_boxed("none".to_string());
            }
            if val.as_str().is_some() {
                return ::xcl_vm::Value::from_str_boxed("str".to_string());
            }
            if val.as_array().is_some() {
                return ::xcl_vm::Value::from_str_boxed("array".to_string());
            }
            if val.as_map().is_some() {
                return ::xcl_vm::Value::from_str_boxed("map".to_string());
            }
            if val.as_func().is_some() {
                return ::xcl_vm::Value::from_str_boxed("function".to_string());
            }
            ::xcl_vm::Value::from_str_boxed("unknown".to_string())
        }

        /// Universal `super` method — returns the parent (base) struct of a composed type.
        #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static __XCL_UNIVERSAL_SUPER: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
            domain: None,
            impl_type: None,
            override_method: None,
            method: "super",
            kind: ::xcl_vm::NativeKind::Pure,
            ptr: ::xcl_vm::RawPtr(__xcl_universal_super as *const ()),
            args: None,
            ret: Some("map"),
            named: &[],
            summary: Some("Return the parent (base) struct of a composed type via super."),
            arg_desc: &[],
        };

        fn __xcl_universal_super(args: &[::xcl_vm::Value]) -> ::xcl_vm::Value {
            let val = ::xcl_vm::extract_arg::<::xcl_vm::Value>(args, 0);
            if let Some(map) = val.as_map() {
                let mut best_match: Option<&::xcl_vm::TypeDescriptor> = None;
                let mut best_field_count = 0usize;
                for desc in ::xcl_vm::TYPE_REGISTRY.iter() {
                    if desc.kind == ::xcl_vm::TypeKind::Record {
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
                        for parent_desc in ::xcl_vm::TYPE_REGISTRY.iter() {
                            if parent_desc.name == parent_name {
                                let mut parent_map = std::collections::HashMap::new();
                                for field in parent_desc.fields {
                                    if let Some(v) = map.get(field.name) {
                                        parent_map.insert(field.name.to_string(), *v);
                                    }
                                }
                                return ::xcl_vm::Value::from_map_boxed_typed_v(
                                    parent_map,
                                    ::xcl_vm::SUBTYPE_USER_BASE,
                                    ::xcl_vm::VARIANT_DEFAULT,
                                );
                            }
                        }
                    }
                }
            }
            ::xcl_vm::Value::nil()
        }
    };

    expanded.into()
}
