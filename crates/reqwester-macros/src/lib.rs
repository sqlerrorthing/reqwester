mod payload;
mod utils;
mod requester;

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput, ItemTrait};

/// ```
/// /// Get a user!
/// #[derive(Debug, Serialize, Payload)]
/// #[payload(returns = GetUserResponse)]
/// pub struct GetUser {
///     /// The user id
///     pub user_id: u64,
///
///     /// Optional metadata ok ik!
///     /// All optionals should been last
///     pub include_metadata: Option<bool>
/// }
/// ```
#[proc_macro_derive(Payload, attributes(payload))]
pub fn payload(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    payload::generate(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// ```
/// /// Just requester
/// #[requester(Send)]
/// pub trait Requester {
///     type Error: core::error::Error + Send;
///
///     fn get_me(&self);
/// }
/// ```
///
/// will expand to
///
/// ```
/// /// Just requester
/// pub trait Requester {
///     type Error: core::error::Error + Send;
///
///     type GetMe: Request<Payload = GetMe, Error = Self::Err> + Send;
///
///     fn get_me(&self) -> Self::GetMe;
/// }
/// ```
#[proc_macro_attribute]
pub fn requester(attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemTrait);

    requester::generate(input, attr)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
