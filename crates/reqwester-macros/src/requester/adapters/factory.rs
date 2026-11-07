//! `#[requester(factory)]` generates `{Requester}Factory` with `create(&self)`.
//! Any `Fn() -> Requester` and `Arc<Factory>` are factories.

use crate::requester::adapter::{Adapter, Context, Output};
use quote::{format_ident, quote};
use syn::Result;

pub(super) struct Factory;

impl Adapter for Factory {
    fn name(&self) -> &'static str {
        "factory"
    }

    fn generate(&self, ctx: &Context<'_>, out: &mut Output) -> Result<()> {
        let Context { vis, ident, .. } = ctx;
        let factory = format_ident!("{ident}Factory");
        let doc = format!("Creates [`{ident}`]s.");

        out.items.push(quote! {
            #[doc = #doc]
            #vis trait #factory {
                /// The created requester.
                type Requester: #ident;

                /// Creates a new requester.
                fn create(&self) -> Self::Requester;
            }

            /// Any `Fn() -> Requester` is a factory.
            impl<__F, __R> #factory for __F
            where
                __F: ::core::ops::Fn() -> __R,
                __R: #ident,
            {
                type Requester = __R;

                fn create(&self) -> __R {
                    self()
                }
            }

            impl<__F: #factory + ?::core::marker::Sized> #factory for ::std::sync::Arc<__F> {
                type Requester = <__F as #factory>::Requester;

                fn create(&self) -> Self::Requester {
                    <__F as #factory>::create(self)
                }
            }
        });

        Ok(())
    }
}
