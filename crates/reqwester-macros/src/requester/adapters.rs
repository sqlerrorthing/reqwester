use super::adapter::Adapter;
use proc_macro2::Ident;
use syn::{Error, Result};

mod factory;
#[cfg(feature = "util")]
mod retryable;

/// Registry of adapters.
pub(super) fn by_name(name: &Ident) -> Result<Box<dyn Adapter>> {
    match name.to_string().as_str() {
        "factory" => Ok(Box::new(factory::Factory)),
        #[cfg(feature = "util")]
        "retryable" => Ok(Box::new(retryable::Retryable)),
        #[cfg(not(feature = "util"))]
        "retryable" => Err(Error::new_spanned(
            name,
            "`retryable` requires the `util` feature of `reqwester`",
        )),
        _ => Err(Error::new_spanned(
            name,
            "unknown argument, expected `Send`, `factory` or `retryable`",
        )),
    }
}
