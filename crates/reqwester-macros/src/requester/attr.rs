use super::adapter::Adapter;
use proc_macro2::Ident;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Error, Result, Token};

/// `#[requester(Send, Clone, factory, retryable)]`: flags are request guarantees, the rest are adapters.
pub(super) struct Attr {
    pub send: bool,
    pub clone: bool,
    adapters: Vec<Ident>,
}

impl Parse for Attr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut send = false;
        let mut clone = false;
        let mut adapters = Vec::new();

        for ident in Punctuated::<Ident, Token![,]>::parse_terminated(input)? {
            if ident == "Send" {
                send = true;
            } else if ident == "Clone" {
                clone = true;
            } else if adapters.contains(&ident) {
                return Err(Error::new_spanned(ident, "duplicated argument"));
            } else {
                adapters.push(ident);
            }
        }

        Ok(Self {
            send,
            clone,
            adapters,
        })
    }
}

impl Attr {
    pub fn adapters(&self) -> Result<Vec<Box<dyn Adapter>>> {
        let adapters = self
            .adapters
            .iter()
            .map(|name| super::adapters::by_name(name))
            .collect::<Result<Vec<_>>>()?;

        for adapter in &adapters {
            adapter.validate(self, &adapters)?;
        }

        Ok(adapters)
    }

    pub fn ensure_no_adapters(&self) -> Result<()> {
        match self.adapters.first() {
            Some(ident) => Err(Error::new_spanned(
                ident,
                "adapters can only be requested on the requester trait, not on its impl",
            )),
            None => Ok(()),
        }
    }
}
