use syn::{GenericArgument, PathArguments, Type};

macro_rules! root {
    ($name:ident) => {{
        use proc_macro2::Span;
        use proc_macro2::Ident;
        use proc_macro_crate::{crate_name, FoundCrate};
        use quote::quote;

        let found_crate = crate_name(stringify!($name)).expect(concat!(stringify!($name), " is present in `Cargo.toml`"));

        match found_crate {
            FoundCrate::Itself => quote!(crate),
            FoundCrate::Name(name) => {
                let ident = Ident::new(&name, Span::call_site());
                quote!( #ident )
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
