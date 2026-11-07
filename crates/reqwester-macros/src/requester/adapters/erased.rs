//! `#[requester(Send, Clone, erased)]` generates a dyn-compatible `Dyn{Requester}` (methods are
//! prefixed with `dyn_`), implemented only by the `Erased{Requester}<R>` newtype; `dyn Dyn{Requester}`
//! and `&`/`Box`/`Arc` of it are requesters. `.erased()` in the `Ext` traits gives
//! `Erased{Requester}<Self>`; with `factory` there is also `Erased{Requester}Factory<E>`.

use crate::requester::adapter::{sync_outlives, Adapter, Context, Method, Output};
use crate::requester::{payload_type, Attr};
use crate::utils::get_container_inner_type;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_quote, Error, FnArg, Lifetime, Result, Type};

pub(super) struct Erased;

impl Adapter for Erased {
    fn name(&self) -> &'static str {
        "erased"
    }

    fn validate(&self, attr: &Attr, _: &[Box<dyn Adapter>]) -> Result<()> {
        if attr.send && attr.clone {
            Ok(())
        } else {
            Err(Error::new(
                Span::call_site(),
                "`erased` requires `Send` and `Clone`: #[requester(Send, Clone, erased)]",
            ))
        }
    }

    fn request_bounds(&self, lifetimes: &[Lifetime]) -> Vec<TokenStream> {
        sync_outlives(lifetimes)
    }

    fn generate(&self, ctx: &Context<'_>, out: &mut Output) -> Result<()> {
        for method in &ctx.methods {
            if method.lifetimes.len() > 1 {
                return Err(Error::new_spanned(
                    &method.sig.generics,
                    "`erased` supports at most one lifetime per method",
                ));
            }
        }

        let Context { vis, ident, util, error_bounds, .. } = ctx;
        let erased = format_ident!("Erased{ident}");
        let dynamic = format_ident!("Dyn{ident}");
        let erased_request = quote!(#util::erased::ErasedRequest);
        let dyn_error = quote!(<Self as #dynamic>::Error);
        let ptr_error = quote!(<__D as #dynamic>::Error);

        let dyn_methods = ctx
            .methods
            .iter()
            .map(|m| dyn_method(m, &erased_request, &dyn_error))
            .collect::<Result<Vec<_>>>()?;
        let dyn_impls = ctx
            .methods
            .iter()
            .map(|m| dyn_impl(ident, m, &erased_request, &dyn_error))
            .collect::<Vec<_>>();
        let dyn_doc = format!(
            "Object-safe mirror of [`{ident}`], implemented by [`{erased}`]. \
             `dyn {dynamic}` and `&`, `Box`, `Arc` of it are [`{ident}`]s."
        );
        let erased_doc = format!("Makes a [`{ident}`] usable as a `dyn {dynamic}`.");

        // (generics, self type, error type, bounds of the generics, receiver of the `dyn` call)
        let impls = [
            (
                quote!(<__E>),
                quote!(dyn #dynamic<Error = __E>),
                quote!(__E),
                quote!(),
                quote!(self),
            ),
            (
                quote!(<'__d, __D>),
                quote!(&'__d __D),
                ptr_error.clone(),
                quote!(__D: #dynamic + ?::core::marker::Sized,),
                quote!(&**self),
            ),
            (
                quote!(<__D>),
                quote!(::std::boxed::Box<__D>),
                ptr_error.clone(),
                quote!(__D: #dynamic + ?::core::marker::Sized,),
                quote!(&**self),
            ),
            (
                quote!(<__D>),
                quote!(::std::sync::Arc<__D>),
                ptr_error.clone(),
                quote!(__D: #dynamic + ?::core::marker::Sized,),
                quote!(&**self),
            ),
        ]
        .into_iter()
        .map(|(generics, ty, error, bounds, receiver)| {
            let forwards = ctx
                .methods
                .iter()
                .map(|m| forward(&dynamic, m, &erased_request, &error, &receiver))
                .collect::<Result<Vec<_>>>()?;

            Ok(quote! {
                impl #generics #ident for #ty
                where
                    #bounds
                    #error: 'static #(+ #error_bounds)*,
                {
                    type Error = #error;

                    #(#forwards)*
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;

        out.items.push(quote! {
            #[doc = #dyn_doc]
            #vis trait #dynamic: ::core::marker::Send + ::core::marker::Sync {
                type Error;

                #(#dyn_methods)*
            }

            #[doc = #erased_doc]
            #[derive(::core::clone::Clone, ::core::fmt::Debug)]
            #vis struct #erased<__R>(pub __R);

            impl<__R> #dynamic for #erased<__R>
            where
                __R: #ident + ::core::marker::Send + ::core::marker::Sync + 'static,
            {
                type Error = <__R as #ident>::Error;

                #(#dyn_impls)*
            }

            #(#impls)*
        });

        out.requester_ext.push(quote! {
            /// Makes this requester usable as a `dyn`.
            fn erased(self) -> #erased<Self> {
                #erased(self)
            }
        });

        if ctx.has("factory") {
            let factory = format_ident!("{ident}Factory");
            let erased_factory = format_ident!("Erased{ident}Factory");
            let factory_doc = format!("A [`{factory}`] with its concrete type erased, cheap to clone.");

            out.items.push(quote! {
                #[doc = #factory_doc]
                #vis struct #erased_factory<__E> {
                    create: ::std::sync::Arc<
                        dyn ::core::ops::Fn() -> ::core::result::Result<::std::sync::Arc<dyn #dynamic<Error = __E>>, __E>
                            + ::core::marker::Send
                            + ::core::marker::Sync,
                    >,
                }

                impl<__E> #erased_factory<__E> {
                    pub fn new<__F>(factory: __F) -> Self
                    where
                        __F: #factory<Error = __E> + 'static,
                        <__F as #factory>::Requester: ::core::marker::Sync,
                        __E: 'static,
                    {
                        Self {
                            create: ::std::sync::Arc::new(move || {
                                #factory::create(&factory).map(|requester| {
                                    ::std::sync::Arc::new(#erased(requester))
                                        as ::std::sync::Arc<dyn #dynamic<Error = __E>>
                                })
                            }),
                        }
                    }
                }

                impl<__E> ::core::clone::Clone for #erased_factory<__E> {
                    fn clone(&self) -> Self {
                        Self { create: ::std::sync::Arc::clone(&self.create) }
                    }
                }

                impl<__E> #factory for #erased_factory<__E>
                where
                    __E: 'static #(+ #error_bounds)*,
                {
                    type Error = __E;
                    type Requester = ::std::sync::Arc<dyn #dynamic<Error = __E>>;

                    fn create(&self) -> ::core::result::Result<Self::Requester, Self::Error> {
                        (self.create)()
                    }
                }
            });

            out.factory_ext.push(quote! {
                /// Erases the type of this factory.
                fn erased(self) -> #erased_factory<<Self as #factory>::Error>
                where
                    Self: 'static,
                    <Self as #factory>::Requester: ::core::marker::Sync,
                    <Self as #factory>::Error: 'static,
                {
                    #erased_factory::new(self)
                }
            });
        }

        Ok(())
    }
}

fn dyn_name(name: &syn::Ident) -> syn::Ident {
    format_ident!("dyn_{name}")
}

/// `ErasedRequest<'a, Payload<'a>, Error>` of a method.
fn erased_return(
    m: &Method<'_>,
    erased_request: &TokenStream,
    error: &TokenStream,
) -> Result<TokenStream> {
    let payload = payload_type(m.original, &m.lifetimes)?;
    let lifetime = match m.lifetimes.as_slice() {
        [] => quote!('static),
        [lifetime] => quote!(#lifetime),
        _ => unreachable!("validated"),
    };

    Ok(quote!(#erased_request<#lifetime, #payload, #error>))
}

/// The method of the dyn-compatible trait: the arguments as written by the user.
fn dyn_method(
    m: &Method<'_>,
    erased_request: &TokenStream,
    error: &TokenStream,
) -> Result<TokenStream> {
    let mut sig = m.original.clone();
    let output = erased_return(m, erased_request, error)?;
    sig.output = parse_quote!(-> #output);
    sig.ident = dyn_name(&sig.ident);

    for arg in &sig.inputs {
        if let FnArg::Typed(pat) = arg
            && matches!(&*pat.ty, Type::ImplTrait(_))
        {
            return Err(Error::new_spanned(
                &pat.ty,
                "`erased` does not support `impl Trait` arguments",
            ));
        }
    }

    let attrs = m.attrs;

    Ok(quote!(#(#attrs)* #sig;))
}

fn dyn_impl(
    ident: &syn::Ident,
    m: &Method<'_>,
    erased_request: &TokenStream,
    error: &TokenStream,
) -> TokenStream {
    let name = &m.sig.ident;
    let args = &m.args;
    let attrs = m.attrs;
    let mut sig = m.original.clone();
    let output = erased_return(m, erased_request, error).expect("checked by `dyn_method`");
    sig.output = parse_quote!(-> #output);
    sig.ident = dyn_name(&sig.ident);

    quote! {
        #(#attrs)*
        #sig {
            #erased_request::new(<__R as #ident>::#name(&self.0, #(#args),*))
        }
    }
}

/// The method of the requester over a pointer to `dyn Dyn{Requester}`: converts the arguments
/// to the declared types.
fn forward(
    dynamic: &syn::Ident,
    m: &Method<'_>,
    erased_request: &TokenStream,
    error: &TokenStream,
    receiver: &TokenStream,
) -> Result<TokenStream> {
    let name = dyn_name(&m.sig.ident);
    let request = &m.request;
    let lifetimes = &m.lifetimes;
    let generics = (!lifetimes.is_empty()).then(|| quote!(<#(#lifetimes),*>));
    let request_type = erased_return(m, erased_request, error)?;
    let attrs = m.attrs;
    let sig = m.sig;

    let originals = m.original.inputs.iter().filter_map(|arg| match arg {
        FnArg::Typed(pat) => Some(&*pat.ty),
        FnArg::Receiver(_) => None,
    });

    let conversions = m.args.iter().zip(originals).map(|(arg, ty)| {
        if get_container_inner_type("Vec", ty).is_some() {
            quote!(::core::iter::Iterator::collect(::core::iter::IntoIterator::into_iter(#arg)))
        } else {
            quote!(::core::convert::Into::into(#arg))
        }
    });

    Ok(quote! {
        type #request #generics = #request_type;

        #(#attrs)*
        #sig {
            #dynamic::#name(#receiver, #(#conversions),*)
        }
    })
}
