use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ItemImpl, Result};

/// The impl `input` tagged with the plugin's id and registered for the axis of its trait.
pub fn expand(arguments: TokenStream, input: ItemImpl) -> Result<TokenStream> {
    if !arguments.is_empty() {
        return Err(Error::new_spanned(
            arguments,
            "`register` takes no arguments; the id is given by `#[plugin(id = \"...\")]`",
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "a plugin cannot be registered by a generic impl",
        ));
    }
    let Some((None, object, _)) = &input.trait_ else {
        return Err(Error::new_spanned(
            &input.self_ty,
            "`register` goes on the impl of the trait of a plugin axis",
        ));
    };
    let plugin = &input.self_ty;
    // typetag takes any constant expression as the name, which is what keeps the id in one place
    Ok(quote! {
        #[typetag::serde(name = <#plugin as ::plugin_factory::__private::PluginId>::ID)]
        #input

        ::plugin_factory::inventory::submit! {
            <<dyn #object as ::plugin_factory::catalog::PluginObject>::Kind>::register::<#plugin>()
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    fn error_of(arguments: TokenStream, input: ItemImpl) -> String {
        expand(arguments, input).unwrap_err().to_string()
    }

    #[test]
    fn an_id_given_to_register_is_an_error() {
        assert!(
            error_of(quote!(id = "points"), parse_quote!(impl Axis for Points {}))
                .starts_with("`register` takes no arguments")
        );
    }

    #[test]
    fn an_inherent_impl_cannot_be_registered() {
        assert!(
            error_of(TokenStream::new(), parse_quote!(impl Points {}))
                .starts_with("`register` goes on the impl")
        );
    }

    #[test]
    fn a_generic_impl_cannot_be_registered() {
        assert_eq!(
            error_of(
                TokenStream::new(),
                parse_quote!(
                    impl<T> Axis for Points<T> {}
                )
            ),
            "a plugin cannot be registered by a generic impl"
        );
    }
}
