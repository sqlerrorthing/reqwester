mod payload;
mod requester;
mod utils;

use proc_macro::TokenStream;
use syn::{DeriveInput, Item, parse_macro_input};

/// Derives [`Payload`], `new` (required fields only) and field setters.
///
/// ```ignore
/// #[derive(Payload)]
/// #[payload(returns = Option<bool>)]
/// pub struct GetUser<'a> {
///     pub user_id: u64,
///     pub note: Option<&'a str>,
/// }
/// ```
#[proc_macro_derive(Payload, attributes(payload))]
pub fn payload(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    payload::generate(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Expands a requester trait (or its impl) into request types and methods.
///
/// Arguments: `Send`, `Clone` (request guarantees), `factory`, `retryable` (adapters).
/// In an impl, a method without a return type gets `type X = impl Request<..>`.
///
/// ```ignore
/// #[requester(Send, factory)]
/// pub trait Api {
///     type Error: std::error::Error + Send;
///
///     fn get_me(&self);                      // -> Self::GetMe
///     fn get_user<'a>(&self, id: &'a str);   // -> Self::GetUser<'a>
/// }
/// ```
#[proc_macro_attribute]
pub fn requester(attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Item);

    requester::generate(input, attr)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
