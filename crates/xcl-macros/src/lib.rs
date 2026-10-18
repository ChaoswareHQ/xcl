use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, ItemImpl, LitStr, Type, parse_macro_input};

/// Parse helper attributes `#[xcl(domain = "...", kind = "...", summary = "...")]`
/// from a DeriveInput's attrs.
fn parse_xcl_helper_attrs(input: &syn::DeriveInput) -> (Option<String>, Option<String>, Option<String>) {
    let mut domain = None;
    let mut kind = None;
    let mut summary = None;

    // Check for `#[xcl(domain = "...", kind = "...", summary = "...")]`
    for attr in &input.attrs {
        if attr.path().is_ident("xcl") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let parsed = meta_list.parse_args_with(|input: syn::parse::ParseStream<'_>| {
                    let mut d = None;
                    let mut k = None;
                    let mut s = None;
                    while !input.is_empty() {
                        let ident: syn::Ident = input.parse()?;
                        let _eq: syn::Token![=] = input.parse()?;
                        let lit: LitStr = input.parse()?;
                        match ident.to_string().as_str() {
                            "domain" => d = Some(lit.value()),
                            "kind" => k = Some(lit.value()),
                            "summary" => s = Some(lit.value()),
                            _ => {}
                        }
                        if input.peek(syn::Token![,]) {
                            input.parse::<syn::Token![,]>()?;
                        }
                    }
                    Ok((d, k, s))
                });
                if let Ok((d, k, s)) = parsed {
                    domain = d;
                    kind = k;
                    summary = s;
                }
            }
        }
    }

    (domain, kind, summary)
}

/// Convert a Rust type to an XCL type string
fn rust_type_to_xcl_type(ty: &Type) -> String {
    match ty {
        Type::Path(type_path) => {
            if let Some(seg) = type_path.path.segments.last() {
                match seg.ident.to_string().as_str() {
                    "String" => "str".to_string(),
                    "str" => "str".to_string(),
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => "int".to_string(),
                    "f32" | "f64" => "float".to_string(),
                    "bool" => "bool".to_string(),
                    "Vec" => "array".to_string(),
                    "HashMap" => "map".to_string(),
                    other => other.to_string(),
                }
            } else {
                "any".to_string()
            }
        }
        _ => "any".to_string(),
    }
}

/// `#[derive(Xcl)]` — Auto-registers Rust traits, structs, and enums in XCL registries.
///
/// - On a `trait`: extracts `fn` item names and registers in `TRAIT_REGISTRY`.
/// - On a `struct`: registers in `TYPE_REGISTRY`. If the struct has fields, it's a Record type.
///   If it's a unit struct, it's an Opaque type.
/// - On an `enum`: registers as a Sum type in `TYPE_REGISTRY`.
///
/// Use `#[xcl(domain = "...", kind = "...", summary = "...")]` for optional metadata.
///
/// # Example
///
/// ```ignore
/// #[derive(Xcl)]
/// pub trait CloudService {
///     fn bucket_create();
///     fn bucket_list();
/// }
///
/// #[derive(Xcl)]
/// pub struct CloudflareProvider;
///
/// #[derive(Xcl)]
/// #[xcl(domain = "io", kind = "Record", summary = "A file handle.")]
/// pub struct FileType {
///     handle: u64,
/// }
///
/// #[derive(Xcl)]
/// #[xcl(kind = "Sum", summary = "Success or failure.")]
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

    let (domain, kind, summary) = parse_xcl_helper_attrs(&input);

    let domain_expr = match domain {
        Some(d) => quote!(Some(#d)),
        None => quote!(None),
    };

    let summary_expr = match summary {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    match &input.data {
        syn::Data::Struct(data_struct) => {
            let statik = syn::Ident::new(
                &format!("__XCL_TYPE_{}", name_str.to_uppercase()),
                proc_macro2::Span::call_site(),
            );

            // Determine kind and fields
            let (kind_expr, fields_tokens) = match &data_struct.fields {
                syn::Fields::Named(fields) => {
                    // Record type with named fields
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
                    // Tuple struct — treat as opaque
                    (quote!(::xcl_vm::TypeKind::Opaque), quote!(&[]))
                }
                syn::Fields::Unit => {
                    // Unit struct — opaque type
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
                };

                // Prevent dead code elimination by referencing the type
                #[allow(dead_code)]
                const _: Option<#name> = None;
            };
            expanded.into()
        }
        syn::Data::Enum(data_enum) => {
            // Sum type with variants
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
                                    fields: &[#(#fs_entries),*],
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
                                    fields: &[#(#fs_entries),*],
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
                };

                // Prevent dead code elimination by referencing the type
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

/// `#[xcl_trait]` — Auto-registers a Rust trait in `TRAIT_REGISTRY` by extracting `fn` item names.
///
/// No attributes needed — the macro inspects the Rust trait body and extracts method names.
///
/// # Example
///
/// ```ignore
/// #[xcl_trait]
/// pub trait CloudService {
///     fn bucket_create();
///     fn bucket_list();
/// }
/// ```
#[proc_macro_attribute]
pub fn xcl_trait(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let trait_item = parse_macro_input!(item as syn::ItemTrait);
    let name = &trait_item.ident;
    let name_str = name.to_string();

    // Extract fn item names from the trait body
    let mut method_names: Vec<String> = Vec::new();
    for item in &trait_item.items {
        if let syn::TraitItem::Fn(tif) = item {
            method_names.push(tif.sig.ident.to_string());
        }
    }

    // Generate methods tokens
    let methods_tokens = if method_names.is_empty() {
        quote!(&[])
    } else {
        let entries = method_names.iter().map(|m_name| {
            quote! {
                ::xcl_vm::TraitMethod {
                    name: #m_name,
                    args: None,
                    ret: None,
                    summary: None,
                }
            }
        });
        quote!(&[#(#entries),*])
    };

    let statik = syn::Ident::new(
        &format!("__XCL_TRAIT_{}", name_str.to_uppercase()),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        #trait_item

        #[::linkme::distributed_slice(::xcl_vm::TRAIT_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static #statik: ::xcl_vm::TraitDescriptor = ::xcl_vm::TraitDescriptor {
            name: #name_str,
            domain: None,
            methods: #methods_tokens,
            summary: None,
        };
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn xcl_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_ident = &func.sig.ident;
    let vis = &func.vis;

    let parsed: NativeAttr = parse_macro_input!(attr);
    let method_name = parsed.method;
    let domain = parsed.domain;
    let impl_type = parsed.impl_type;
    let override_method = parsed.override_method;
    let args_attr = parsed.args;
    let ret_attr = parsed.ret;
    let summary = parsed.summary;
    let arg_descs = parsed.arg_desc;
    let named_params = parsed.named;

    let statik = syn::Ident::new(
        &format!("__XCL_FN_{}", fn_ident),
        fn_ident.span(),
    );

    let domain_expr = if let Some(ref d) = domain {
        quote!(Some(#d))
    } else {
        quote!(None)
    };

    let impl_type_expr = if let Some(ref t) = impl_type {
        quote!(Some(#t))
    } else {
        quote!(None)
    };

    let override_method_expr = if let Some(ref m) = override_method {
        quote!(Some(#m))
    } else {
        quote!(None)
    };

    let args_expr: Option<String> = args_attr;
    let ret_expr: Option<String> = ret_attr;

    let args_static = match &args_expr {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };
    let ret_static = match &ret_expr {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    // Generate summary tokens
    let summary_static = match &summary {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    // Generate arg_desc tokens
    let arg_desc_static = if arg_descs.is_empty() {
        quote!(&[])
    } else {
        let entries = arg_descs.iter().map(|(name, desc)| {
            quote!((#name, #desc))
        });
        quote!(&[#(#entries),*])
    };

    // Generate named params tokens
    let named_static = if named_params.is_empty() {
        quote!(&[])
    } else {
        let entries = named_params.iter().map(|np| {
            let n = &np.name;
            let t = &np.ty;
            let d = &np.default;
            quote! {
                ::xcl_vm::NamedParam {
                    name: #n,
                    ty: #t,
                    default: #d,
                }
            }
        });
        quote!(&[#(#entries),*])
    };

    // Check if the function uses typed Rust parameters (not &[Value] or &mut DomainCtx)
    let uses_typed_params = !func.sig.inputs.is_empty() && !is_raw_args_signature(&func);

    if uses_typed_params {
        // Generate a wrapper that converts &[Value] to typed Rust args
        let wrapper_ident = syn::Ident::new(
            &format!("__xcl_fn_wrapper_{}", fn_ident),
            fn_ident.span(),
        );

        let (arg_conversions, arg_vars) = generate_arg_conversions(&func, &args_expr);

        let expanded = quote! {
            #func

            #[allow(dead_code)]
            fn #wrapper_ident(args: &[::xcl_vm::Value]) -> ::xcl_vm::Value {
                #(#arg_conversions)*
                #fn_ident(#(#arg_vars),*)
            }

            #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
            #[allow(non_upper_case_globals)]
            #vis static #statik: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
                domain: #domain_expr,
                impl_type: #impl_type_expr,
                override_method: #override_method_expr,
                method: #method_name,
                kind: ::xcl_vm::NativeKind::Pure,
                ptr: ::xcl_vm::RawPtr(#wrapper_ident as *const ()),
                args: #args_static,
                ret: #ret_static,
                named: #named_static,
                summary: #summary_static,
                arg_desc: #arg_desc_static,
            };
        };

        expanded.into()
    } else {
        // Legacy: function takes &[Value] or &mut DomainCtx directly
        let kind = detect_kind(&func);

        let expanded = quote! {
            #func

            #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
            #[allow(non_upper_case_globals)]
            #vis static #statik: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
                domain: #domain_expr,
                impl_type: #impl_type_expr,
                override_method: #override_method_expr,
                method: #method_name,
                kind: #kind,
                ptr: ::xcl_vm::RawPtr(#fn_ident as *const ()),
                args: #args_static,
                ret: #ret_static,
                named: #named_static,
                summary: #summary_static,
                arg_desc: #arg_desc_static,
            };
        };

        expanded.into()
    }
}

/// Check if the function signature is the legacy style: `fn(args: &[Value])` or `fn(ctx: &mut DomainCtx)`
fn is_raw_args_signature(func: &ItemFn) -> bool {
    if func.sig.inputs.is_empty() {
        return true; // no-arg functions are fine either way
    }
    let first = func.sig.inputs.first().unwrap();
    if let FnArg::Typed(pt) = first {
        is_ctx_type(&pt.ty) || is_value_slice_type(&pt.ty)
    } else {
        false
    }
}

fn is_value_slice_type(ty: &Type) -> bool {
    if let Type::Reference(r) = ty
        && let Type::Slice(s) = &*r.elem
        && let Type::Path(p) = &*s.elem
        && let Some(seg) = p.path.segments.last()
    {
        return seg.ident == "Value";
    }
    false
}

struct NativeAttr {
    method:         String,
    domain:         Option<String>,
    impl_type:      Option<String>,
    override_method: Option<String>,
    args:           Option<String>,
    ret:      Option<String>,
    summary:  Option<String>,
    arg_desc: Vec<(String, String)>,
    named:    Vec<NamedParamSpec>,
}

struct NamedParamSpec {
    name:    String,
    ty:      String,
    default: String,
}

impl syn::parse::Parse for NativeAttr {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        // Check if it's a simple string literal
        if input.peek(syn::Lit) {
            let lit: LitStr = input.parse()?;
            let val = lit.value();
            // Extract domain from dotted name like "io.print"
            let (method, domain) = split_dotted_name(&val);
            return Ok(NativeAttr {
                method,
                domain,
                impl_type: None,
                override_method: None,
                args:       None,
                ret:      None,
                summary:  None,
                arg_desc: Vec::new(),
                named:    Vec::new(),
            });
        }

        // Parse named attributes manually
        let mut method = None;
        let mut domain = None;
        let mut impl_type = None;
        let mut override_method = None;
        let mut args = None;
        let mut ret = None;
        let mut summary = None;
        let mut arg_desc = Vec::new();
        let mut named = Vec::new();

        loop {
            if input.is_empty() {
                break;
            }

            let key: syn::Ident = input.parse()?;
            let key_s = key.to_string();

            if key_s == "arg_desc" {
                // arg_desc("name" = "description", ...)
                let content;
                syn::parenthesized!(content in input);
                while !content.is_empty() {
                    let arg_name: LitStr = content.parse()?;
                    let _: syn::Token![=] = content.parse()?;
                    let arg_doc: LitStr = content.parse()?;
                    arg_desc.push((arg_name.value(), arg_doc.value()));
                    if content.peek(syn::Token![,]) {
                        content.parse::<syn::Token![,]>()?;
                    }
                }
            } else if key_s == "named" {
                // named(
                //   { name = "bool", ty = "bool", default = "false" },
                //   { name = "int", ty = "i64", default = "none" },
                // )
                let content;
                syn::parenthesized!(content in input);
                while !content.is_empty() {
                    let inner;
                    syn::braced!(inner in content);
                    let mut name = None;
                    let mut ty = None;
                    let mut default = None;
                    while !inner.is_empty() {
                        let k: syn::Ident = inner.parse()?;
                        let _: syn::Token![=] = inner.parse()?;
                        let v: LitStr = inner.parse()?;
                        match k.to_string().as_str() {
                            "name" => name = Some(v.value()),
                            "ty" => ty = Some(v.value()),
                            "default" => default = Some(v.value()),
                            _ => {}
                        }
                        if inner.peek(syn::Token![,]) {
                            inner.parse::<syn::Token![,]>()?;
                        }
                    }
                    if let (Some(n), Some(t), Some(d)) = (name, ty, default) {
                        named.push(NamedParamSpec { name: n, ty: t, default: d });
                    }
                    if content.peek(syn::Token![,]) {
                        content.parse::<syn::Token![,]>()?;
                    } else {
                        break;
                    }
                }
            } else {
                // key = "value" form
                let _: syn::Token![=] = input.parse()?;
                let lit: LitStr = input.parse()?;
                let val = lit.value();
                match key_s.as_str() {
                    "name" | "method" => {
                        // If domain not explicitly set, try to extract from dotted name
                        if domain.is_none() {
                            let (m, d) = split_dotted_name(&val);
                            method = Some(m);
                            domain = d;
                        } else {
                            method = Some(val);
                        }
                    }
                    "domain" => domain = Some(val),
                    "impl_type" => impl_type = Some(val),
                    "override_method" => override_method = Some(val),
                    "args" => args = Some(val),
                    "ret" => ret = Some(val),
                    "summary" => summary = Some(val),
                    _ => {}
                }
            }

            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            } else {
                break;
            }
        }

        Ok(NativeAttr {
            method:         method.unwrap_or_default(),
            domain,
            impl_type,
            override_method,
            args,
            ret,
            summary,
            arg_desc,
            named,
        })
    }
}

/// Split a dotted name like "io.print" into ("print", Some("io")).
/// If no dot, returns the whole name with None domain.
fn split_dotted_name(name: &str) -> (String, Option<String>) {
    if let Some(dot_pos) = name.find('.') {
        let domain = name[..dot_pos].to_string();
        let method = name[dot_pos + 1..].to_string();
        (method, Some(domain))
    } else {
        (name.to_string(), None)
    }
}

/// Generate argument conversion code for typed Rust parameters.
/// Returns (conversion_statements, argument_variable_names).
fn generate_arg_conversions(
    func: &ItemFn,
    _args_attr: &Option<String>,
) -> (Vec<proc_macro2::TokenStream>, Vec<proc_macro2::TokenStream>) {
    let mut conversions = Vec::new();
    let mut arg_vars = Vec::new();

    for (i, input) in func.sig.inputs.iter().enumerate() {
        if let FnArg::Typed(pt) = input {
            let arg_name = &pt.pat;
            let arg_type = &pt.ty;

            // Generate: let <name> = ::xcl_vm::extract_arg::<Type>(args, <index>);
            let index = syn::Index::from(i);
            let conversion = quote! {
                let #arg_name = ::xcl_vm::extract_arg::<#arg_type>(args, #index);
            };
            conversions.push(conversion);

            let var = quote! { #arg_name };
            arg_vars.push(var);
        }
    }

    (conversions, arg_vars)
}

#[proc_macro_attribute]
pub fn xcl_type(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: TypeAttr = parse_macro_input!(attr);
    let item = parse_macro_input!(item as syn::Item);

    let name = parsed.name;
    let domain = parsed.domain;
    let kind_str = parsed.kind;

    let kind_expr = match kind_str.as_str() {
        "Opaque" => quote!(::xcl_vm::TypeKind::Opaque),
        "Record" => quote!(::xcl_vm::TypeKind::Record),
        "Sum"    => quote!(::xcl_vm::TypeKind::Sum),
        "Alias"  => quote!(::xcl_vm::TypeKind::Alias),
        _        => quote!(::xcl_vm::TypeKind::Opaque),
    };

    let domain_expr = if let Some(ref d) = domain {
        quote!(Some(#d))
    } else {
        quote!(None)
    };

    // Extract the item's name for unique static identifier
    let item_name = if let syn::Item::Struct(s) = &item {
        s.ident.to_string()
    } else if let syn::Item::Enum(e) = &item {
        e.ident.to_string()
    } else {
        "Type".to_string()
    };

    let statik = syn::Ident::new(
        &format!("__XCL_TYPE_{}", item_name.to_uppercase()),
        proc_macro2::Span::call_site(),
    );

    // Generate summary tokens
    let summary_tokens = match &parsed.summary {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    // Generate fields tokens
    let fields_tokens = if parsed.fields.is_empty() {
        quote!(&[])
    } else {
        let fs_entries = parsed.fields.iter().map(|f| {
            let name_s = f.name.to_string();
            let ty = &f.ty;
            let opt = f.optional;
            quote! {
                ::xcl_vm::TypeField {
                    name: #name_s,
                    ty: #ty,
                    optional: #opt,
                }
            }
        });
        quote!(&[#(#fs_entries),*])
    };

    // Generate variants tokens
    let variants_tokens = if parsed.variants.is_empty() {
        quote!(&[])
    } else {
        let entries = parsed.variants.iter().map(|v| {
            let name_s = v.name.to_string();
            let fs = if v.fields.is_empty() {
                quote!(&[])
            } else {
                let fs_entries = v.fields.iter().map(|f| {
                    let name_s = f.name.to_string();
                    let ty = &f.ty;
                    let opt = f.optional;
                    quote! {
                        ::xcl_vm::TypeField {
                            name: #name_s,
                            ty: #ty,
                            optional: #opt,
                        }
                    }
                });
                quote!(&[#(#fs_entries),*])
            };
            quote! {
                ::xcl_vm::TypeVariant {
                    name: #name_s,
                    fields: #fs,
                }
            }
        });
        quote!(&[#(#entries),*])
    };

    let expanded = quote! {
        #item

        #[::linkme::distributed_slice(::xcl_vm::TYPE_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static #statik: ::xcl_vm::TypeDescriptor = ::xcl_vm::TypeDescriptor {
            name: #name,
            domain: #domain_expr,
            kind: #kind_expr,
            fields: #fields_tokens,
            variants: #variants_tokens,
            summary: #summary_tokens,
        };
    };

    expanded.into()
}

struct TypeAttr {
    name: String,
    domain: Option<String>,
    kind: String,
    summary: Option<String>,
    fields: Vec<FieldSpec>,
    variants: Vec<VariantSpec>,
}

struct FieldSpec {
    name: syn::Ident,
    ty: String,
    optional: bool,
}

struct VariantSpec {
    name: syn::Ident,
    fields: Vec<FieldSpec>,
}

impl syn::parse::Parse for TypeAttr {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut domain = None;
        let mut kind = None;
        let mut summary = None;
        let mut fields = Vec::new();
        let mut variants = Vec::new();

        loop {
            if input.is_empty() {
                break;
            }

            // Peek the next identifier
            let ident: syn::Ident = input.parse()?;
            let ident_s = ident.to_string();

            // Check what follows the identifier
            if input.peek(syn::Token![=]) {
                // name = "...", domain = "...", kind = "...", summary = "..."
                let _eq: syn::Token![=] = input.parse()?;
                let lit: LitStr = input.parse()?;
                match ident_s.as_str() {
                    "name" => name = Some(lit.value()),
                    "domain" => domain = Some(lit.value()),
                    "kind" => kind = Some(lit.value()),
                    "summary" => summary = Some(lit.value()),
                    _ => {}
                }
            } else if input.peek(syn::token::Paren) {
                // variants( ... ) or fields( ... ) — parenthesized form
                let content;
                syn::parenthesized!(content in input);
                if ident_s == "fields" {
                    fields = parse_fields_stream(&content)?;
                } else if ident_s == "variants" {
                    variants = parse_variants_stream(&content)?;
                }
            } else if input.peek(syn::token::Brace) {
                // variants { ... } or fields { ... } — braced form
                let content;
                syn::braced!(content in input);
                if ident_s == "fields" {
                    fields = parse_fields_stream(&content)?;
                } else if ident_s == "variants" {
                    variants = parse_variants_stream(&content)?;
                }
            } else {
                // Unknown attribute, ignore
            }

            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            } else {
                break;
            }
        }

        Ok(TypeAttr {
            name: name.unwrap_or_default(),
            domain,
            kind: kind.unwrap_or_else(|| "Opaque".to_string()),
            summary,
            fields,
            variants,
        })
    }
}

fn parse_fields_stream(input: syn::parse::ParseStream<'_>) -> syn::Result<Vec<FieldSpec>> {
    let mut fields = Vec::new();
    while !input.is_empty() {
        let name: syn::Ident = input.parse()?;
        let _colon: syn::Token![:] = input.parse()?;
        let lit: LitStr = input.parse()?;
        let raw_ty = lit.value();
        let (ty, optional) = if let Some(stripped) = raw_ty.strip_suffix('?') {
            (stripped.to_string(), true)
        } else {
            (raw_ty, false)
        };
        fields.push(FieldSpec { name, ty, optional });
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
    }
    Ok(fields)
}

fn parse_variants_stream(input: syn::parse::ParseStream<'_>) -> syn::Result<Vec<VariantSpec>> {
    let mut variants = Vec::new();
    while !input.is_empty() {
        let name: syn::Ident = input.parse()?;
        let fields = if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            parse_fields_stream(&content)?
        } else {
            Vec::new()
        };
        variants.push(VariantSpec { name, fields });
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        } else {
            break;
        }
    }
    Ok(variants)
}

fn detect_kind(func: &ItemFn) -> proc_macro2::TokenStream {
    let Some(FnArg::Typed(pt)) = func.sig.inputs.first() else {
        return quote!(::xcl_vm::NativeKind::Pure);
    };

    if is_ctx_type(&pt.ty) {
        quote!(::xcl_vm::NativeKind::Ctx)
    } else {
        quote!(::xcl_vm::NativeKind::Pure)
    }
}

fn is_ctx_type(ty: &Type) -> bool {
    if let Type::Reference(r) = ty
        && let Type::Path(p) = &*r.elem
        && let Some(seg) = p.path.segments.last()
    {
        return seg.ident == "DomainCtx";
    }
    false
}

/// `#[xcl_impl]` — Auto-registers a Rust impl block in `IMPL_REGISTRY` by extracting `fn` item names.
///
/// No attributes needed — the macro inspects the Rust impl block and extracts method names,
/// trait name, and type name automatically.
///
/// # Example
///
/// ```ignore
/// #[xcl_impl]
/// impl CloudService for CloudflareProvider {
///     fn bucket_create() { }
///     fn bucket_list() { }
/// }
/// ```
#[proc_macro_attribute]
pub fn xcl_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let impl_item = parse_macro_input!(item as ItemImpl);

    // Extract type name from self type
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

    // Extract trait name from the trait path
    let trait_name = match &impl_item.trait_ {
        Some((path, _for_token)) => {
            if let Some(seg) = path.segments.last() {
                seg.ident.to_string()
            } else {
                return syn::Error::new_spanned(&impl_item.self_ty, "Cannot extract trait name from impl block")
                    .to_compile_error()
                    .into();
            }
        }
        None => {
            return syn::Error::new_spanned(&impl_item.self_ty, "#[xcl_impl] requires a trait impl block (impl Trait for Type)")
                .to_compile_error()
                .into();
        }
    };

    // Extract fn item names from the impl block body
    let mut method_names: Vec<String> = Vec::new();
    for item in &impl_item.items {
        if let syn::ImplItem::Fn(iif) = item {
            method_names.push(iif.sig.ident.to_string());
        }
    }

    // Generate methods tokens
    let methods_tokens = if method_names.is_empty() {
        quote!(&[])
    } else {
        let entries = method_names.iter().map(|m_name| {
            quote! {
                ::xcl_vm::ImplMethod {
                    name: #m_name,
                    args: None,
                    ret: None,
                    summary: None,
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
            domain: None,
            methods: #methods_tokens,
        };
    };

    expanded.into()
}