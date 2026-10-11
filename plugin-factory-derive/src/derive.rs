use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, quote};
use syn::{
    Attribute, Data, DeriveInput, Error, Expr, ExprPath, Fields, Ident, LitStr, Result, Token,
    Type, ext::IdentExt, meta::ParseNestedMeta, token::Paren,
};

/// The `Plugin`, `Configurable` and `PluginType` impls of the struct `input`.
pub fn expand(input: &DeriveInput) -> Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "a plugin cannot be derived for a generic struct",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            &input.ident,
            "a plugin can only be derived for a struct",
        ));
    };
    let container = Container::parse(input)?;
    let is_unit = matches!(data.fields, Fields::Unit);
    let fields = match &data.fields {
        Fields::Named(fields) => collect(fields.named.iter().map(Field::parse))?,
        Fields::Unit => Vec::new(),
        Fields::Unnamed(_) => {
            return Err(Error::new_spanned(
                &data.fields,
                "a plugin can only be derived for a struct with named fields or none",
            ));
        }
    };
    let fields: Vec<_> = fields.into_iter().flatten().collect();

    let plugin = &input.ident;
    let Container {
        id,
        label,
        description,
        declare_static,
        declare,
        recommends,
        default,
    } = &container;
    let declare_static = declare_static.as_ref().map(|declare_static| {
        quote! {
            fn declare_static(declaration: &mut ::plugin_factory::compatibility::Declaration) {
                (#declare_static)(declaration)
            }
        }
    });
    let declare = declare.as_ref().map(|declare| {
        quote! {
            fn declare(&self, declaration: &mut ::plugin_factory::compatibility::Declaration) {
                (#declare)(self, declaration)
            }
        }
    });
    let recommends = recommends.as_ref().map(|recommends| {
        quote! {
            fn recommends(&self) -> ::std::vec::Vec<::plugin_factory::config::PluginConfig> {
                (#recommends)(self)
            }
        }
    });
    // a unit struct deserializes from `null` rather than from an empty config
    let is_unit = is_unit.then(|| {
        quote!(
            const IS_UNIT: bool = true;
        )
    });
    let schema_fields = fields
        .iter()
        .map(|field| field.schema_field(default.as_ref()));
    // a plugin without fields keeps the default, so that it reads as one written by hand would
    let plugins = (!fields.is_empty()).then(|| {
        let field_names = fields.iter().map(|field| &field.ident);
        quote! {
            fn plugins(&self) -> ::std::vec::Vec<&dyn ::plugin_factory::Plugin> {
                let mut plugins = ::std::vec::Vec::new();
                #(::plugin_factory::schema::FieldType::plugins(&self.#field_names, &mut plugins);)*
                plugins
            }
        }
    });

    Ok(quote! {
        impl ::plugin_factory::Plugin for #plugin {
            #declare
            #recommends
        }

        impl ::plugin_factory::schema::Configurable for #plugin {
            #is_unit

            fn schema() -> ::plugin_factory::schema::Schema {
                ::plugin_factory::schema::Schema::new() #(.field(#schema_fields))*
            }

            #plugins
        }

        impl ::plugin_factory::PluginType for #plugin {
            const ID: &'static str = #id;
            const LABEL: &'static str = #label;
            const DESCRIPTION: &'static str = #description;

            #declare_static
        }
    })
}

/// What the attributes of the struct say.
struct Container {
    id: LitStr,
    label: LitStr,
    description: LitStr,
    declare_static: Option<Expr>,
    declare: Option<Expr>,
    recommends: Option<Expr>,
    /// `#[serde(default)]` on the struct.
    default: Option<SerdeDefault>,
}

impl Container {
    fn parse(input: &DeriveInput) -> Result<Self> {
        let mut id = None;
        let mut label = None;
        let mut description = None;
        let mut declare_static = None;
        let mut declare = None;
        let mut recommends = None;
        for attribute in attributes(&input.attrs, "plugin") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("id") {
                    set_once(&mut id, &meta, meta.value()?.parse()?)
                } else if meta.path.is_ident("label") {
                    set_once(&mut label, &meta, meta.value()?.parse()?)
                } else if meta.path.is_ident("description") {
                    set_once(&mut description, &meta, meta.value()?.parse()?)
                } else if meta.path.is_ident("declare_static") {
                    set_once(&mut declare_static, &meta, meta.value()?.parse()?)
                } else if meta.path.is_ident("declare") {
                    set_once(&mut declare, &meta, meta.value()?.parse()?)
                } else if meta.path.is_ident("recommends") {
                    set_once(&mut recommends, &meta, meta.value()?.parse()?)
                } else {
                    Err(meta.error(
                        "expected `id`, `label`, `description`, `declare_static`, `declare` or \
                         `recommends`",
                    ))
                }
            })?;
        }

        let mut default = None;
        for attribute in attributes(&input.attrs, "serde") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("default") {
                    default = Some(SerdeDefault::parse(&meta)?);
                    Ok(())
                } else if ["rename_all", "transparent", "from", "try_from"]
                    .iter()
                    .any(|unsupported| meta.path.is_ident(unsupported))
                {
                    Err(meta.error(
                        "a plugin whose fields are not what serde deserializes cannot be derived",
                    ))
                } else {
                    skip(&meta)
                }
            })?;
        }

        let required = |value: Option<LitStr>, name: &str| {
            value.ok_or_else(|| {
                Error::new_spanned(
                    &input.ident,
                    format!("a plugin needs `#[plugin({name} = \"...\")]`"),
                )
            })
        };
        Ok(Self {
            id: required(id, "id")?,
            label: required(label, "label")?,
            description: required(description, "description")?,
            declare_static,
            declare,
            recommends,
            default,
        })
    }
}

/// What the attributes of a field `serde` deserializes say.
struct Field {
    ident: Ident,
    ty: Type,
    /// The name `serde` reads the field by.
    name: LitStr,
    label: LitStr,
    description: Option<LitStr>,
    default: Option<SerdeDefault>,
    /// `deserialize_with` or `with`, which disables the default `serde` gives a missing `Option`.
    deserialized_with: bool,
    /// The function the field is serialized with instead of its `Serialize` impl.
    serialize_with: Option<ExprPath>,
}

impl Field {
    /// The field `field`, or [`None`] if `serde` does not deserialize it.
    fn parse(field: &syn::Field) -> Result<Option<Self>> {
        let ident = field.ident.clone().expect("the fields are named");
        let mut name = LitStr::new(&ident.unraw().to_string(), ident.span());
        let mut default = None;
        let mut skipped = false;
        let mut deserialized_with = false;
        let mut serialize_with = None;
        for attribute in attributes(&field.attrs, "serde") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if !meta.input.peek(Token![=]) {
                        return Err(meta.error(
                            "a field of a plugin is read and written by one name, given by \
                             `rename = \"...\"`",
                        ));
                    }
                    name = meta.value()?.parse()?;
                } else if meta.path.is_ident("default") {
                    default = Some(SerdeDefault::parse(&meta)?);
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_deserializing") {
                    skipped = true;
                } else if meta.path.is_ident("flatten") {
                    return Err(meta.error("a flattened field cannot be part of a schema"));
                } else if meta.path.is_ident("deserialize_with") {
                    deserialized_with = true;
                    skip(&meta)?;
                } else if meta.path.is_ident("serialize_with") {
                    serialize_with = Some(meta.value()?.parse::<LitStr>()?.parse()?);
                } else if meta.path.is_ident("with") {
                    deserialized_with = true;
                    let mut module: ExprPath = meta.value()?.parse::<LitStr>()?.parse()?;
                    module
                        .path
                        .segments
                        .push(Ident::new("serialize", Span::call_site()).into());
                    serialize_with = Some(module);
                } else {
                    skip(&meta)?;
                }
                Ok(())
            })?;
        }
        if skipped {
            return Ok(None);
        }

        let mut label = None;
        let mut description = None;
        for attribute in attributes(&field.attrs, "plugin") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("label") {
                    set_once(&mut label, &meta, meta.value()?.parse()?)
                } else if meta.path.is_ident("description") {
                    set_once(&mut description, &meta, meta.value()?.parse()?)
                } else {
                    Err(meta.error("expected `label` or `description`"))
                }
            })?;
        }
        let label = label.ok_or_else(|| {
            Error::new_spanned(
                &ident,
                "a field of a plugin needs `#[plugin(label = \"...\")]`",
            )
        })?;

        Ok(Some(Self {
            ident,
            ty: field.ty.clone(),
            name,
            label,
            description,
            default,
            deserialized_with,
            serialize_with,
        }))
    }

    /// The `Field` of the schema, given the `#[serde(default)]` of the struct.
    fn schema_field(&self, container_default: Option<&SerdeDefault>) -> TokenStream {
        let Self {
            ident,
            ty,
            name,
            label,
            description,
            default,
            deserialized_with,
            serialize_with,
        } = self;
        let described = description
            .as_ref()
            .map(|description| quote!(let field = field.described(#description);));
        let default = match (default, container_default) {
            (Some(SerdeDefault::Trait), _) => {
                Some(quote!(<#ty as ::core::default::Default>::default()))
            }
            (Some(SerdeDefault::Function(function)), _) => Some(quote!(#function())),
            (None, Some(SerdeDefault::Trait)) => {
                Some(quote!(<Self as ::core::default::Default>::default().#ident))
            }
            (None, Some(SerdeDefault::Function(function))) => Some(quote!(#function().#ident)),
            (None, None) => None,
        };
        let default = match default {
            Some(default) => {
                let serialize = match serialize_with {
                    Some(serialize_with) => serialize_with.to_token_stream(),
                    None => quote!(::plugin_factory::__private::serde::Serialize::serialize),
                };
                quote! {
                    let field = field.with_default(
                        ::plugin_factory::__private::default_value(&#default, #serialize),
                    );
                }
            }
            // serde deserializes a missing field as what its type deserializes from nothing,
            // unless a function of its own deserializes it
            None if !deserialized_with => quote! {
                let field = match <#ty as ::plugin_factory::schema::FieldType>::missing() {
                    ::std::option::Option::Some(default) => field.with_default(default),
                    ::std::option::Option::None => field,
                };
            },
            None => TokenStream::new(),
        };
        quote! {
            {
                let field = ::plugin_factory::schema::Field::new(
                    #name,
                    #label,
                    <#ty as ::plugin_factory::schema::FieldType>::kind(),
                );
                #described
                #default
                field
            }
        }
    }
}

/// A `#[serde(default)]`.
enum SerdeDefault {
    /// `default`, i.e. the `Default` impl.
    Trait,
    /// `default = "function"`.
    Function(ExprPath),
}

impl SerdeDefault {
    fn parse(meta: &ParseNestedMeta) -> Result<Self> {
        if meta.input.peek(Token![=]) {
            Ok(Self::Function(meta.value()?.parse::<LitStr>()?.parse()?))
        } else {
            Ok(Self::Trait)
        }
    }
}

/// The attributes among `attributes` named `name`.
fn attributes<'a>(
    attributes: &'a [Attribute],
    name: &'a str,
) -> impl Iterator<Item = &'a Attribute> {
    attributes
        .iter()
        .filter(move |attribute| attribute.path().is_ident(name))
}

/// Sets `slot` to `value`, unless it was set before.
fn set_once<T>(slot: &mut Option<T>, meta: &ParseNestedMeta, value: T) -> Result<()> {
    if slot.is_some() {
        return Err(meta.error("given twice"));
    }
    *slot = Some(value);
    Ok(())
}

/// Skips the value of an item of a `serde` attribute the derive does not read.
fn skip(meta: &ParseNestedMeta) -> Result<()> {
    if meta.input.peek(Token![=]) {
        meta.value()?.parse::<Expr>()?;
    } else if meta.input.peek(Paren) {
        meta.input.parse::<TokenTree>()?;
    }
    Ok(())
}

/// Every value of `results`, or every error among them combined.
fn collect<T>(results: impl Iterator<Item = Result<T>>) -> Result<Vec<T>> {
    let mut values = Vec::new();
    let mut errors: Option<Error> = None;
    for result in results {
        match result {
            Ok(value) => values.push(value),
            Err(error) => match &mut errors {
                Some(errors) => errors.combine(error),
                None => errors = Some(error),
            },
        }
    }
    errors.map_or(Ok(values), Err)
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    fn error_of(input: DeriveInput) -> String {
        expand(&input).unwrap_err().to_string()
    }

    #[test]
    fn a_plugin_without_an_id_is_an_error() {
        assert_eq!(
            error_of(parse_quote! {
                #[plugin(label = "points", description = "points")]
                struct Points;
            }),
            "a plugin needs `#[plugin(id = \"...\")]`"
        );
    }

    #[test]
    fn a_field_without_a_label_is_an_error() {
        assert_eq!(
            error_of(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points {
                    rounds: i64,
                }
            }),
            "a field of a plugin needs `#[plugin(label = \"...\")]`"
        );
    }

    #[test]
    fn a_skipped_field_needs_no_label() {
        assert!(
            expand(&parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points {
                    #[serde(skip)]
                    cache: i64,
                }
            })
            .is_ok()
        );
    }

    #[test]
    fn a_flattened_field_is_an_error() {
        assert_eq!(
            error_of(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points {
                    #[serde(flatten)]
                    #[plugin(label = "rest")]
                    rest: Rest,
                }
            }),
            "a flattened field cannot be part of a schema"
        );
    }

    #[test]
    fn renamed_fields_are_an_error() {
        assert_eq!(
            error_of(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                #[serde(rename_all = "camelCase")]
                struct Points;
            }),
            "a plugin whose fields are not what serde deserializes cannot be derived"
        );
        assert!(
            error_of(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points {
                    #[serde(rename(deserialize = "rounds"))]
                    #[plugin(label = "rounds")]
                    count: i64,
                }
            })
            .starts_with("a field of a plugin is read and written by one name")
        );
    }

    #[test]
    fn an_attribute_given_twice_is_an_error() {
        assert_eq!(
            error_of(parse_quote! {
                #[plugin(id = "points", id = "score", label = "points", description = "points")]
                struct Points;
            }),
            "given twice"
        );
    }

    #[test]
    fn only_a_unit_struct_is_deserialized_from_null() {
        let is_unit = quote!(
            const IS_UNIT: bool = true;
        )
        .to_string();
        let expanded = |input: DeriveInput| expand(&input).unwrap().to_string();

        assert!(
            expanded(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points;
            })
            .contains(&is_unit)
        );
        assert!(
            !expanded(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points {}
            })
            .contains(&is_unit)
        );
    }

    #[test]
    fn a_tuple_struct_is_an_error() {
        assert_eq!(
            error_of(parse_quote! {
                #[plugin(id = "points", label = "points", description = "points")]
                struct Points(i64);
            }),
            "a plugin can only be derived for a struct with named fields or none"
        );
    }
}
