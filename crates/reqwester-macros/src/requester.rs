use crate::utils::{IdentCaseExt, get_container_inner_type, root};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use radix_fmt::radix_36;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Deref;
use syn::punctuated::Punctuated;
use syn::{
    Error, FnArg, Generics, ImplItem, ImplItemFn, ImplItemType, Item, ItemImpl, ItemTrait,
    Lifetime, Pat, Result, ReturnType, Signature, Stmt, Token, TraitItem, TraitItemFn,
    TraitItemType, Type, parse, parse_quote,
};

mod adapter;
mod adapters;
mod attr;

use adapter::{Adapter, Context, Output};
use attr::Attr;

pub(crate) fn generate(input: Item, attr: proc_macro::TokenStream) -> Result<TokenStream> {
    let root = root!("reqwester-core");
    let attr = parse::<Attr>(attr)?;

    match input {
        Item::Trait(item) => generate_trait(item, &root, &attr),
        Item::Impl(item) => {
            attr.ensure_no_adapters()?;
            generate_impl(item, &root, &attr)
        }
        other => Err(Error::new_spanned(
            other,
            "#[requester] can only be applied to a trait or a trait impl",
        )),
    }
}

fn generate_trait(mut input: ItemTrait, root: &TokenStream, attr: &Attr) -> Result<TokenStream> {
    input
        .items
        .iter()
        .find(|i| matches!(i, TraitItem::Type(TraitItemType { ident, .. }) if ident == "Error"))
        .ok_or_else(|| Error::new(Span::call_site(), "An assoc type `Error` is required"))?;

    let adapters = attr.adapters()?;
    let original = input.clone();

    let mut items = Vec::with_capacity(input.items.len());

    for item in input.items {
        match item {
            TraitItem::Fn(mut fun) => {
                items.push(TraitItem::Type(expand_trait_fn(
                    root, attr, &adapters, &mut fun,
                )?));
                items.push(TraitItem::Fn(fun));
            }
            x => items.push(x),
        }
    }

    input.items = items;
    let forwards = generate_forward(&input);

    let generated = run_adapters(&input, &original, root, attr, &adapters)?;

    Ok(quote! {
        #input
        #forwards
        #generated
    })
}

/// Runs adapters over the expanded trait and builds the `Ext` traits.
fn run_adapters(
    input: &ItemTrait,
    original: &ItemTrait,
    root: &TokenStream,
    attr: &Attr,
    adapters: &[Box<dyn Adapter>],
) -> Result<TokenStream> {
    if adapters.is_empty() {
        return Ok(TokenStream::new());
    }

    let ctx = Context::new(input, original, root, attr, adapters)?;
    let mut out = Output::default();

    for adapter in adapters {
        adapter.generate(&ctx, &mut out)?;
    }

    let Context { vis, ident, .. } = &ctx;
    let items = &out.items;

    let ext = (!out.requester_ext.is_empty()).then(|| {
        let ext = format_ident!("{ident}Ext");
        let methods = &out.requester_ext;

        quote! {
            /// Adapter extension methods.
            #vis trait #ext: #ident + ::core::marker::Sized {
                #(#methods)*
            }

            impl<__R: #ident> #ext for __R {}
        }
    });

    let factory_ext = (!out.factory_ext.is_empty()).then(|| {
        let factory = format_ident!("{ident}Factory");
        let ext = format_ident!("{ident}FactoryExt");
        let methods = &out.factory_ext;

        quote! {
            /// Adapter extension methods of the factory.
            #vis trait #ext: #factory + ::core::marker::Sized {
                #(#methods)*
            }

            impl<__F: #factory> #ext for __F {}
        }
    });

    Ok(quote! {
        #(#items)*
        #ext
        #factory_ext
    })
}

/// Expands `impl Requester for Api`: generates request types, converts `Into` args back.
fn generate_impl(mut input: ItemImpl, root: &TokenStream, attr: &Attr) -> Result<TokenStream> {
    if input.trait_.is_none() {
        return Err(Error::new_spanned(
            &input,
            "#[requester] can only be applied to a trait impl",
        ));
    }

    let mut items = Vec::with_capacity(input.items.len());

    for item in input.items {
        match item {
            ImplItem::Fn(mut fun) => {
                let ty = expand_impl_fn(root, attr, &mut fun)?;
                items.push(ImplItem::Type(ty));
                items.push(ImplItem::Fn(fun));
            }
            x => items.push(x),
        }
    }

    input.items = items;

    Ok(quote!(#input))
}

/// Lifetimes of the method, re-applied to the generated associated type.
fn method_lifetimes(sig: &Signature) -> Vec<Lifetime> {
    sig.generics
        .lifetimes()
        .map(|l| l.lifetime.clone())
        .collect()
}

/// Explicit return type, or the camel-cased method name with the method lifetimes.
fn payload_type(sig: &Signature, lifetimes: &[Lifetime]) -> Result<Type> {
    match &sig.output {
        ReturnType::Default => {
            let name = sig.ident.to_upper_camel_case();
            Ok(parse_quote!(#name <#(#lifetimes),*>))
        }
        ReturnType::Type(_, ret) if let Type::Path(_) = ret.deref() => Ok((**ret).clone()),
        _ => Err(Error::new_spanned(&sig.output, "Unsupported return type")),
    }
}

fn gat_generics(lifetimes: &[Lifetime]) -> Generics {
    parse_quote!(<#(#lifetimes),*>)
}

/// Bound of every request; `Send`/`Clone` are stated here so opaque `impl Request` types satisfy them.
fn request_bound(root: &TokenStream, attr: &Attr, payload: &Type) -> TokenStream {
    let clone = attr.clone.then(|| quote!(+ ::core::clone::Clone));

    if attr.send {
        quote! {
            #root::Request<
                Payload = #payload,
                Error = Self::Error,
                Send: ::core::marker::Send,
            > + ::core::marker::Send #clone
        }
    } else {
        quote!(#root::Request<Payload = #payload, Error = Self::Error> #clone)
    }
}

fn expand_trait_fn(
    root: &TokenStream,
    attr: &Attr,
    adapters: &[Box<dyn Adapter>],
    fun: &mut TraitItemFn,
) -> Result<TraitItemType> {
    if fun.sig.asyncness.is_some() {
        return Err(Error::new_spanned(
            fun.sig.asyncness,
            "Requester fn's cannot be async",
        ));
    }

    let lifetimes = method_lifetimes(&fun.sig);
    let payload = payload_type(&fun.sig, &lifetimes)?;
    let request = fun.sig.ident.to_upper_camel_case();
    let generics = gat_generics(&lifetimes);

    fun.sig.output = parse_quote! { -> Self::#request #generics };

    expand_fn_args(&mut fun.sig)?;

    let bound = request_bound(root, attr, &payload);
    let bounds = adapters
        .iter()
        .flat_map(|adapter| adapter.request_bounds(&lifetimes))
        .map(|bound| quote!(+ #bound));

    Ok(parse_quote! {
        type #request #generics: #bound #(#bounds)*;
    })
}

fn expand_impl_fn(root: &TokenStream, attr: &Attr, fun: &mut ImplItemFn) -> Result<ImplItemType> {
    if fun.sig.asyncness.is_some() {
        return Err(Error::new_spanned(
            fun.sig.asyncness,
            "Requester fn's cannot be async",
        ));
    }

    let lifetimes = method_lifetimes(&fun.sig);
    let request = fun.sig.ident.to_upper_camel_case();
    let generics = gat_generics(&lifetimes);

    let ty: Type = match &fun.sig.output {
        ReturnType::Default => {
            let payload = payload_type(&fun.sig, &lifetimes)?;
            let bound = request_bound(root, attr, &payload);
            parse_quote! { impl #bound }
        }
        ReturnType::Type(_, ret) => (**ret).clone(),
    };

    fun.sig.output = parse_quote! { -> Self::#request #generics };

    let rebinds = expand_fn_args(&mut fun.sig)?;
    fun.block.stmts.splice(0..0, rebinds);

    Ok(parse_quote! {
        type #request #generics = #ty;
    })
}

/// Rewrites arguments into `impl Into<T>` / `impl IntoIterator`; returns statements converting them back.
fn expand_fn_args(sig: &mut Signature) -> Result<Vec<Stmt>> {
    let mut is_self_found = false;
    let mut rebinds = Vec::new();

    let generics = sig.generics.clone();

    for arg in sig.inputs.iter_mut() {
        let FnArg::Typed(pat) = arg else {
            is_self_found = true;
            continue;
        };

        if matches!(&*pat.ty, Type::ImplTrait(_)) || is_generic_param(&pat.ty, &generics) {
            continue;
        }

        let original = (*pat.ty).clone();

        let (new_ty, convert): (Type, TokenStream) =
            match get_container_inner_type("Vec", &original) {
                Some(inner) => (
                    parse_quote! { impl ::core::iter::IntoIterator<Item = #inner> },
                    quote!(::core::iter::IntoIterator::into_iter),
                ),
                None => (
                    parse_quote! { impl ::core::convert::Into<#original> },
                    quote!(::core::convert::Into::into),
                ),
            };

        // The parameter itself never needs `mut`; the rebinding keeps the user's pattern.
        let Pat::Ident(ident) = &mut *pat.pat else {
            return Err(Error::new_spanned(
                &pat.pat,
                "Requester arguments must be simple identifiers",
            ));
        };
        let binding = ident.clone();
        ident.mutability = None;
        let name = &ident.ident;

        rebinds.push(if get_container_inner_type("Vec", &original).is_some() {
            parse_quote! { let #binding: #original = #convert(#name).collect(); }
        } else {
            parse_quote! { let #binding: #original = #convert(#name); }
        });

        *pat.ty = new_ty;
    }

    if is_self_found {
        Ok(rebinds)
    } else {
        Err(Error::new_spanned(
            &sig.inputs,
            "All requester methods must contain self receiver",
        ))
    }
}

fn is_generic_param(ty: &Type, generics: &syn::Generics) -> bool {
    let ty = match ty {
        Type::Reference(r) => &*r.elem,
        ty => ty,
    };

    matches!(
        ty,
        Type::Path(p) if p.path.segments.len() == 1
            && generics.type_params().any(|tp| tp.ident == p.path.segments[0].ident)
    )
}

fn generate_forward(item: &ItemTrait) -> TokenStream {
    let ident_snake = item.ident.to_snake_case();

    let mut hasher = DefaultHasher::new();
    item.ident.span().file().hash(&mut hasher);
    item.ident.span().start().hash(&mut hasher);
    item.ident.span().end().hash(&mut hasher);
    let hash = radix_36(hasher.finish() as u32);

    let internal_exported = format_ident!("__{hash}_{ident_snake}_forward");
    let normal = format_ident!("{ident_snake}_forward");

    let methods = item.items.iter().filter_map(|item| {
        if let TraitItem::Fn(fun) = item {
            Some(forward_method(fun))
        } else {
            None
        }
    });

    let internal = format_ident!("_{internal_exported}");

    quote! {
        #[doc(hidden)]
        macro_rules! #internal {
            ($method:ident $(, $rest:ident )* $(,)? => $body:ident, $ty:ident ) => {
                #internal!(@method $method $body $ty);

                $(
                    #internal!(@method $rest $body $ty);
                )*
            };

            #(#methods)*
        }

        #[doc(hidden)]
        #[macro_export]
        macro_rules! #internal_exported {
            ($($tt:tt)*) => {
                #internal! { $($tt)* }
            };
        }

        pub use #internal_exported as #normal;
    }
}

fn forward_method(fun: &TraitItemFn) -> TokenStream {
    let ident = &fun.sig.ident;
    let ty = ident.to_upper_camel_case();
    let lifetimes = method_lifetimes(&fun.sig);
    let generics = gat_generics(&lifetimes);

    // first arg is receiver
    let args = fun
        .sig
        .inputs
        .iter()
        .skip(1)
        .collect::<Punctuated<_, Token![,]>>();

    let attrs = &fun.attrs;
    let sig = &fun.sig;

    quote! {
        (@method #ident $body:ident $ty:ident) => {
            type #ty #generics = $ty![#ty #generics];

            #(#attrs)*
            #sig {
                let this = self;
                $body!(#ident this (#args))
            }
        };
    }
}
