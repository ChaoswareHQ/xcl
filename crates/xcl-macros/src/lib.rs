//! XCL Macros â€” Derive and impl attribute macros for registering types and methods with the VM.
//!
//! # Supported macros
//!
//! Only 2 macros are needed:
//!
//! - `#[derive(Xcl)]` â€” Register a struct or enum in `TYPE_REGISTRY`.
//! - `#[xcl_impl]` â€” Register an impl block: each method becomes a native entry in `NATIVE_REGISTRY`.
//!
//! Universal methods (`type`, `super`) are auto-registered by the VM at startup.
//!
//! # Example
//!
//! ```ignore
//! #[derive(Xcl)]
//! pub struct Https;
//!
//! #[xcl_impl]
//! impl Https {
//!     /// Perform an HTTPS GET request.
//!     pub fn get(url: String) -> Value {
//!         // ...
//!     }
//! }
//! ```
//! This registers `https.get` in the VM â€” no manual wrappers or statics needed.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse::Parse, Attribute, Expr, ExprLit, FnArg, Ident, ItemFn, ItemImpl, ItemMod, Lit, LitStr,
    Meta, Token, Type, Visibility, VisRestricted, parse_macro_input,
};

/// Extract the first line of `///` doc comments as a summary string.
fn extract_doc_summary(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("doc")
            && let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(el) = &nv.value
            && let syn::Lit::Str(lit) = &el.lit
        {
            let line = lit.value().trim().to_string();
            if !line.is_empty() {
                return Some(line);
            }
        }
    }
    None
}

/// Infer the domain from the source file path using proc_macro2's span-locations.
/// For example, `src/io/mod.rs` â†’ `"io"`, `src/proc.rs` â†’ `"proc"`.
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
fn parse_xcl_helper_attrs(
    input: &syn::DeriveInput,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let mut domain = None;
    let mut kind = None;
    let mut summary = None;
    let mut super_type = None;

    for attr in &input.attrs {
        if attr.path().is_ident("xcl")
            && let syn::Meta::List(meta_list) = &attr.meta
        {
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
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32"
                    | "u64" | "u128" | "usize" => "int".to_string(),
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

/// `#[derive(Xcl)]` â€” Auto-registers a struct or enum in `TYPE_REGISTRY`.
///
/// Supports optional `#[xcl(...)]` helper attributes:
/// - `domain = "..."` â€” override the auto-detected domain.
/// - `kind = "Record" | "Opaque" | "Sum"` â€” override the type kind.
/// - `summary = "..."` â€” override the auto-detected summary.
/// - `super = "..."` â€” set the parent type for inheritance.
///
/// The derive macro auto-detects:
/// - **domain** from the source file path (e.g., `src/io/mod.rs` â†’ `"io"`).
/// - **kind** from the type shape (unit struct â†’ Opaque, named fields â†’ Record, enum â†’ Sum).
/// - **summary** from the first `///` doc comment line.
///
/// Use `#[xcl(domain = "...", kind = "...", summary = "...")]` to override any auto-detected value.
/// # Example
///
/// ```ignore
/// // In src/io/mod.rs â€” domain auto-detected as "io"
/// /// A handle to an open file.
/// #[derive(Xcl)]
/// pub struct FileType;
/// /// A stream handle for standard I/O.
/// #[derive(Xcl)]
/// #[xcl(kind = "Sum")]
/// pub struct StreamType;
///
/// // In src/proc.rs â€” domain auto-detected as "proc"
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

    let (attr_domain, kind, summary, super_type) = parse_xcl_helper_attrs(&input);

    // Auto-detect domain from type name if not explicitly provided
    let domain = attr_domain.unwrap_or_else(|| name_str.to_lowercase());
    let domain_expr = quote!(Some(#domain));

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
                syn::Fields::Unnamed(_) => (quote!(::xcl_vm::TypeKind::Opaque), quote!(&[])),
                syn::Fields::Unit => (quote!(::xcl_vm::TypeKind::Opaque), quote!(&[])),
            };

            let expanded = quote! {
                #[::linkme::distributed_slice(::xcl_plugins::TYPE_REGISTRY)]
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
                #[::linkme::distributed_slice(::xcl_plugins::TYPE_REGISTRY)]
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
            syn::Error::new_spanned(name, "#[derive(Xcl)] is not supported for unions yet")
                .to_compile_error()
                .into()
        }
    }
}

/// `#[xcl_trait]` â€” Registers a trait in `TRAIT_REGISTRY`.
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

        #[::linkme::distributed_slice(::xcl_plugins::TRAIT_REGISTRY)]
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

/// `#[xcl_fn]` â€” Auto-registers a function in `NATIVE_REGISTRY`.
///
/// Zero arguments required â€” everything is auto-detected:
/// - **domain + method** from the function name (e.g., `io_open` â†’ domain="io", method="open").
/// - **kind** from the signature (`&mut DomainCtx` â†’ Ctx, `&[Value]` â†’ Pure).
/// - **summary** from the first `///` doc comment line.
///
/// Optional override attributes are still supported:
/// - `name = "domain.method"` â€” override the auto-detected name.
/// - `domain = "..."` â€” override the auto-detected domain.
/// - `impl_type = "..."` â€” for trait implementation methods.
/// - `override_method = "..."` â€” for trait method override.
/// - `args = "..."` â€” manual arg signature string.
/// - `ret = "..."` â€” manual return type string.
/// - `summary = "..."` â€” override auto-detected summary.
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
        // Explicit name provided â€” extract domain from dotted name if present
        parsed
            .domain
            .clone()
            .or_else(|| domain_from_name(&method_name).map(|s| s.to_string()))
    } else {
        // No explicit name â€” use auto-detected domain, but allow explicit override
        parsed.domain.clone().or(Some(auto_domain))
    };

    let impl_type = parsed.impl_type.clone();
    let override_method = parsed.override_method.clone();
    let args_attr = parsed.args.clone();
    let ret_attr = parsed.ret.clone();

    // Auto-detect summary from doc comments if not explicitly provided
    let summary = parsed
        .summary
        .clone()
        .or_else(|| extract_doc_summary(&func.attrs));

    let statik = syn::Ident::new(&format!("__XCL_FN_{}", fn_ident), fn_ident.span());

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
        let entries = parsed
            .arg_desc
            .iter()
            .map(|(name, desc)| quote!((#name, #desc)));
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
        let wrapper_ident =
            syn::Ident::new(&format!("__xcl_fn_wrapper_{}", fn_ident), fn_ident.span());

        let (arg_conversions, arg_vars) = generate_arg_conversions(&func, &args_attr);

        let expanded = quote! {
            #func

            #[allow(dead_code)]
            fn #wrapper_ident(args: &[::xcl_vm::Value]) -> ::xcl_vm::Value {
                #(#arg_conversions)*
                #fn_ident(#(#arg_vars),*)
            }

            #[::linkme::distributed_slice(::xcl_plugins::NATIVE_REGISTRY)]
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
                auto_import: false,
            };
        };

        expanded.into()
    } else {
        let kind = detect_kind(&func);

        let expanded = quote! {
            #func

            #[::linkme::distributed_slice(::xcl_plugins::NATIVE_REGISTRY)]
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
                auto_import: false,
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
        // No underscore â€” treat the whole name as the method with no domain
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
    method: String,
    domain: Option<String>,
    impl_type: Option<String>,
    override_method: Option<String>,
    args: Option<String>,
    ret: Option<String>,
    summary: Option<String>,
    arg_desc: Vec<(String, String)>,
    named: Vec<NamedParamSpec>,
}

struct NamedParamSpec {
    name: String,
    ty: String,
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
                args: None,
                ret: None,
                summary: None,
                arg_desc: Vec::new(),
                named: Vec::new(),
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
                        named.push(NamedParamSpec {
                            name: n,
                            ty: t,
                            default: d,
                        });
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
            method: method.unwrap_or_default(),
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
        "Sum" => quote!(::xcl_vm::TypeKind::Sum),
        "Alias" => quote!(::xcl_vm::TypeKind::Alias),
        _ => quote!(::xcl_vm::TypeKind::Opaque),
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

        #[::linkme::distributed_slice(::xcl_plugins::TYPE_REGISTRY)]
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
                // variants( ... ) or fields( ... ) â€” parenthesized form
                let content;
                syn::parenthesized!(content in input);
                if ident_s == "fields" {
                    fields = parse_fields_stream(&content)?;
                } else if ident_s == "variants" {
                    variants = parse_variants_stream(&content)?;
                }
            } else if input.peek(syn::token::Brace) {
                // variants { ... } or fields { ... } â€” braced form
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

/// `#[xcl_impl]` â€” Auto-registers each method in a Rust impl block as a native entry.
///
/// Domain is auto-detected from the type name (e.g., `Https` -> `"https"`, `Fs` -> `"fs"`).
/// Method names are used as-is â€” `get()` becomes `https.get`, `read()` becomes `fs.read`.
///
/// # Auto-import
///
/// By default, methods are NOT auto-imported. Use `#[xcl_impl(auto_import)]` to enable
/// auto-import for all methods in the impl block. Auto-imported methods can be called
/// as bare identifiers (e.g., `map [1, 2, 3] (x) -> x * 2` instead of `collections.map ...`).
///
/// Domains like `https` should NOT use auto_import because method names like `get` are too common.
///
/// # Example
///
/// ```ignore
/// #[derive(Xcl)]
/// pub struct Collections;
///
/// #[xcl_impl(auto_import)]
/// impl Collections {
///     /// Map a function over an array.
///     pub fn map(arr: Vec<Value>, fn_: Value) -> Value {
///         // ...
///     }
/// }
/// ```
/// This registers `collections.map` AND allows bare `map` to resolve to it.
#[proc_macro_attribute]
pub fn xcl_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse optional attributes: #[xcl_impl(auto_import)]
    let mut auto_import = false;
    if !attr.is_empty()
        && let Ok(ident) = syn::parse2::<syn::Ident>(attr.into())
        && ident == "auto_import"
    {
        auto_import = true;
    }

    let impl_item = parse_macro_input!(item as ItemImpl);

    // Extract the type name from the impl block's self type
    let (type_name, type_path_expr) = match impl_item.self_ty.as_ref() {
        syn::Type::Path(type_path) => {
            if let Some(seg) = type_path.path.segments.last() {
                let name = seg.ident.to_string();
                let type_path_copy = type_path.clone();
                (name, type_path_copy)
            } else {
                return syn::Error::new_spanned(
                    &impl_item.self_ty,
                    "Cannot extract type name from impl block",
                )
                .to_compile_error()
                .into();
            }
        }
        _ => {
            return syn::Error::new_spanned(
                &impl_item.self_ty,
                "Cannot extract type name from impl block",
            )
            .to_compile_error()
            .into();
        }
    };

    // Auto-detect domain from type name
    let domain = type_name.to_lowercase();

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
            &format!(
                "__XCL_IMPL_{}_{}",
                type_name.to_uppercase(),
                trait_name.to_uppercase()
            ),
            proc_macro2::Span::call_site(),
        );

        let expanded = quote! {
            #impl_item

            #[::linkme::distributed_slice(::xcl_plugins::IMPL_REGISTRY)]
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

                // Check return type
                let ret_type = match &iif.sig.output {
                    syn::ReturnType::Type(_, ty) => {
                        let ty_str = rust_type_to_xcl_type(ty);
                        quote!(Some(#ty_str))
                    }
                    syn::ReturnType::Default => quote!(None),
                };

                let statik = syn::Ident::new(
                    &format!(
                        "__XCL_NATIVE_{}_{}",
                        type_name.to_uppercase(),
                        method_name_str.to_uppercase()
                    ),
                    proc_macro2::Span::call_site(),
                );

                let domain_expr = quote!(Some(#domain));

                // Generate wrapper function that converts args from Value to Rust types,
                // calls the inherent method, and converts the result back to Value
                let wrapper_name = syn::Ident::new(
                    &format!(
                        "__xcl_wrapper_{}_{}",
                        type_name.to_lowercase(),
                        method_name_str
                    ),
                    proc_macro2::Span::call_site(),
                );

                // Build arg extraction code for each parameter
                let arg_extracts: Vec<proc_macro2::TokenStream> = iif.sig.inputs.iter().enumerate().map(|(i, arg)| {
                    if let syn::FnArg::Typed(pt) = arg {
                        let arg_type = &pt.ty;
                        let arg_idx = syn::Index::from(i);
                        let local_name = syn::Ident::new(&format!("arg_{}", i), proc_macro2::Span::call_site());

                        // Check if this is Vec<Value> â€” extract as Value and convert
                        let is_vec_value = matches!(arg_type.as_ref(), syn::Type::Path(type_path) if {
                            if let Some(seg) = type_path.path.segments.first() {
                                seg.ident == "Vec"
                            } else {
                                false
                            }
                        });

                        if is_vec_value {
                            quote! {
                                let #local_name = {
                                    let val = ::xcl_vm::extract_arg::<::xcl_vm::Value>(args, #arg_idx);
                                    val.as_array().map(|arr| arr.iter().copied().collect::<Vec<::xcl_vm::Value>>()).unwrap_or_default()
                                };
                            }
                        } else {
                            quote! {
                                let #local_name = ::xcl_vm::extract_arg::<#arg_type>(args, #arg_idx);
                            }
                        }
                    } else {
                        quote! {}
                    }
                }).collect();

                // Build the argument list for the method call
                let call_args: Vec<proc_macro2::TokenStream> = iif
                    .sig
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(i, _arg)| {
                        let local_name =
                            syn::Ident::new(&format!("arg_{}", i), proc_macro2::Span::call_site());
                        quote!(#local_name)
                    })
                    .collect();

                // Build the return conversion based on return type.
                // Uses IntoValue trait: all types (String, i64, f64, bool, Value, Vec<T>,
                // and custom structs) implement IntoValue, so the macro just calls .into_value().
                let return_conversion = match &iif.sig.output {
                    syn::ReturnType::Type(_, _ty) => {
                        quote! {
                            ::xcl_vm::IntoValue::into_value(#type_path_expr::#method_name(#(#call_args),*))
                        }
                    }
                    syn::ReturnType::Default => {
                        quote! {
                            #type_path_expr::#method_name(#(#call_args),*);
                            ::xcl_vm::Value::nil()
                        }
                    }
                };

                let auto_import_val = auto_import;
                native_entries.push(quote! {
                    #[::linkme::distributed_slice(::xcl_plugins::NATIVE_REGISTRY)]
                    #[allow(non_upper_case_globals)]
                    static #statik: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
                        domain: #domain_expr,
                        impl_type: None,
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
                        #(#arg_extracts)*
                        #return_conversion
                    }
                });
            }
        }

        let expanded = quote! {
            #impl_item

            #(#native_entries)*
        };

        expanded.into()
    }
}

/// `#[xcl_opcode(name = "OP_...", code = N)]` â€” Register an extension opcode handler.
///
/// Declares a function as an opcode handler and registers it in the VM's
/// [`OPCODE_REGISTRY`](::xcl_plugins::OPCODE_REGISTRY) distributed slice.
///
/// # Requirements
///
/// - `name` must be a valid Rust identifier starting with `OP_`.
/// - `code` must be an integer in the range `128..=255`.
/// - The function signature must be:
///   `fn(&mut ::xcl_vm::VM, usize, ::xcl_vm::Instr) -> ::xcl_vm::OpcodeResult`
///
/// # Example
///
/// ```ignore
/// #[xcl_opcode(name = "OP_XCL_TEST", code = 200)]
/// fn op_xcl_test(
///     vm: &mut ::xcl_vm::VM,
///     top: usize,
///     instr: ::xcl_vm::Instr,
/// ) -> ::xcl_vm::OpcodeResult {
///     vm.push_value(::xcl_vm::Value::from_i48(42));
///     ::xcl_vm::OpcodeResult::Next
/// }
/// ```
#[proc_macro_attribute]
pub fn xcl_opcode(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let mut name: Option<String> = None;
    let mut code: Option<u8> = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            name = Some(meta.value()?.parse::<syn::LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("code") {
            code = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse::<u8>()?);
            Ok(())
        } else {
            Err(meta.error("unsupported xcl_opcode attribute"))
        }
    });
    parse_macro_input!(attr with parser);

    let name = match name {
        Some(n) => n,
        None => {
            return syn::Error::new_spanned(
                &func,
                "#[xcl_opcode] requires `name = \"OP_...\"` attribute",
            )
            .to_compile_error()
            .into();
        }
    };

    if !name.starts_with("OP_") {
        return syn::Error::new_spanned(
            &func,
            "Opcode name must start with `OP_`",
        )
        .to_compile_error()
        .into();
    }

    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return syn::Error::new_spanned(
            &func,
            "Opcode name must be a valid Rust identifier",
        )
        .to_compile_error()
        .into();
    }

    let code = match code {
        Some(c) => c,
        None => {
            return syn::Error::new_spanned(
                &func,
                "#[xcl_opcode] requires `code = <int>` attribute (must be 128..=255)",
            )
            .to_compile_error()
            .into();
        }
    };

    if !(128..=255).contains(&code) {
        return syn::Error::new_spanned(
            &func,
            "Opcode code must be in range 128..=255",
        )
        .to_compile_error()
        .into();
    }
    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();

    let const_ident = syn::Ident::new(&name, proc_macro2::Span::call_site());

    let static_ident = syn::Ident::new(
        &format!("__XCL_OPCODE_{}", func_name_str.to_uppercase()),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        #func

        pub const #const_ident: u8 = #code;

        #[::linkme::distributed_slice(::xcl_plugins::OPCODE_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static #static_ident: ::xcl_vm::OpcodeEntry = ::xcl_vm::OpcodeEntry {
            code: #code,
            name: #name,
            handler: #func_name,
        };
    };

    expanded.into()
}

// Proc-macro for XCL core opcode generation.
//
// Exports `#[xcl_opcodes(scope = core)]` on a module and `#[xcl_op(...)]` on functions
// inside that module to generate opcode constants and a direct-call dispatch match.

/// Parse `#[xcl_op(code = N, arity = M, stack = "...", flags = "...")]` attributes.
struct OpMeta {
    code: u8,
    name: Option<String>,
}

impl Parse for OpMeta {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut code: Option<u8> = None;
        let mut name: Option<String> = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: Expr = input.parse()?;

            match key.to_string().as_str() {
                "code" => {
                    if let Expr::Lit(ExprLit { lit: Lit::Int(lit_int), .. }) = value {
                        code = Some(lit_int.base10_parse()?);
                    } else {
                        return Err(syn::Error::new_spanned(value, "expected integer literal"));
                    }
                }
                "name" => {
                    if let Expr::Lit(ExprLit { lit: Lit::Str(lit_str), .. }) = value {
                        name = Some(lit_str.value());
                    } else {
                        return Err(syn::Error::new_spanned(value, "expected string literal"));
                    }
                }
                _ => {
                    // Ignore other keys like arity, stack, flags â€” they are metadata only
                }
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let code = code.ok_or_else(|| syn::Error::new(Span::call_site(), "missing `code`"))?;
        Ok(OpMeta { code, name })
    }
}

/// Extract xcl_op metadata from function attributes.
fn extract_xcl_op_attrs(attrs: &[Attribute]) -> Option<OpMeta> {
    for attr in attrs {
        if attr.path().is_ident("xcl_op")
            && let Meta::List(ml) = &attr.meta
        {
            let meta: OpMeta = syn::parse2(ml.tokens.clone()).ok()?;
            return Some(meta);
        }
    }
    None
}

/// Check if an attribute is xcl_op (to filter it out).
fn is_xcl_op_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("xcl_op")
}

/// Convert function name like `op_add` to constant name `OP_ADD`.
fn func_to_const(name: &str) -> String {
    if let Some(stripped) = name.strip_prefix("op_") {
        format!("OP_{}", stripped).to_uppercase()
    } else {
        name.to_uppercase()
    }
}

/// Parse scope from `#[xcl_opcodes(scope = core)]`.
struct OpcodesMeta {
    scope: String,
}

impl Parse for OpcodesMeta {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let _key: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let scope: Ident = input.parse()?;
        Ok(OpcodesMeta {
            scope: scope.to_string(),
        })
    }
}

#[proc_macro_attribute]
pub fn xcl_opcodes(args: TokenStream, input: TokenStream) -> TokenStream {
    let meta = parse_macro_input!(args as OpcodesMeta);
    let item_mod = parse_macro_input!(input as ItemMod);

    let scope = meta.scope;
    let mod_name = &item_mod.ident;

    // Collect functions from the module
    let funcs = if let Some(ref block) = item_mod.content {
        block.1.clone()
    } else {
        return syn::Error::new_spanned(item_mod.ident, "module must have a body")
            .to_compile_error()
            .into();
    };

    let mut consts = Vec::new();
    let mut dispatch_arms = Vec::new();
    let mut cleaned_items = Vec::new();
    let mut codes_seen = Vec::new();
    let mut names_seen = Vec::new();
    let mut name_arms = Vec::new();

    for item in funcs {
        if let syn::Item::Fn(func) = &item {
            let Some(op_meta) = extract_xcl_op_attrs(&func.attrs) else {
                cleaned_items.push(item);
                continue;
            };

            let code = op_meta.code;

            // Validate code < 128 for core
            if scope == "core" && code >= 128 {
                return syn::Error::new(
                    func.sig.ident.span(),
                    format!("core opcode code must be < 128, got {}", code),
                )
                .to_compile_error()
                .into();
            }

            // Check duplicate codes
            if codes_seen.contains(&code) {
                return syn::Error::new(
                    func.sig.ident.span(),
                    format!("duplicate opcode code {}", code),
                )
                .to_compile_error()
                .into();
            }
            codes_seen.push(code);

            // Determine constant name
            let const_name = op_meta
                .name
                .unwrap_or_else(|| func_to_const(&func.sig.ident.to_string()));

            // Check duplicate names
            if names_seen.contains(&const_name) {
                return syn::Error::new(
                    func.sig.ident.span(),
                    format!("duplicate opcode name {}", const_name),
                )
                .to_compile_error()
                .into();
            }
            names_seen.push(const_name.clone());

            let const_ident = Ident::new(&const_name, func.sig.ident.span());

            // Generate constant
            consts.push(quote! {
                pub const #const_ident: u8 = #code;
            });

            // Generate dispatch arm
            let func_ident = &func.sig.ident;
            dispatch_arms.push(quote! {
                #code => {
                    #mod_name::#func_ident(vm, top, instr)
                }
            });

            // Generate opcode name arm
            name_arms.push(quote! {
                #code => #const_name,
            });

            // Build cleaned function (remove xcl_op attrs, keep others, make pub(crate))
            let mut cleaned_func = func.clone();
            cleaned_func.attrs.retain(|a| !is_xcl_op_attr(a));
            let span = Span::call_site();
            cleaned_func.vis = Visibility::Restricted(VisRestricted {
                pub_token: syn::token::Pub(span),
                paren_token: syn::token::Paren(span),
                in_token: Some(syn::token::In(span)),
                path: Box::new(syn::Path::from(Ident::new("crate", span))),
            });
            cleaned_items.push(syn::Item::Fn(cleaned_func));
        } else {
            cleaned_items.push(item);
        }
    }

    // Build the migrated-opcode bitmask (only for core scope)
    let mask_lit = if scope == "core" {
        let mask: u128 = codes_seen.iter().map(|&c| 1u128 << c).sum();
        quote! { #mask }
    } else {
        quote! { 0 }
    };

    // Build the output: consts module + cleaned module + dispatch function + opcode_name_core
    let output = if scope == "core" {
        quote! {
            pub mod consts {
                #(#consts)*
            }

            mod #mod_name {
                #(#cleaned_items)*
            }

            pub(crate) const MIGRATED_MASK: u128 = #mask_lit;

            #[inline(always)]
            pub(crate) fn is_core_migrated(op: u8) -> bool {
                op < 128 && ((MIGRATED_MASK >> op) & 1) == 1
            }

            #[inline(always)]
            pub(crate) fn dispatch_core(
                vm: &mut crate::runtime::vm::VM,
                top: usize,
                instr: crate::Instr,
            ) -> Option<crate::runtime::opcode::OpcodeResult> {
                use crate::runtime::opcode::OpcodeResult;
                Some(match instr.op() {
                    #(#dispatch_arms)*
                    _ => return None,
                })
            }

            pub(crate) fn opcode_name_core(op: u8) -> Option<&'static str> {
                Some(match op {
                    #(#name_arms)*
                    _ => return None,
                })
            }
        }
    } else {
        quote! {
            pub mod consts {
                #(#consts)*
            }

            mod #mod_name {
                #(#cleaned_items)*
            }

            pub(crate) fn dispatch_core(
                vm: &mut crate::runtime::vm::VM,
                top: usize,
                instr: crate::Instr,
            ) -> Option<crate::runtime::opcode::OpcodeResult> {
                use crate::runtime::opcode::OpcodeResult;
                Some(match instr.op() {
                    #(#dispatch_arms)*
                    _ => return None,
                })
            }

            pub(crate) fn opcode_name_core(op: u8) -> Option<&'static str> {
                Some(match op {
                    #(#name_arms)*
                    _ => return None,
                })
            }
        }
    };

    output.into()
}
