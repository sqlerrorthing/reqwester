//! `#[requester(Send, retryable)]` generates `Retryable{Requester}` (and a factory with `factory`)
//! plus `.retryable(policy)` in the `Ext` traits. Requests must be `Clone`, one lifetime per method max.

use crate::requester::adapter::{Adapter, Context, Method, Output};
use crate::requester::attr::Attr;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Error, Lifetime, Result};

pub(super) struct Retryable;

impl Adapter for Retryable {
    fn name(&self) -> &'static str {
        "retryable"
    }

    fn validate(&self, attr: &Attr, _: &[Box<dyn Adapter>]) -> Result<()> {
        if attr.send {
            Ok(())
        } else {
            Err(Error::new(
                Span::call_site(),
                "`retryable` requires `Send`: #[requester(Send, retryable)]",
            ))
        }
    }

    fn request_bounds(&self, lifetimes: &[Lifetime]) -> Vec<TokenStream> {
        let outlives = match lifetimes {
            [] => quote!('static),
            lifetimes => quote!(#(#lifetimes)+*),
        };

        vec![quote!(::core::marker::Sync), outlives]
    }

    fn generate(&self, ctx: &Context<'_>, out: &mut Output) -> Result<()> {
        if let Some(method) = ctx.methods.iter().find(|m| m.lifetimes.len() > 1) {
            return Err(Error::new_spanned(
                &method.sig.generics,
                "`retryable` supports at most one lifetime per method",
            ));
        }

        let Context {
            vis, ident, util, ..
        } = ctx;
        let retryable = format_ident!("Retryable{ident}");
        let retryable_factory = format_ident!("Retryable{ident}Factory");

        let retry = quote!(#util::retryable);
        let bounds = |r: &TokenStream| bounds(ctx, r);
        let requester_bounds = bounds(&quote!(__R));

        let types_and_methods = ctx.methods.iter().map(|m| method(ctx, m, &retry));

        let doc = format!(
            "A [`{ident}`] whose failed requests are re-sent according to a \
             [`RetryPolicy`]({retry}::RetryPolicy)."
        );

        out.items.push(quote! {
            #[doc = #doc]
            #vis struct #retryable<__R, __P, __S = #retry::DefaultSleeper> {
                inner: __R,
                policy: ::std::sync::Arc<__P>,
                sleeper: __S,
            }

            impl<__R, __P> #retryable<__R, __P> {
                pub fn new(inner: __R, policy: __P) -> Self {
                    Self::with_sleeper(inner, policy, ::core::default::Default::default())
                }
            }

            impl<__R, __P, __S> #retryable<__R, __P, __S> {
                pub fn with_sleeper(inner: __R, policy: __P, sleeper: __S) -> Self {
                    Self::shared(inner, ::std::sync::Arc::new(policy), sleeper)
                }

                /// With a policy shared with other requesters.
                pub fn shared(inner: __R, policy: ::std::sync::Arc<__P>, sleeper: __S) -> Self {
                    Self { inner, policy, sleeper }
                }

                pub fn inner(&self) -> &__R {
                    &self.inner
                }

                pub fn into_inner(self) -> __R {
                    self.inner
                }
            }

            impl<__R: ::core::clone::Clone, __P, __S: ::core::clone::Clone> ::core::clone::Clone
                for #retryable<__R, __P, __S>
            {
                fn clone(&self) -> Self {
                    Self {
                        inner: ::core::clone::Clone::clone(&self.inner),
                        policy: ::std::sync::Arc::clone(&self.policy),
                        sleeper: ::core::clone::Clone::clone(&self.sleeper),
                    }
                }
            }

            impl<__R, __P, __S> #ident for #retryable<__R, __P, __S>
            where
                #requester_bounds
            {
                type Error = <__R as #ident>::Error;

                #(#types_and_methods)*
            }
        });

        out.requester_ext.push(quote! {
            /// Retries the requests of this requester according to `policy`.
            fn retryable<__P>(self, policy: __P) -> #retryable<Self, __P> {
                #retryable::new(self, policy)
            }
        });

        if ctx.has("factory") {
            let factory = format_ident!("{ident}Factory");
            let factory_bounds = bounds(&quote!(<__F as #factory>::Requester));

            out.items.push(quote! {
                /// A factory of retrying requesters.
                #vis struct #retryable_factory<__F, __P, __S = #retry::DefaultSleeper> {
                    inner: __F,
                    policy: ::std::sync::Arc<__P>,
                    sleeper: __S,
                }

                impl<__F, __P> #retryable_factory<__F, __P> {
                    pub fn new(inner: __F, policy: __P) -> Self {
                        Self::with_sleeper(inner, policy, ::core::default::Default::default())
                    }
                }

                impl<__F, __P, __S> #retryable_factory<__F, __P, __S> {
                    pub fn with_sleeper(inner: __F, policy: __P, sleeper: __S) -> Self {
                        Self { inner, policy: ::std::sync::Arc::new(policy), sleeper }
                    }

                    pub fn inner(&self) -> &__F {
                        &self.inner
                    }

                    pub fn into_inner(self) -> __F {
                        self.inner
                    }
                }

                impl<__F, __P, __S> #factory for #retryable_factory<__F, __P, __S>
                where
                    __F: #factory,
                    #factory_bounds
                {
                    type Error = <__F as #factory>::Error;
                    type Requester = #retryable<<__F as #factory>::Requester, __P, __S>;

                    fn create(&self) -> ::core::result::Result<Self::Requester, Self::Error> {
                        ::core::result::Result::Ok(#retryable::shared(
                            #factory::create(&self.inner)?,
                            ::std::sync::Arc::clone(&self.policy),
                            ::core::clone::Clone::clone(&self.sleeper),
                        ))
                    }
                }
            });

            out.factory_ext.push(quote! {
                /// Makes the created requesters retry their requests according to `policy`.
                fn retryable<__P>(self, policy: __P) -> #retryable_factory<Self, __P> {
                    #retryable_factory::new(self, policy)
                }
            });
        }

        Ok(())
    }
}

/// Bounds of `Retryable{Requester}<R, P, S>`, `r` being `R`.
fn bounds(ctx: &Context<'_>, r: &TokenStream) -> TokenStream {
    let Context { ident, util, .. } = ctx;
    let retry = quote!(#util::retryable);

    let clone = ctx.methods.iter().map(|m| {
        let request = &m.request;
        let lifetimes = &m.lifetimes;
        let generics = (!lifetimes.is_empty()).then(|| quote!(<#(#lifetimes),*>));
        let binder = (!lifetimes.is_empty()).then(|| quote!(for<#(#lifetimes),*>));

        quote!(#binder <#r as #ident>::#request #generics: ::core::clone::Clone)
    });

    quote! {
        #r: #ident,
        #(#clone,)*
        <#r as #ident>::Error: ::core::marker::Send + 'static,
        __P: #retry::RetryPolicy<<#r as #ident>::Error> + ::core::marker::Send + ::core::marker::Sync + 'static,
        __S: #retry::Sleeper,
    }
}

/// Associated request type and the forwarding method.
fn method(ctx: &Context<'_>, m: &Method<'_>, retry: &TokenStream) -> TokenStream {
    let Context { ident, .. } = ctx;
    let Method {
        sig,
        attrs,
        request,
        lifetimes,
        args,
    } = m;

    let generics = (!lifetimes.is_empty()).then(|| quote!(<#(#lifetimes),*>));
    let outlives = match lifetimes.as_slice() {
        [] => quote!('static),
        [lifetime] => quote!(#lifetime),
        _ => unreachable!("validated"),
    };
    let name = &sig.ident;

    quote! {
        type #request #generics = #retry::RetryableRequest<
            #outlives,
            <__R as #ident>::#request #generics,
            __P,
            __S,
        >;

        #(#attrs)*
        #sig {
            #retry::RetryableRequest::with_sleeper(
                #ident::#name(&self.inner, #(#args),*),
                ::std::sync::Arc::clone(&self.policy),
                ::core::clone::Clone::clone(&self.sleeper),
            )
        }
    }
}
