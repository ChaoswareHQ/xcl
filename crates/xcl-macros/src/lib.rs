use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn xcl_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_ident = &func.sig.ident;
    let vis = &func.vis;

    let parsed: NativeAttr = parse_macro_input!(attr);
    let method_name = parsed.method;
    let domain = parsed.domain;
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
    method:   String,
    domain:   Option<String>,
    args:     Option<String>,
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
                args:     None,
                ret:      None,
                summary:  None,
                arg_desc: Vec::new(),
                named:    Vec::new(),
            });
        }

        // Parse named attributes manually
        let mut method = None;
        let mut domain = None;
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
            method:   method.unwrap_or_default(),
            domain,
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

/// `#[xcl_impl(type = "Dog", trait = "Animal")]` — registers an impl block
/// so the doc generator knows `Dog` implements `Animal`.
///
/// The attribute can also include method declarations:
/// ```ignore
/// #[xcl_impl(
///     type = "Dog",
///     trait = "Animal",
///     methods(
///         bark { args = "volume: i64", ret = "str", summary = "Make a barking sound." },
///         eat { args = "food: str", summary = "Eat some food." },
///     ),
/// )]
/// ```
#[proc_macro_attribute]
pub fn xcl_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: ImplAttr = parse_macro_input!(attr);
    let item = parse_macro_input!(item as syn::Item);

    let type_name = parsed.type_name;
    let trait_name = parsed.trait_name;
    let domain = parsed.domain;

    let domain_expr = if let Some(ref d) = domain {
        quote!(Some(#d))
    } else {
        quote!(None)
    };

    // Generate methods tokens
    let methods_tokens = if parsed.methods.is_empty() {
        quote!(&[])
    } else {
        let entries = parsed.methods.iter().map(|m| {
            let name_s = m.name.to_string();
            let args_static = match &m.args {
                Some(s) => quote!(Some(#s)),
                None => quote!(None),
            };
            let ret_static = match &m.ret {
                Some(s) => quote!(Some(#s)),
                None => quote!(None),
            };
            let summary_static = match &m.summary {
                Some(s) => quote!(Some(#s)),
                None => quote!(None),
            };
            quote! {
                ::xcl_vm::ImplMethod {
                    name: #name_s,
                    args: #args_static,
                    ret: #ret_static,
                    summary: #summary_static,
                }
            }
        });
        quote!(&[#(#entries),*])
    };

    // Generate a unique static name from type + trait
    let statik = syn::Ident::new(
        &format!("__XCL_IMPL_{}_{}", type_name.to_uppercase(), trait_name.to_uppercase()),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        #item

        #[::linkme::distributed_slice(::xcl_vm::IMPL_REGISTRY)]
        #[allow(non_upper_case_globals)]
        static #statik: ::xcl_vm::ImplDescriptor = ::xcl_vm::ImplDescriptor {
            type_name: #type_name,
            trait_name: #trait_name,
            domain: #domain_expr,
            methods: #methods_tokens,
        };
    };

    expanded.into()
}

struct ImplAttr {
    type_name: String,
    trait_name: String,
    domain: Option<String>,
    methods: Vec<MethodImplSpec>,
}

struct MethodImplSpec {
    name: syn::Ident,
    args: Option<String>,
    ret: Option<String>,
    summary: Option<String>,
}

impl syn::parse::Parse for ImplAttr {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut type_name = None;
        let mut trait_name = None;
        let mut domain = None;
        let mut methods = Vec::new();

        loop {
            if input.is_empty() {
                break;
            }

            let ident: syn::Ident = input.parse()?;
            let ident_s = ident.to_string();

            if input.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                let lit: LitStr = input.parse()?;
                match ident_s.as_str() {
                    "type" => type_name = Some(lit.value()),
                    "trait" => trait_name = Some(lit.value()),
                    "domain" => domain = Some(lit.value()),
                    _ => {}
                }
            } else if input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in input);
                if ident_s == "methods" {
                    methods = parse_impl_methods_stream(&content)?;
                }
            }

            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            } else {
                break;
            }
        }

        Ok(ImplAttr {
            type_name: type_name.unwrap_or_default(),
            trait_name: trait_name.unwrap_or_default(),
            domain,
            methods,
        })
    }
}

fn parse_impl_methods_stream(input: syn::parse::ParseStream<'_>) -> syn::Result<Vec<MethodImplSpec>> {
    let mut methods = Vec::new();
    while !input.is_empty() {
        let name: syn::Ident = input.parse()?;
        let mut args = None;
        let mut ret = None;
        let mut summary = None;

        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            while !content.is_empty() {
                let key: syn::Ident = content.parse()?;
                let _eq: syn::Token![=] = content.parse()?;
                let lit: LitStr = content.parse()?;
                match key.to_string().as_str() {
                    "args" => args = Some(lit.value()),
                    "ret" => ret = Some(lit.value()),
                    "summary" => summary = Some(lit.value()),
                    _ => {}
                }
                if content.peek(syn::Token![,]) {
                    content.parse::<syn::Token![,]>()?;
                }
            }
        }

        methods.push(MethodImplSpec { name, args, ret, summary });

        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        } else {
            break;
        }
    }
    Ok(methods)
}