use proc_macro::TokenStream;

/// Placeholder — full implementation in Phase 3.
#[proc_macro_attribute]
pub fn overridable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
