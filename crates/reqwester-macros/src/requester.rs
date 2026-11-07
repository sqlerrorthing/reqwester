use crate::utils::{IdentCaseExt, get_container_inner_type, root};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use radix_fmt::radix_36;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Deref;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{custom_keyword, parse, parse_quote, Error, FnArg, Generics, ImplItem, ImplItemFn, ImplItemType, Item, ItemImpl, ItemTrait, Lifetime, Pat, Result, ReturnType, Signature, Stmt, Token, TraitItem, TraitItemFn, TraitItemType, Type};

struct Attr {
    send: bool,
}

impl Parse for Attr {
    fn parse(input: ParseStream) -> Result<Self> {
        custom_keyword!(Send);

        let send = if input.is_empty() {
            false
        } else {
            input.parse::<Send>()?;
            true
        };

        if !input.is_empty() {
            return Err(input.error("unexpected attribute argument"));
        }

        Ok(Self { send })
    }
}

pub(crate) fn generate(input: Item, attr: proc_macro::TokenStream) -> Result<TokenStream> {
    let root = root!("reqwester-core");
    let attr = parse::<Attr>(attr)?;

    match input {
        Item::Trait(item) => generate_trait(item, &root, &attr),
        Item::Impl(item) => generate_impl(item, &root),
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

    let mut items = Vec::with_capacity(input.items.len());

    for item in input.items {
        match item {
            TraitItem::Fn(mut fun) => {
                items.push(TraitItem::Type(expand_trait_fn(root, attr, &mut fun)?));
                items.push(TraitItem::Fn(fun));
            }
            x => items.push(x),
        }
    }

    input.items = items;
    let forwards = generate_forward(&input);

    Ok(quote! {
        #input
        #forwards
    })
}

/// Expands an `impl Requester for Api { .. }` block.
///
/// For every method an associated type is generated:
/// - `fn f(&self, ..) -> Ty` becomes `type F = Ty;`
/// - `fn f(&self, ..)` becomes `type F = impl Request<Payload = F, Error = Self::Error>;`
///   (requires `#![feature(impl_trait_in_assoc_type)]` in the user crate)
///
/// Arguments are rewritten exactly like in the trait (`impl Into<T>`), and converted
/// back to their declared types at the start of the body, so the body sees the plain types.
fn generate_impl(mut input: ItemImpl, root: &TokenStream) -> Result<TokenStream> {
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
                let ty = expand_impl_fn(root, &mut fun)?;
                items.push(ImplItem::Type(ty));
                items.push(ImplItem::Fn(fun));
            }
            x => items.push(x),
        }
    }

    input.items = items;

    Ok(quote!(#input))
}

/// `<'a, 'b>` of the method, to be re-applied to the generated associated type.
fn method_lifetimes(sig: &Signature) -> Vec<Lifetime> {
    sig.generics
        .lifetimes()
        .map(|l| l.lifetime.clone())
        .collect()
}

/// Payload type of a request: the explicit return type, or the camel-cased
/// method name parametrized with the method lifetimes.
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

fn expand_trait_fn(root: &TokenStream, attr: &Attr, fun: &mut TraitItemFn) -> Result<TraitItemType> {
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

    let send = attr.send.then_some(quote!(+ Send));

    Ok(parse_quote! {
        type #request #generics: #root::Request<Payload = #payload, Error = Self::Error> #send;
    })
}

fn expand_impl_fn(root: &TokenStream, fun: &mut ImplItemFn) -> Result<ImplItemType> {
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
            parse_quote! { impl #root::Request<Payload = #payload, Error = Self::Error> }
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

/// Rewrites every plain argument type `T` into `impl Into<T>`
/// (and `Vec<T>` into `impl IntoIterator<Item = T>`).
///
/// Arguments that are already `impl Trait` (or a method generic) are left untouched,
/// so no synthetic generics are introduced into the signature.
///
/// Returns statements that convert the arguments back to their original types;
/// they are only used for impl blocks, where the body needs the plain types.
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

        let (new_ty, convert): (Type, TokenStream) = match get_container_inner_type("Vec", &original) {
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

        pat.ty = Box::new(new_ty);
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
