use proc_macro2::Ident;
use pastey::paste;
use syn::{GenericArgument, PathArguments, Type};

macro_rules! root {
    ($name:literal) => {{
        use proc_macro2::Span;
        use proc_macro2::Ident;
        use proc_macro_crate::{crate_name, FoundCrate};
        use quote::quote;

        match crate_name($name) {
            Ok(FoundCrate::Itself) => quote!(crate),
            Ok(FoundCrate::Name(name)) => {
                let ident = Ident::new(&name, Span::call_site());
                quote!( #ident )
            }
            Err(_) => {
                quote!( reqwester )
            }
        }
    }};
}

pub(crate) use root;

pub(crate) fn get_container_inner_type<'a>(container: &str, ty: &'a Type) -> Option<&'a Type> {
    if let Type::Path(type_path) = ty
        && let Some(last_segment) = type_path.path.segments.last()
        && last_segment.ident == container
        && let PathArguments::AngleBracketed(args) = &last_segment.arguments
        && let Some(GenericArgument::Type(inner_type)) = args.args.first()
    {
        return Some(inner_type);
    }

    None
}

macro_rules! impl_ident_to_case {
    (
        $($heck_case_trait:ident)+
    ) => {
        paste! {
            mod sealed {
                pub trait Sealed {}
            }

            impl sealed::Sealed for Ident {}

            #[allow(unused)]
            pub trait IdentCaseExt: sealed::Sealed {
                $(
                    fn [<$heck_case_trait:snake>](&self) -> Self;
                )+
            }

            impl IdentCaseExt for Ident {
                $(
                    fn [<$heck_case_trait:snake>](&self) -> Self {
                        Ident::new(&heck::$heck_case_trait::[<$heck_case_trait:snake>](self.to_string().as_str()), self.span())
                    }
                )+
            }
        }
    };
}

impl_ident_to_case! {
    ToUpperCamelCase
    ToLowerCamelCase
    ToSnakeCase
    ToKebabCase
    ToShoutySnakeCase
    ToShoutyKebabCase
}
