use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ItemTrait, Result};

/// The trait `input` tagged for `typetag`, with the registry of its plugins and its `Axis` impl.
pub fn expand(arguments: TokenStream, input: ItemTrait) -> Result<TokenStream> {
    if !arguments.is_empty() {
        return Err(Error::new_spanned(arguments, "`axis` takes no arguments"));
    }
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "an axis cannot be a generic trait",
        ));
    }
    if let Some(attribute) = input.attrs.iter().find(|attribute| {
        attribute
            .path()
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "typetag")
    }) {
        return Err(Error::new_spanned(
            attribute,
            "an axis is tagged for `typetag` by `axis`, so it takes no `typetag` attribute of its \
             own",
        ));
    }
    let axis = &input.ident;
    // the registry is a type of the crate declaring the axis, since inventory only collects those;
    // it is named only through `Register`, so it is declared where nothing else can see it
    Ok(quote! {
        #[::typetag::serde(tag = "type", content = "value")]
        #input

        const _: () = {
            pub struct Registry(::plugin_factory::catalog::PluginEntry);

            ::plugin_factory::__private::inventory::collect!(Registry);

            impl ::plugin_factory::catalog::Axis for dyn #axis {
                fn __index() -> &'static ::plugin_factory::catalog::Index {
                    fn registered() -> ::std::vec::Vec<
                        &'static ::plugin_factory::catalog::PluginEntry,
                    > {
                        ::core::iter::IntoIterator::into_iter(
                            ::plugin_factory::__private::inventory::iter::<Registry>,
                        )
                        .map(|registered| &registered.0)
                        .collect()
                    }

                    static INDEX: ::plugin_factory::catalog::Index =
                        ::plugin_factory::catalog::Index::new(registered);
                    &INDEX
                }
            }

            impl<P> ::plugin_factory::__private::Register<P> for dyn #axis
            where
                P: ::plugin_factory::PluginType + #axis,
            {
                type Registry = Registry;

                const REGISTRY: Registry =
                    Registry(::plugin_factory::catalog::PluginEntry::new::<P>());
            }
        };
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    fn error_of(input: ItemTrait) -> String {
        expand(TokenStream::new(), input).unwrap_err().to_string()
    }

    #[test]
    fn an_argument_given_to_axis_is_an_error() {
        assert_eq!(
            expand(
                quote!(name = "metric"),
                parse_quote!(
                    trait Metric: Plugin {}
                )
            )
            .unwrap_err()
            .to_string(),
            "`axis` takes no arguments"
        );
    }

    #[test]
    fn a_generic_trait_is_an_error() {
        assert_eq!(
            error_of(parse_quote!(
                trait Metric<T>: Plugin {}
            )),
            "an axis cannot be a generic trait"
        );
        assert_eq!(
            error_of(parse_quote!(
                trait Metric<'a>: Plugin {}
            )),
            "an axis cannot be a generic trait"
        );
    }

    #[test]
    fn a_typetag_attribute_next_to_axis_is_an_error() {
        assert!(
            error_of(parse_quote! {
                #[typetag::serde(tag = "type", content = "value")]
                trait Metric: Plugin {}
            })
            .starts_with("an axis is tagged for `typetag` by `axis`")
        );
        assert!(
            error_of(parse_quote! {
                #[typetag::serde]
                trait Metric: Plugin {}
            })
            .starts_with("an axis is tagged for `typetag` by `axis`")
        );
    }

    #[test]
    fn a_trait_is_tagged_adjacently() {
        let expanded = expand(
            TokenStream::new(),
            parse_quote!(
                trait Metric: Plugin {}
            ),
        )
        .unwrap()
        .to_string();

        assert!(expanded.starts_with(
            &quote!(#[::typetag::serde(tag = "type", content = "value")]).to_string()
        ));
    }
}
