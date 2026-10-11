//! The derive macro of `plugin-factory`, re-exported by it; use it from there.

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};

mod derive;
mod register;

/// Implements `Plugin`, `Configurable` and `PluginType` for a struct: its id, label and
/// description from `#[plugin(..)]` on the struct, and its schema and the plugins it holds from
/// its fields.
///
/// # Attributes
///
/// On the struct:
///
/// - `id = "..."`, `label = "..."` and `description = "..."`, all required. The id is also the
///   name the plugin is persisted under, see `register`.
/// - `declare_static = path`, `declare = path` and `recommends = path`, each optional: a function
///   the function of `PluginType` or `Plugin` of that name forwards to, taking what it takes.
///
/// On every field `serde` deserializes:
///
/// - `label = "..."`, required, and `description = "..."`, optional.
///
/// A field is in the schema under the name `serde` reads it by, and of the kind its type answers,
/// see `FieldType`. Its default is what `serde` fills a missing field with:
/// `#[serde(default)]`, on the field or the struct, or `None` for an `Option` that is not
/// deserialized with a function of its own. A default is serialized as the field is. The plugins
/// held by the fields are what `Configurable::plugins` returns.
///
/// # Errors
///
/// Fails to compile for anything but a struct with named fields or none, for a generic struct,
/// and for `serde` attributes that make the fields differ from what is deserialized:
/// `flatten`, `rename_all`, `transparent`, `from`, `try_from`, and a `rename` that differs between
/// serializing and deserializing.
#[proc_macro_derive(Plugin, attributes(plugin))]
pub fn derive_plugin(input: TokenStream) -> TokenStream {
    derive::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Registers a plugin for the axis whose trait the annotated impl implements: tags it for
/// `typetag` with its `PluginType::ID` and submits it to the catalog of the axis.
///
/// Takes no arguments. The axis has to be declared with `plugin_kind!` and its trait to be
/// `#[typetag::serde]`, and the crate using it has to depend on `typetag`.
#[proc_macro_attribute]
pub fn register(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let arguments = proc_macro2::TokenStream::from(arguments);
    register::expand(arguments, parse_macro_input!(input as ItemImpl))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
