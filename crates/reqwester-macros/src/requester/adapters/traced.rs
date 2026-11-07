//! `#[requester(traced)]` generates `Traced{Requester}<R>` (a span and an outcome event per
//! request), `.traced()` in the `Ext` traits and, with `factory`, `Traced{Requester}Factory`.

use crate::requester::adapter::{Adapter, Context, Method, Output};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Result;

pub(super) struct Traced;

impl Adapter for Traced {
    fn name(&self) -> &'static str {
        "traced"
    }

    fn generate(&self, ctx: &Context<'_>, out: &mut Output) -> Result<()> {
        let Context {
            vis, ident, util, ..
        } = ctx;
        let traced = format_ident!("Traced{ident}");
        let traced_factory = format_ident!("Traced{ident}Factory");
        let tracing = quote!(#util::traced);

        let methods = ctx.methods.iter().map(|m| method(ctx, m, &tracing));
        let doc = format!("A [`{ident}`] whose requests are traced.");

        out.items.push(quote! {
            #[doc = #doc]
            #[derive(::core::clone::Clone)]
            #vis struct #traced<__R> {
                inner: __R,
            }

            impl<__R> #traced<__R> {
                pub fn new(inner: __R) -> Self {
                    Self { inner }
                }

                pub fn inner(&self) -> &__R {
                    &self.inner
                }

                pub fn into_inner(self) -> __R {
                    self.inner
                }
            }

            impl<__R> #ident for #traced<__R>
            where
                __R: #ident,
                <__R as #ident>::Error: ::core::fmt::Display,
            {
                type Error = <__R as #ident>::Error;

                #(#methods)*
            }
        });

        out.requester_ext.push(quote! {
            /// Traces the requests of this requester.
            fn traced(self) -> #traced<Self> {
                #traced::new(self)
            }
        });

        if ctx.has("factory") {
            let factory = format_ident!("{ident}Factory");

            out.items.push(quote! {
                /// A factory of tracing requesters.
                #[derive(::core::clone::Clone)]
                #vis struct #traced_factory<__F> {
                    inner: __F,
                }

                impl<__F> #traced_factory<__F> {
                    pub fn new(inner: __F) -> Self {
                        Self { inner }
                    }

                    pub fn inner(&self) -> &__F {
                        &self.inner
                    }

                    pub fn into_inner(self) -> __F {
                        self.inner
                    }
                }

                impl<__F> #factory for #traced_factory<__F>
                where
                    __F: #factory,
                    <__F as #factory>::Error: ::core::fmt::Display,
                {
                    type Error = <__F as #factory>::Error;
                    type Requester = #traced<<__F as #factory>::Requester>;

                    fn create(&self) -> ::core::result::Result<Self::Requester, Self::Error> {
                        ::core::result::Result::Ok(#traced::new(#factory::create(&self.inner)?))
                    }
                }
            });

            out.factory_ext.push(quote! {
                /// Makes the created requesters trace their requests.
                fn traced(self) -> #traced_factory<Self> {
                    #traced_factory::new(self)
                }
            });
        }

        Ok(())
    }
}

/// Associated request type and the forwarding method.
fn method(ctx: &Context<'_>, m: &Method<'_>, tracing: &TokenStream) -> TokenStream {
    let Context { ident, .. } = ctx;
    let Method {
        sig,
        attrs,
        request,
        lifetimes,
        args,
    } = m;

    let generics = (!lifetimes.is_empty()).then(|| quote!(<#(#lifetimes),*>));
    let name = &sig.ident;
    let label = name.to_string();

    quote! {
        type #request #generics = #tracing::TracedRequest<<__R as #ident>::#request #generics>;

        #(#attrs)*
        #sig {
            #tracing::TracedRequest::named(#ident::#name(&self.inner, #(#args),*), #label)
        }
    }
}
