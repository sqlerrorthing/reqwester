//! `#[requester(factory)]` generates `{Requester}Factory` with a fallible `create(&self)`.
//! Any `Fn() -> Requester`, `Arc<Factory>` and `{Requester}FactoryFn(|| Result<..>)` are factories.

use crate::requester::adapter::{Adapter, Context, Output};
use quote::{format_ident, quote};
use syn::Result;

pub(super) struct Factory;

impl Adapter for Factory {
    fn name(&self) -> &'static str {
        "factory"
    }

    fn generate(&self, ctx: &Context<'_>, out: &mut Output) -> Result<()> {
        let Context {
            vis, ident, attr, ..
        } = ctx;
        let factory = format_ident!("{ident}Factory");
        let factory_fn = format_ident!("{ident}FactoryFn");
        let doc = format!("Creates independent [`{ident}`]s.");
        let fn_doc = format!("A [`{factory}`] from a fallible closure.");

        // With `Send`, factories are shared between tasks and create requesters to spawn.
        let (supertraits, requester_bounds, closure_bounds) = if attr.send {
            (
                quote!(: ::core::marker::Send + ::core::marker::Sync),
                quote!(+ ::core::marker::Send + 'static),
                quote! {
                    __F: ::core::marker::Send + ::core::marker::Sync,
                    __R: ::core::marker::Send + 'static,
                },
            )
        } else {
            Default::default()
        };

        out.items.push(quote! {
            #[doc = #doc]
            #vis trait #factory #supertraits {
                /// Same as the error of the requester.
                type Error;

                /// The created requester.
                type Requester: #ident<Error = Self::Error> #requester_bounds;

                /// Creates a new requester, sharing no state with the previous ones.
                fn create(&self) -> ::core::result::Result<Self::Requester, Self::Error>;
            }

            /// Any `Fn() -> Requester` is a factory.
            impl<__F, __R> #factory for __F
            where
                __F: ::core::ops::Fn() -> __R,
                __R: #ident,
                #closure_bounds
            {
                type Error = <__R as #ident>::Error;
                type Requester = __R;

                fn create(&self) -> ::core::result::Result<__R, Self::Error> {
                    ::core::result::Result::Ok(self())
                }
            }

            #[doc = #fn_doc]
            #vis struct #factory_fn<__F>(pub __F);

            impl<__F, __R> #factory for #factory_fn<__F>
            where
                __F: ::core::ops::Fn() -> ::core::result::Result<__R, <__R as #ident>::Error>,
                __R: #ident,
                #closure_bounds
            {
                type Error = <__R as #ident>::Error;
                type Requester = __R;

                fn create(&self) -> ::core::result::Result<__R, Self::Error> {
                    (self.0)()
                }
            }

            impl<__F: #factory + ?::core::marker::Sized> #factory for ::std::sync::Arc<__F> {
                type Error = <__F as #factory>::Error;
                type Requester = <__F as #factory>::Requester;

                fn create(&self) -> ::core::result::Result<Self::Requester, Self::Error> {
                    <__F as #factory>::create(self)
                }
            }
        });

        Ok(())
    }
}
