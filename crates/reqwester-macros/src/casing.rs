use pastey::paste;
use proc_macro2::Ident;

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
