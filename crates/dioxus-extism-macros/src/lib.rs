use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, FnArg, ItemFn, Pat, Type};

/// Wraps a Dioxus component to support plugin transforms via `OverridableComponent`.
///
/// The macro checks the override map fast path: if no plugin has registered a transform
/// for this component name, the original component renders with zero network overhead.
///
/// All parameters must be concrete serialisable types. `impl Trait` parameters are
/// rejected at compile time with a clear error.
#[proc_macro_attribute]
pub fn overridable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    // Reject impl Trait parameters — they cannot be serialised into the props JSON.
    for arg in &input.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            if let Type::ImplTrait(_) = &*pat_type.ty {
                let param_name = if let Pat::Ident(ident) = &*pat_type.pat {
                    ident.ident.to_string()
                } else {
                    "unknown".to_string()
                };
                let msg = format!(
                    "Parameter `{param_name}: impl Trait` cannot be used with \
                     #[overridable]. Use a concrete type or wrap in a serialisable struct.",
                );
                return syn::Error::new(Span::call_site(), msg)
                    .to_compile_error()
                    .into();
            }
        }
    }

    let fn_name = &input.sig.ident;
    let fn_name_str = fn_name.to_string();
    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let body = &input.block;

    // Build "key": &value entries for each non-children named parameter.
    let json_entries: Vec<_> = input
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(pat_type) = arg {
                if let Pat::Ident(ident) = &*pat_type.pat {
                    let name = &ident.ident;
                    // Skip children — it's an Element and not serialisable.
                    if name == "children" {
                        return None;
                    }
                    let name_str = name.to_string();
                    return Some(quote! { #name_str: &#name });
                }
            }
            None
        })
        .collect();

    let generated = quote! {
        #(#attrs)*
        #vis #sig {
            // Borrow params to build the props JSON; borrows end before the body block.
            let __props = ::serde_json::json!({
                #(#json_entries),*
            });
            // Evaluate the original component body as the fallback element.
            let __fallback: ::dioxus::prelude::Element = #body;
            ::dioxus::prelude::rsx! {
                ::dioxus_extism_frontend::OverridableComponent {
                    name: #fn_name_str,
                    props: __props,
                    fallback: __fallback,
                }
            }
        }
    };

    generated.into()
}
