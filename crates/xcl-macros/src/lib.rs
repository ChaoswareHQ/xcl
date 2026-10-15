use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn xcl_native(attr: TokenStream, item: TokenStream) -> TokenStream {
    let name = parse_macro_input!(attr as LitStr);
    let func = parse_macro_input!(item as ItemFn);

    let fn_ident = &func.sig.ident;
    let vis = &func.vis;

    let kind = detect_kind(&func);

    let statik = syn::Ident::new(
        &format!("__XCL_NATIVE_{}", fn_ident),
        fn_ident.span(),
    );

    let expanded = quote! {
        #func

        #[::linkme::distributed_slice(::xcl_vm::NATIVE_REGISTRY)]
        #[::linkme(crate = ::linkme)]
        #[allow(non_upper_case_globals)]
        #vis static #statik: ::xcl_vm::NativeEntry = ::xcl_vm::NativeEntry {
            name: #name,
            kind: #kind,
            ptr: #fn_ident as *const (),
        };
    };

    expanded.into()
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
    if let Type::Reference(r) = ty {
        if let Type::Path(p) = &*r.elem {
            if let Some(seg) = p.path.segments.last() {
                return seg.ident == "DomainCtx";
            }
        }
    }
    false
}