use super::adapter::Adapter;
use proc_macro2::Ident;
use syn::{Error, Result};

mod factory;
#[cfg(feature = "util")]
mod erased;
#[cfg(feature = "util")]
mod retryable;
#[cfg(feature = "util")]
mod traced;

/// Registry of adapters.
pub(super) fn by_name(name: &Ident) -> Result<Box<dyn Adapter>> {
    match name.to_string().as_str() {
        "factory" => Ok(Box::new(factory::Factory)),
        #[cfg(feature = "util")]
        "retryable" => Ok(Box::new(retryable::Retryable)),
        #[cfg(feature = "util")]
        "traced" => Ok(Box::new(traced::Traced)),
        #[cfg(feature = "util")]
        "erased" => Ok(Box::new(erased::Erased)),
        #[cfg(not(feature = "util"))]
        "retryable" | "traced" | "erased" => Err(Error::new_spanned(
            name,
            "this adapter requires the `util` feature of `reqwester`",
        )),
        _ => Err(Error::new_spanned(
            name,
            "unknown argument, expected `Send`, `Clone`, `factory`, `retryable`, `traced` or `erased`",
        )),
    }
}
