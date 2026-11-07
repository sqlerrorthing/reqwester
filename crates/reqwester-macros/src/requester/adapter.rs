//! Extension point of `#[requester]`: everything generated on top of the trait.
//! Add one in `adapters/<name>.rs` and register it in `adapters::by_name`.

use super::attr::Attr;
use crate::utils::IdentCaseExt;
use proc_macro2::{Ident, TokenStream};
use syn::{
    Error, FnArg, ItemTrait, Lifetime, Pat, Result, Signature, TraitItem, TypeParamBound, Visibility,
};

use super::method_lifetimes;

pub(super) trait Adapter {
    /// Name used in `#[requester(<name>)]`.
    fn name(&self) -> &'static str;

    /// Checks compatibility with the attribute and other adapters.
    fn validate(&self, _attr: &Attr, _adapters: &[Box<dyn Adapter>]) -> Result<()> {
        Ok(())
    }

    /// Extra bounds of a method's request type.
    fn request_bounds(&self, _lifetimes: &[Lifetime]) -> Vec<TokenStream> {
        Vec::new()
    }

    fn generate(&self, ctx: &Context<'_>, out: &mut Output) -> Result<()>;
}

/// What adapters produce.
#[derive(Default)]
pub(super) struct Output {
    /// Free items.
    pub items: Vec<TokenStream>,
    /// Methods of `{Requester}Ext`.
    pub requester_ext: Vec<TokenStream>,
    /// Methods of `{Requester}FactoryExt` (needs `factory`).
    pub factory_ext: Vec<TokenStream>,
}

/// A method of the expanded trait.
#[allow(unused)]
pub(super) struct Method<'a> {
    /// Expanded signature.
    pub sig: &'a Signature,
    pub attrs: &'a [syn::Attribute],
    /// Associated request type (`F`).
    pub request: Ident,
    pub lifetimes: Vec<Lifetime>,
    /// Argument names, without the receiver.
    pub args: Vec<Ident>,
    /// The signature as written by the user.
    pub original: &'a Signature,
}

#[allow(dead_code)]
pub(super) struct Context<'a> {
    /// Path of `reqwester`.
    pub root: &'a TokenStream,
    pub util: TokenStream,
    pub attr: &'a Attr,
    pub vis: &'a Visibility,
    pub ident: &'a Ident,
    pub methods: Vec<Method<'a>>,
    /// Bounds of `type Error` as written by the user.
    pub error_bounds: Vec<TypeParamBound>,
    enabled: Vec<&'static str>,
}

#[allow(unused)]
impl<'a> Context<'a> {
    pub fn new(
        input: &'a ItemTrait,
        original: &'a ItemTrait,
        root: &'a TokenStream,
        attr: &'a Attr,
        adapters: &[Box<dyn Adapter>],
    ) -> Result<Self> {
        if !input.generics.params.is_empty() {
            return Err(Error::new_spanned(
                &input.generics,
                "adapters do not support generic requesters",
            ));
        }

        let originals = original.items.iter().filter_map(|item| match item {
            TraitItem::Fn(fun) => Some(&fun.sig),
            _ => None,
        });

        let methods = input
            .items
            .iter()
            .filter_map(|item| match item {
                TraitItem::Fn(fun) => Some(fun),
                _ => None,
            })
            .zip(originals)
            .map(|(fun, original)| {
                let args = fun
                    .sig
                    .inputs
                    .iter()
                    .filter_map(|arg| match arg {
                        FnArg::Typed(pat) => Some(match &*pat.pat {
                            Pat::Ident(ident) => Ok(ident.ident.clone()),
                            other => Err(Error::new_spanned(
                                other,
                                "Requester arguments must be simple identifiers",
                            )),
                        }),
                        FnArg::Receiver(_) => None,
                    })
                    .collect::<Result<_>>()?;

                Ok(Method {
                    sig: &fun.sig,
                    attrs: &fun.attrs,
                    request: fun.sig.ident.to_upper_camel_case(),
                    lifetimes: method_lifetimes(&fun.sig),
                    args,
                    original,
                })
            })
            .collect::<Result<_>>()?;

        let error_bounds = original
            .items
            .iter()
            .find_map(|item| match item {
                TraitItem::Type(ty) if ty.ident == "Error" => Some(ty.bounds.iter().cloned().collect()),
                _ => None,
            })
            .unwrap_or_default();

        Ok(Self {
            root,
            util: util_root(),
            attr,
            vis: &input.vis,
            ident: &input.ident,
            methods,
            error_bounds,
            enabled: adapters.iter().map(|a| a.name()).collect(),
        })
    }

    /// Whether the adapter is requested.
    pub fn has(&self, name: &str) -> bool {
        self.enabled.contains(&name)
    }
}

/// Resolved apart from the core path: users may depend on `reqwester` only.
fn util_root() -> TokenStream {
    use proc_macro_crate::{FoundCrate, crate_name};

    match crate_name("reqwester") {
        Ok(FoundCrate::Name(name)) => {
            let name = Ident::new(&name, proc_macro2::Span::call_site());
            quote::quote!(::#name::util)
        }
        Ok(FoundCrate::Itself) | Err(_) => quote::quote!(::reqwester::util),
    }
}

/// Bounds shared by adapters that keep requests across calls: `Sync` and the lifetime
/// the request borrows (`'static` without lifetimes).
pub(super) fn sync_outlives(lifetimes: &[Lifetime]) -> Vec<TokenStream> {
    let outlives = match lifetimes {
        [] => quote::quote!('static),
        lifetimes => quote::quote!(#(#lifetimes)+*),
    };

    vec![quote::quote!(::core::marker::Sync), outlives]
}
