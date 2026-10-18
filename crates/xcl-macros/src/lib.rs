//! XCL Macros — Derive and impl attribute macros for registering types and methods with the VM.
//!
//! # Supported macros
//!
//! - `#[derive(Xcl)]` — Register a struct or enum in `TYPE_REGISTRY`.
//! - `#[xcl_impl]` — Register an impl block in `IMPL_REGISTRY` (trait impl) or `NATIVE_REGISTRY` (inherent impl).
//! - `xcl_universal_methods!()` — Register universal methods (`type`, `super`) available on all values.

use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, ItemImpl, LitStr, Type, parse_macro_input};

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

/// Infer the domain from the source file path using proc_macro2's span-locations.
/// For example, `src/io/mod.rs` → `"io"`, `src/proc.rs` → `"proc"`.
fn infer_domain_from_span(span: proc_macro2::Span) -> Option<String> {
    let path = span.local_file()?;
    let path_str = path.to_string_lossy().to_string();

    // Normalize path separators
    let normalized = path_str.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();

    // Find the module name from the file path
    // e.g., ".../src/io/mod.rs" -> "io"
    // e.g., ".../src/proc.rs" -> "proc"
    for i in (0..parts.len()).rev() {
        let part = parts[i];
        if part == "mod.rs" {
            // Look at the parent directory
            if i > 0 {
                let parent = parts[i - 1];
                if parent != "src" && !parent.is_empty() {
                    return Some(parent.to_string());
                }
            }
        } else if part.ends_with(".rs") && !part.is_empty() {
            // e.g., "proc.rs" -> "proc"
            let name = part.trim_end_matches(".rs");
            if !name.is_empty() && name != "lib" {
                return Some(name.to_string());
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

    // Auto-detect domain from source file path if not provided
    if domain.is_none() {
        domain = infer_domain_from_span(input.ident.span());
    }

    // Auto-detect summary from /// doc comments if not provided
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
/// The derive macro auto-detects:
/// - **domain** from the source file path (e.g., `src/io/mod.rs` → `"io"`).
/// - **kind** from the type shape (unit struct → Opaque, named fields → Record, enum → Sum).
/// - **summary** from the first `///` doc comment line.
///
/// Use `#[xcl(domain = "...", kind = "...", summary = "...")]` to override any auto-detected value.
/// # Example
///
/// ```ignore
/// // In src/io/mod.rs — domain auto-detected as "io"
/// /// A handle to an open file.
/// #[derive(Xcl)]
/// pub struct FileType;
/// /// A stream handle for standard I/O.
/// #[derive(Xcl)]
/// #[xcl(kind = "Sum")]
/// pub struct StreamType;
///
/// // In src/proc.rs — domain auto-detected as "proc"
/// /// A handle to a spawned child process.
/// #[derive(Xcl)]
/// pub struct ProcessType;
///
/// /// Success or failure result.
/// #[derive(Xcl)]
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
/// # Example — Trait impl/// ```ignore
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

/// `#[xcl_fn]` — Auto-registers a function in `NATIVE_REGISTRY`.
///
/// Zero arguments required — everything is auto-detected:
/// - **domain + method** from the function name (e.g., `io_open` → domain="io", method="open").
/// - **kind** from the signature (`&mut DomainCtx` → Ctx, `&[Value]` → Pure).
/// - **summary** from the first `///` doc comment line.
///
/// Optional override attributes are still supported:
/// - `name = "domain.method"` — override the auto-detected name.
/// - `domain = "..."` — override the auto-detected domain.
/// - `impl_type = "..."` — for trait implementation methods.
/// - `override_method = "..."` — for trait method override.
/// - `args = "..."` — manual arg signature string.
/// - `ret = "..."` — manual return type string.
/// - `summary = "..."` — override auto-detected summary.
///
/// # Example
///
/// ```ignore
/// /// Open a file with the given mode.
/// #[xcl_fn]
/// pub fn io_open(ctx: &mut DomainCtx) -> Value { ... }
///
/// /// Get the first element of an array.
/// #[xcl_fn]
/// fn arr_first(args: &[Value]) -> Value { ... }
/// ```
#[proc_macro_attribute]
pub fn xcl_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_ident = &func.sig.ident;
    let vis = &func.vis;

    // Parse optional override attributes
    let parsed: NativeAttr = parse_macro_input!(attr);

    // Auto-detect domain and method from function name
    // e.g., "io_open" -> domain="io", method="open"
    // e.g., "arr_first" -> domain="arr", method="first"
    let fn_name_str = fn_ident.to_string();
    let (auto_domain, auto_method) = fn_name_to_domain_method(&fn_name_str);

    // Use explicit values if provided, otherwise auto-detect
    let method_name = if parsed.method.is_empty() {
        auto_method
    } else {
        parsed.method.clone()
    };
    
    let domain = if !parsed.method.is_empty() {
        // Explicit name provided — extract domain from dotted name if present
        parsed.domain.clone().or_else(|| domain_from_name(&method_name).map(|s| s.to_string()))
    } else {
        // No explicit name — use auto-detected domain, but allow explicit override
        parsed.domain.clone().or(Some(auto_domain))
    };

    let impl_type = parsed.impl_type.clone();
    let override_method = parsed.override_method.clone();
    let args_attr = parsed.args.clone();
    let ret_attr = parsed.ret.clone();

    // Auto-detect summary from doc comments if not explicitly provided
    let summary = parsed.summary.clone().or_else(|| extract_doc_summary(&func.attrs));

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

    let args_static = match &args_attr {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };
    let ret_static = match &ret_attr {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    let summary_static = match &summary {
        Some(s) => quote!(Some(#s)),
        None => quote!(None),
    };

    // Generate arg_desc tokens
    let arg_desc_static = if parsed.arg_desc.is_empty() {
        quote!(&[])
    } else {
        let entries = parsed.arg_desc.iter().map(|(name, desc)| {
            quote!((#name, #desc))
        });
        quote!(&[#(#entries),*])
    };

    // Generate named params tokens
    let named_static = if parsed.named.is_empty() {
        quote!(&[])
    } else {
        let entries = parsed.named.iter().map(|np| {
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

        let (arg_conversions, arg_vars) = generate_arg_conversions(&func, &args_attr);

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

/// Convert a snake_case function name to (domain, method).
/// e.g., "io_open" -> ("io", "open")
/// e.g., "arr_first" -> ("arr", "first")
/// e.g., "proc_spawn" -> ("proc", "spawn")
/// e.g., "complex_abs" -> ("complex", "abs")
fn fn_name_to_domain_method(fn_name: &str) -> (String, String) {
    if let Some(underscore_pos) = fn_name.find('_') {
        let domain = fn_name[..underscore_pos].to_string();
        let method = fn_name[underscore_pos + 1..].to_string();
        (domain, method)
    } else {
        // No underscore — treat the whole name as the method with no domain
        (String::new(), fn_name.to_string())
    }
}

/// Extract domain from a dotted name like "io.open" -> Some("io").
fn domain_from_name(name: &str) -> Option<&str> {
    if let Some(dot_pos) = name.find('.') {
        Some(&name[..dot_pos])
    } else {
        None
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
/// # Example///
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
pub fn xcl_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse optional attributes: #[xcl_impl(auto_import)]
    let mut auto_import = false;
    if !attr.is_empty() {
        if let Ok(ident) = syn::parse2::<syn::Ident>(attr.into()) {
            if ident == "auto_import" {
                auto_import = true;
            }
        }
    }

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

                    let auto_import_val = auto_import;
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
                            auto_import: #auto_import_val,
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
                    let auto_import_val = auto_import;
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
                            auto_import: #auto_import_val,
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
            auto_import: false,
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
            auto_import: false,
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
