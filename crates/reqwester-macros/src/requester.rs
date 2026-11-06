use std::hash::{DefaultHasher, Hash, Hasher};
use crate::casing::IdentCaseExt;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use std::ops::Deref;
use radix_fmt::{radix_3, radix_36};
use syn::punctuated::Punctuated;
use syn::{Error, FnArg, ItemTrait, Result, ReturnType, Token, TraitItem, TraitItemFn, TraitItemType, Type, parse_quote, Signature, GenericParam, WherePredicate};
use crate::utils::{get_container_inner_type, root};

pub(crate) fn generate(mut input: ItemTrait) -> Result<TokenStream> {
    let root = root!(reqwester_core);

    input
        .items
        .iter()
        .find(|i| matches!(i, TraitItem::Type(TraitItemType { ident, .. }) if ident == "Error"))
        .ok_or_else(|| Error::new(Span::call_site(), "An assoc type `Error` is required"))?;

    let mut items = Vec::with_capacity(input.items.len());

    for item in input.items {
        match item {
            TraitItem::Fn(mut fun) => {
                items.push(TraitItem::Type(expand_fn(&root, &mut fun)?));
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

fn expand_fn(root: &TokenStream, fun: &mut TraitItemFn) -> Result<TraitItemType> {
    if fun.sig.asyncness.is_some() {
        return Err(Error::new_spanned(
            fun.sig.asyncness,
            "Requester fn's cannot be async",
        ));
    }

    let payload = match &fun.sig.output {
        ReturnType::Default => fun.sig.ident.to_upper_camel_case().into(),
        ReturnType::Type(_, ret) if let Type::Path(path) = ret.deref() => path.path.clone(),
        _ => {
            return Err(Error::new_spanned(
                &fun.sig.output,
                "Unsupported return type",
            ));
        }
    };

    let request = fun.sig.ident.to_upper_camel_case();

    fun.sig.output = parse_quote! { -> Self::#request };

    expand_fn_args(&mut fun.sig)?;

    Ok(parse_quote! {
        type #request: #root::Request<Payload = #payload, Error: Into<Self::Error>>;
    })
}

fn generate_name(mut idx: usize) -> Ident {
    let mut name = String::new();
    loop {
        let rem = idx % 26;
        name.push((b'A' + rem as u8) as char);
        if idx < 26 {
            break;
        }
        idx = (idx / 26) - 1;
    }
    let reversed: String = name.chars().rev().collect();
    format_ident!("{}", reversed)
}

fn get_next_generic_ident(mut idx: usize, existing: &[Ident]) -> (Ident, usize) {
    loop {
        let ident = generate_name(idx);
        idx += 1;
        if !existing.contains(&ident) {
            return (ident, idx);
        }
    }
}

fn is_already_generic(ty: &Type, existing_generics: &[Ident]) -> bool {
    let mut current_ty = ty;
    if let Type::Reference(type_ref) = current_ty {
        current_ty = &type_ref.elem;
    }

    match current_ty {
        Type::ImplTrait(_) => true,
        Type::Path(type_path) if type_path.path.segments.len() == 1 => {
            let ident = &type_path.path.segments[0].ident;
            existing_generics.contains(ident)
        }
        _ => false,
    }
}

fn expand_fn_args(sig: &mut Signature) -> Result<()> {
    let mut is_self_found = false;

    let mut new_generics: Vec<GenericParam> = Vec::new();
    let mut where_predicates: Vec<WherePredicate> = Vec::new();

    let mut generic_idx = 0;

    let existing_generics: Vec<Ident> = sig
        .generics
        .type_params()
        .map(|tp| tp.ident.clone())
        .collect();

    for arg in sig.inputs.iter_mut() {
        if let FnArg::Typed(pat) = arg {
            if is_already_generic(&pat.ty, &existing_generics) {
                continue;
            }

            let (gen_ident, next_idx) = get_next_generic_ident(generic_idx, &existing_generics);
            generic_idx = next_idx;

            if let Some(inner) = get_container_inner_type("Vec", &pat.ty.clone()) {
                pat.ty = parse_quote! { #gen_ident };

                new_generics.push(parse_quote! { #gen_ident });

                where_predicates.push(parse_quote! {
                    #gen_ident: ::core::iter::IntoIterator<Item = #inner>
                });
            } else {
                let ty = pat.ty.clone();
                pat.ty = parse_quote! { #gen_ident };

                new_generics.push(parse_quote! { #gen_ident });

                where_predicates.push(parse_quote! {
                    #gen_ident: ::core::convert::Into<#ty>
                });
            }
        } else {
            is_self_found = true;
        }
    }

    if is_self_found {
        sig.generics.params.extend(new_generics);

        if !where_predicates.is_empty() {
            let where_clause = sig.generics.make_where_clause();
            where_clause.predicates.extend(where_predicates);
        }

        Ok(())
    } else {
        Err(Error::new_spanned(
            &sig.inputs,
            "All requester methods must contain self receiver",
        ))
    }
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
            type #ty = $ty![#ty];

            #(#attrs)*
            #sig {
                let this = self;
                $body!(#ident this (#args))
            }
        };
    }
}
