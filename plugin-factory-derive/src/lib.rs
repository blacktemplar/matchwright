//! The derive macro of `plugin-factory`, re-exported by it; use it from there.

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, ItemTrait, parse_macro_input};

mod axis;
mod derive;
mod register;

/// Declares the annotated trait a plugin axis: tags it for `typetag` adjacently, by `"type"` and
/// `"value"`, and implements `Axis` for its trait object, which is how the axis is named from
/// then on, e.g. `dyn Metric`.
///
/// Takes no arguments. The trait needs `Plugin` as a supertrait, must not be generic and must not
/// carry a `typetag` attribute of its own; the crate declaring it has to depend on `typetag`.
#[proc_macro_attribute]
pub fn axis(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let arguments = proc_macro2::TokenStream::from(arguments);
    axis::expand(arguments, parse_macro_input!(input as ItemTrait))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Implements `Plugin`, `Configurable` and `PluginType` for a struct: its id, label and
/// description from `#[plugin(..)]` on the struct, and its schema and the plugins it holds from
/// its fields.
///
/// # Attributes
///
/// On the struct:
///
/// - `id = "..."` and `label = "..."`, required, and `description = "..."`, optional. The id is
///   also the name the plugin is persisted under, see `register`.
/// - `declare_static = path`, `declare = path` and `recommends = path`, each optional: a function
///   the function of `PluginType` or `Plugin` of that name forwards to, taking what it takes.
///
/// On every field `serde` deserializes:
///
/// - `label = "..."`, required, `description = "..."`, optional, and `default = expression` or
///   `default`, optional: the answer the field offers, an expression of the field's type, or its
///   `Default` impl;
/// - or `skip` alone, which leaves the field out of the schema, e.g. for state a plugin builds up
///   once it is configured. Such a field needs a `#[serde(default)]`, on the field or the struct,
///   which is what a plugin built from a config holds in it; a config answering it is not
///   rejected, but the answer is dropped.
///
/// A field is in the schema under the name `serde` reads it by, and of the kind its type answers,
/// see `FieldType`. The answer it offers is its `#[plugin(default ..)]`, serialized as the field
/// is; a `#[serde(default)]` only says what a missing field is read as, and offers nothing. The
/// plugins held by the fields of the schema are what `Configurable::plugins` returns.
///
/// # Errors
///
/// Fails to compile for anything but a struct with named fields or none, for a generic struct,
/// and for `serde` attributes that make the fields differ from what is deserialized:
/// `flatten`, `rename_all`, `transparent`, `from`, `try_from`, and a `rename` that differs between
/// serializing and deserializing. A field written as an `Option` needs `#[serde(default)]`, on
/// the field or the struct, or a function deserializing it, such as
/// `#[serde(deserialize_with = "Option::deserialize")]`, so that `serde` does not read it missing
/// as `None`.
#[proc_macro_derive(Plugin, attributes(plugin))]
pub fn derive_plugin(input: TokenStream) -> TokenStream {
    derive::expand_plugin(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Implements `Configurable` for a composite, a struct that is not a plugin: its schema and the
/// plugins it holds from its fields, as `Plugin` derives them for a plugin.
///
/// # Attributes
///
/// None on the struct. On every field `serde` deserializes, `label = "..."`, required, and
/// `description = "..."` and `default`, optional, or `skip`, as for `Plugin`.
///
/// # Errors
///
/// Fails to compile as `Plugin` does, and for `#[plugin(..)]` on the struct.
#[proc_macro_derive(Configurable, attributes(plugin))]
pub fn derive_configurable(input: TokenStream) -> TokenStream {
    derive::expand_configurable(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Registers a plugin for the axis whose trait the annotated impl implements: tags it for
/// `typetag` with its `PluginType::ID` and submits it to the catalog of the axis.
///
/// Takes no arguments. The axis has to be declared with `axis`, and the crate using it has to
/// depend on `typetag`.
#[proc_macro_attribute]
pub fn register(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let arguments = proc_macro2::TokenStream::from(arguments);
    register::expand(arguments, parse_macro_input!(input as ItemImpl))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
