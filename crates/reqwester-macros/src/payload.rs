use crate::utils::{get_container_inner_type, root};
use derive_new::new;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    Data, DataStruct, DeriveInput, Error, Expr, Fields, FieldsNamed, Generics, MetaNameValue,
    Result, Token, Type, TypePath, Visibility, parse_quote,
};

#[derive(Clone)]
pub struct Args {
    pub returns: Type,
    pub error: Type,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let args = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;

        let mut returns = None;
        let mut error = parse_quote!(!);

        for arg in args {
            let value = arg.value.clone();
            let ty: Type = syn::parse2(quote! { #value })?;

            if arg.path.is_ident("returns") {
                returns = Some(ty);
            } else if arg.path.is_ident("error") {
                error = ty;
            } else {
                return Err(Error::new_spanned(arg, "unsupported argument"));
            }
        }

        Ok(Self {
            returns: returns.ok_or_else(|| Error::new(input.span(), "missing `returns`"))?,
            error,
        })
    }
}

pub(crate) fn generate(input: DeriveInput) -> Result<TokenStream> {
    let args = input
        .attrs
        .iter()
        .find(|a| a.path().is_ident("payload"))
        .map(|attr| attr.parse_args::<Args>())
        .ok_or(Error::new(Span::call_site(), "missing #[payload] attr"))??;

    match input.data.clone() {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => StructPayload::new(
            input.vis,
            input.generics,
            input.ident,
            args,
            named,
            root!("reqwester-core"),
        )
        .generate(),
        _ => Err(Error::new_spanned(
            input,
            "only named structs are supported",
        )),
    }
}

#[derive(new)]
struct StructPayload {
    vis: Visibility,
    generics: Generics,
    ident: Ident,
    args: Args,
    fields: FieldsNamed,
    root: TokenStream,
}

impl StructPayload {
    fn setters_trait(&self, setters_ident: &Ident) -> TokenStream {
        let Self {
            ident,
            root,
            vis,
            generics,
            ..
        } = self;

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        let methods = self.fields.named.iter().map(|field| {
            let st_ident = ident;
            let ident = field.ident.as_ref().expect("Expected named field");
            let ty = &field.ty;

            let set_doc = format!("Setter for the [`{ident}`]({st_ident}::{ident}) field.");

            if let Some(inner) = get_container_inner_type("Option", ty) {
                let set = format_ident!("set_{ident}");
                let doc = format!("Setter for the [`{ident}`]({st_ident}::{ident}) field, wrapped the value in [`Option::Some`].");
                quote! {
                    #[doc = #doc]
                    fn #ident(mut self, value: impl ::core::convert::Into<#inner>) -> Self {
                        self.payload_mut().#ident = ::core::option::Option::Some(::core::convert::Into::into(value));
                        self
                    }

                    #[doc = #set_doc]
                    fn #set(mut self, value: impl ::core::convert::Into<::core::option::Option<#inner>>) -> Self {
                        self.payload_mut().#ident = ::core::convert::Into::into(value);
                        self
                    }
                }
            } else if let Some(inner) = get_container_inner_type("Vec", ty) {
                let push = format_ident!("push_{ident}");
                quote! {
                    #[doc = #set_doc]
                    fn #ident(mut self, value: impl ::core::iter::IntoIterator<Item = #inner>) -> Self {
                        self.payload_mut().#ident = ::core::iter::IntoIterator::into_iter(value).collect();
                        self
                    }

                    fn #push(mut self, element: impl ::core::convert::Into<#inner>) -> Self {
                        self.payload_mut().#ident.push(::core::convert::Into::into(element));
                        self
                    }
                }
            } else {
                quote! {
                    #[doc = #set_doc]
                    fn #ident(mut self, value: impl ::core::convert::Into<#ty>) -> Self {
                        self.payload_mut().#ident = ::core::convert::Into::into(value);
                        self
                    }
                }
            }
        });

        let doc = format!("Setters for fields of {ident}");

        quote! {
            #[doc = #doc]
            #vis trait #setters_ident #impl_generics: #root::HasPayload<Payload = #ident #ty_generics> + ::core::marker::Sized
            #where_clause
            {
                #(#methods)*
            }
        }
    }

    fn setters_blanket_impl(&self, setters_ident: &Ident) -> TokenStream {
        let Self {
            ident,
            root,
            generics,
            ..
        } = self;

        let mut generics = generics.clone();
        let (_, ty_generics, _) = self.generics.split_for_impl();

        generics.params.push(parse_quote!(__Payload));

        let where_clause = generics.make_where_clause();
        where_clause.predicates.push(parse_quote!(
            __Payload: #root::HasPayload<Payload = #ident #ty_generics>
        ));

        let (impl_generics, _, where_clause) = generics.split_for_impl();

        quote! {
            impl #impl_generics #setters_ident #ty_generics for __Payload
            #where_clause
            {}
        }
    }

    fn setters(&self) -> TokenStream {
        let ident = &self.ident;

        let setters_ident = format_ident!("{ident}Setters");

        let setters_trait = self.setters_trait(&setters_ident);
        let setters_blanket_impl = self.setters_blanket_impl(&setters_ident);

        quote! {
            #setters_trait
            #setters_blanket_impl
        }
    }

    fn generate_new(&self) -> TokenStream {
        let Self {
            ident, generics, ..
        } = self;

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        let mut args = Vec::new();
        let mut required_inits = Vec::new();
        let mut optional_inits = Vec::new();

        for f in &self.fields.named {
            let field_ident = &f.ident;
            let ty = &f.ty;

            if get_container_inner_type("Option", ty).is_some() {
                optional_inits.push(quote! { #field_ident: None });
            } else if let Some(inner) = get_container_inner_type("Vec", ty) {
                args.push(quote! { #field_ident: impl ::core::iter::IntoIterator<Item = #inner> });
                required_inits.push(quote! { #field_ident: ::core::iter::IntoIterator::into_iter(#field_ident).collect() });
            } else {
                args.push(quote! { #field_ident: impl ::core::convert::Into<#ty> });
                required_inits
                    .push(quote! { #field_ident: ::core::convert::Into::into(#field_ident) });
            }
        }

        quote! {
            impl #impl_generics #ident #ty_generics
            #where_clause {
                pub fn new(#(#args),*) -> Self {
                    Self {
                        #(#required_inits,)*
                        #(#optional_inits,)*
                    }
                }
            }
        }
    }

    fn generate(self) -> Result<TokenStream> {
        let Self {
            ident,
            root,
            generics,
            ..
        } = &self;
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        let output = &self.args.returns;
        let error = &self.args.error;

        let setters = self.setters();
        let new = self.generate_new();

        Ok(quote! {
            #new

            impl #impl_generics #root::Payload for #ident #ty_generics #where_clause {
                type Output = #output;
                type Error = #error;
            }

            #setters
        })
    }
}
