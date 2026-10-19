//! Every plugin a program has linked in, so that it can be found without being named.
//!
//! A plugin axis is a trait declared with [`axis`](crate::axis), named by its trait object, which
//! implements [`Axis`]. A plugin registers itself for its axis with [`register`](crate::register),
//! as one [`PluginEntry`], which describes it and states what it has to be told. Registration is
//! open: a plugin of a third-party crate ends up in the same catalog as the ones of the crate
//! defining the axis. The plugins of an axis are browsed through its [`AnyAxis`].
//!
//! A program reads the catalog to offer the plugins of an axis to choose from, and to settle a
//! fact a composite requires but none of its plugins provides, see
//! [`draft::finish`](crate::draft::finish).

use std::{
    any::{self, TypeId},
    fmt::{self, Debug, Formatter},
};

use serde_json::Map;

use crate::{
    __private::Index,
    Plugin, PluginType,
    compatibility::{Declaration, OpenFacts, ProvidedFact},
    config::{self, Config, ConfigError, Path, PluginConfig, Value},
    schema::Schema,
};

/// One axis of plugins, i.e. a plugin trait together with the registry the plugins of that trait
/// register in, implemented for the trait object, e.g. `dyn Metric`. Its plugins are browsed
/// through [`AnyAxis::of`].
///
/// Implemented by [`axis`](crate::axis) alone, which is also the only way to get the registry.
/// The trait of an axis has [`Plugin`] as a supertrait, so that any plugin can be asked what it
/// declares; a trait without it does not compile as an axis:
///
/// ```compile_fail
/// #[plugin_factory::axis]
/// pub trait Seeding: std::fmt::Debug {}
/// ```
///
/// # Examples
/// ```
/// use plugin_factory::{Plugin, axis, catalog::AnyAxis, register};
/// use serde::{Deserialize, Serialize};
///
/// /// How the entrants of a phase are seeded, an axis of a crate building on the engine.
/// #[axis]
/// pub trait Seeding: Plugin {
///     fn seed(&self);
/// }
///
/// #[derive(Debug, Serialize, Deserialize, Plugin)]
/// #[plugin(
///     id = "my_crate::by_rank",
///     label = "by rank",
///     description = "the best entrant is seeded first"
/// )]
/// struct ByRank;
///
/// #[register]
/// impl Seeding for ByRank {
///     fn seed(&self) {}
/// }
///
/// # fn main() {
/// assert_eq!(
///     AnyAxis::of::<dyn Seeding>()
///         .entry("my_crate::by_rank")
///         .unwrap()
///         .label(),
///     "by rank"
/// );
/// # }
/// ```
pub trait Axis: Plugin {
    /// The registry of this axis. Implemented by [`axis`](crate::axis); not part of the API.
    #[doc(hidden)]
    fn __index() -> &'static Index;
}

/// One plugin as it is registered in the catalog.
///
/// Everything in it comes from the plugin's type, so registering one takes no strings; submitted
/// by [`register`](crate::register), which is the only way into the catalog. An entry describes
/// the plugin but does not build it: a plugin is built as a plugin of its axis, see
/// [`PluginConfig::build`].
pub struct PluginEntry {
    id: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    declare: fn(&mut Declaration),
    instantiate: Instantiate,
    schema: fn() -> Schema,
    complete: fn(Config) -> Value,
    type_name: fn() -> &'static str,
}

impl PluginEntry {
    /// The entry of the plugin `P`.
    pub(crate) const fn new<P: PluginType>() -> Self {
        Self {
            id: P::ID,
            label: P::LABEL,
            description: P::DESCRIPTION,
            declare: P::declare_static,
            instantiate: instantiate::<P>,
            schema: P::schema,
            complete: config::complete::<P>,
            type_name: any::type_name::<P>,
        }
    }

    /// What identifies the plugin, see [`PluginType::ID`].
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// What an operator picking a plugin reads.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// What an operator picking a plugin reads about what it does, if anything.
    pub fn description(&self) -> Option<&'static str> {
        self.description
    }

    /// What the plugin provides and requires whatever its configuration, see
    /// [`PluginType::declare_static`]. Built on every call.
    pub(crate) fn declaration(&self) -> Declaration {
        Declaration::of(self.declare)
    }

    /// What the plugin as `config` configures it provides and requires, its sub-plugins included:
    /// the [static declaration](Self::declaration), what this configuration adds to it, see
    /// [`Plugin::declare`], and what the plugins in its fields declare.
    ///
    /// Builds the plugin from `config`, see [`Self::instance`], and drops it again.
    ///
    /// # Errors
    /// As [`Self::instance`], with paths relative to the plugin.
    pub(crate) fn declaration_for(&self, config: &Config) -> Result<Declaration, ConfigError> {
        let instance = self.instance(config, &Path::ROOT)?;
        Ok(Declaration::of(|declaration| {
            instance.declare_all(declaration);
        }))
    }

    /// The plugin built from `config`, its config at `at`, for what only a configured plugin can
    /// say: what it declares on top of its static declaration, and what it recommends.
    ///
    /// # Errors
    /// Returns [`ConfigError::Rejected`] if the plugin's `Deserialize` impl rejects the config,
    /// and whatever walking `config` with every field to be answered fails with otherwise, e.g.
    /// [`ConfigError::Unanswered`], see [`Self::walk`].
    pub(crate) fn instance(
        &self,
        config: &Config,
        at: &Path,
    ) -> Result<Box<dyn Plugin>, ConfigError> {
        (self.instantiate)(&self.walk(config, at, true)?, at)
    }

    /// What has to be answered in order to create the plugin, see
    /// [`Configurable::schema`](crate::schema::Configurable::schema). Built on every call.
    pub fn schema(&self) -> Schema {
        (self.schema)()
    }

    /// The plugin with each field answered by the answer it offers, see
    /// [`Field::default_value`](crate::schema::Field::default_value). A field offering no answer
    /// is left unanswered.
    pub fn default_config(&self) -> PluginConfig {
        let mut config = Config::new();
        for field in self.schema().fields() {
            if let Some(default) = field.default_value() {
                config.set(field.name(), default.clone());
            }
        }
        PluginConfig::configured(self.id, config)
    }

    /// The axes of the plugins the fields of the plugin's schema take, in schema order, an axis
    /// once per field taking it.
    pub(crate) fn sub_axes(&self) -> Vec<AnyAxis> {
        self.schema()
            .fields()
            .iter()
            .filter_map(|field| field.kind().axis())
            .collect()
    }

    /// `config`, the config of the plugin at `at`, walked as [`Schema::walk`] walks it.
    ///
    /// If `complete`, this is the value the plugin is deserialized from, as
    /// [`config::complete`] writes it.
    ///
    /// # Errors
    /// Returns what [`Schema::walk`] returns.
    pub(crate) fn walk(
        &self,
        config: &Config,
        at: &Path,
        complete: bool,
    ) -> Result<Value, ConfigError> {
        let walked = self.schema().walk(config, at, complete)?;
        // an answer still being edited keeps an object as its config, which a draft edits
        if complete {
            return Ok((self.complete)(walked));
        }
        Ok(walked.into())
    }

    /// The name of the plugin's type, for an error message.
    pub(crate) fn type_name(&self) -> &'static str {
        (self.type_name)()
    }
}

impl Debug for PluginEntry {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PluginEntry")
            .field(&self.id())
            .finish()
    }
}

/// How a [`PluginEntry`] builds its plugin, see [`instantiate`].
type Instantiate = fn(&Value, &Path) -> Result<Box<dyn Plugin>, ConfigError>;

/// The plugin `P` deserialized from `config`, its complete config at `at`, see
/// [`PluginEntry::instance`].
fn instantiate<P: PluginType>(config: &Value, at: &Path) -> Result<Box<dyn Plugin>, ConfigError> {
    Ok(Box::new(config::deserialize::<P>(config, at)?))
}

/// An [`Axis`] as a value, through which its registered plugins are browsed, e.g. the axis of the
/// plugins a field takes, see [`FieldKind::Plugin`](crate::schema::FieldKind::Plugin).
///
/// Two values are equal if they are of the same axis.
#[derive(Clone, Copy)]
pub struct AnyAxis {
    id: TypeId,
    index: fn() -> &'static Index,
}

impl AnyAxis {
    /// The axis `A`, e.g. `AnyAxis::of::<dyn Metric>()`.
    pub const fn of<A: Axis + ?Sized>() -> Self {
        Self {
            id: TypeId::of::<A>(),
            index: A::__index,
        }
    }

    /// Every plugin registered for this axis, in the order of their ids.
    ///
    /// # Panics
    /// Panics if two plugins are registered for this axis with the same id, see [`Self::entry`].
    pub fn entries(&self) -> impl Iterator<Item = &'static PluginEntry> {
        (self.index)().entries().values().copied()
    }

    /// The plugin registered for this axis with the id `id`, see [`PluginType::ID`], or [`None`]
    /// if none is.
    ///
    /// # Panics
    /// Panics if two plugins are registered for this axis with the same id, naming the id and
    /// both plugins. Every access to the catalog of the axis checks this.
    pub fn entry(&self, id: &str) -> Option<&'static PluginEntry> {
        (self.index)().entries().get(id).copied()
    }

    /// `plugin`, the answer at `at`, as a plugin of this axis with its config walked as
    /// [`PluginEntry::walk`] walks it, written as [`PluginConfig`] writes a plugin answer.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownPlugin`] if no plugin of this axis is registered with the id
    /// of `plugin`, and what [`PluginEntry::walk`] returns otherwise.
    pub(crate) fn walk(
        &self,
        plugin: &PluginConfig,
        at: &Path,
        complete: bool,
    ) -> Result<Value, ConfigError> {
        let entry = self
            .entry(plugin.plugin())
            .ok_or_else(|| ConfigError::UnknownPlugin { path: at.clone() })?;
        Ok(Value::Object(Map::from_iter([
            ("type".to_owned(), Value::String(plugin.plugin().to_owned())),
            (
                "value".to_owned(),
                entry.walk(plugin.config(), at, complete)?,
            ),
        ])))
    }

    /// The types of the facts the plugins of this axis provide, i.e. what a slot for this axis
    /// could contribute while it is still to be filled, see [`OpenFacts`].
    ///
    /// Includes what the plugins of their [sub-axes](PluginEntry::sub_axes) provide,
    /// transitively, since a plugin filling the slot can have slots of its own. Read from the
    /// static declarations, so it holds whatever the plugins end up configured with, and is
    /// collected once per axis.
    pub(crate) fn facts(&self) -> &'static OpenFacts {
        (self.index)().facts.get_or_init(|| self.collect_facts())
    }

    /// [`Self::facts`], collected anew.
    fn collect_facts(&self) -> OpenFacts {
        let mut visited = vec![*self];
        let mut pending = vec![*self];
        let mut facts = Vec::new();
        while let Some(axis) = pending.pop() {
            for entry in axis.entries() {
                facts.extend(
                    entry
                        .declaration()
                        .provided()
                        .iter()
                        .map(ProvidedFact::type_id),
                );
                for sub_axis in entry.sub_axes() {
                    // axes can nest each other, so each is visited once
                    if !visited.contains(&sub_axis) {
                        visited.push(sub_axis);
                        pending.push(sub_axis);
                    }
                }
            }
        }
        facts.into_iter().collect()
    }
}

impl PartialEq for AnyAxis {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for AnyAxis {}

impl Debug for AnyAxis {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("AnyAxis").field(&self.id).finish()
    }
}

/// The catalog itself, tested on an axis of its own so that it does not depend on what the plugins
/// of a program happen to register.
#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        axis,
        compatibility::Fact,
        register,
        schema::{Configurable, Field},
    };

    /// A fact exactly one registered plugin provides unasked.
    #[derive(Debug)]
    struct Settled;

    impl Fact for Settled {}

    /// A fact two registered plugins provide unasked.
    #[derive(Debug)]
    struct Ambiguous;

    impl Fact for Ambiguous {}

    /// A fact only a plugin that has to be configured provides.
    #[derive(Debug)]
    struct Configured;

    impl Fact for Configured {}

    /// A fact a plugin provides unasked and a plugin that has to be configured provides too.
    #[derive(Debug)]
    struct Shared;

    impl Fact for Shared {}

    /// A plugin axis with no behavior at all, so that a test plugin is its declaration.
    #[axis]
    trait Tested: Plugin {}

    /// The only plugin providing [`Settled`], and one of the two providing [`Shared`].
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "only_provider",
        label = "only provider",
        description = "the one plugin providing the settled fact",
        declare_static = Self::declare_static
    )]
    struct OnlyProvider;

    impl OnlyProvider {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Settled).provide(Shared);
        }
    }

    #[register]
    impl Tested for OnlyProvider {}

    /// One of the two plugins providing [`Ambiguous`].
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "one_provider",
        label = "one provider",
        description = "a plugin providing the ambiguous fact",
        declare_static = provides_ambiguous
    )]
    struct OneProvider;

    #[register]
    impl Tested for OneProvider {}

    /// The other plugin providing [`Ambiguous`].
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "another_provider",
        label = "another provider",
        description = "the other plugin providing the ambiguous fact",
        declare_static = provides_ambiguous
    )]
    struct AnotherProvider;

    #[register]
    impl Tested for AnotherProvider {}

    fn provides_ambiguous(declaration: &mut Declaration) {
        declaration.provide(Ambiguous);
    }

    /// A plugin with a field without a default, so that the catalog cannot create it. The answer
    /// is only read in its [`Debug`] form, since a boxed plugin cannot be downcast.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "configured_provider",
        label = "configured provider",
        description = "a plugin that has to be configured",
        declare_static = Self::declare_static
    )]
    struct ConfiguredProvider {
        #[plugin(label = "Rounds")]
        rounds: i64,
    }

    impl ConfiguredProvider {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Configured).provide(Shared);
        }
    }

    #[register]
    impl Tested for ConfiguredProvider {}

    /// A fact only a plugin of [`Nested`] provides.
    #[derive(Debug)]
    struct Deep;

    impl Fact for Deep {}

    /// A plugin axis whose plugins only ever fill a field of a plugin of [`Tested`].
    #[axis]
    trait Nested: Plugin {}

    /// A plugin with a field taking a plugin or nothing, of an axis its own axis takes nowhere
    /// else.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "wrapper",
        label = "wrapper",
        description = "a plugin with a slot of another axis"
    )]
    struct Wrapper {
        #[serde(deserialize_with = "Option::deserialize")]
        #[plugin(label = "Nested")]
        nested: Option<Box<dyn Nested>>,
    }

    #[register]
    impl Tested for Wrapper {}

    /// The plugin providing [`Deep`], with a field of the axis its wrapper is of, so that the two
    /// axes nest each other.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "deep_provider",
        label = "deep provider",
        description = "the one plugin providing the deep fact",
        declare_static = Self::declare_static
    )]
    struct DeepProvider {
        #[serde(deserialize_with = "Option::deserialize")]
        #[plugin(label = "Inner")]
        inner: Option<Box<dyn Tested>>,
    }

    impl DeepProvider {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Deep).provide(Crosswise);
        }
    }

    #[register]
    impl Nested for DeepProvider {}

    /// A fact only [`Chained`] provides.
    #[derive(Debug)]
    struct Downstream;

    impl Fact for Downstream {}

    /// A fact a plugin of [`Tested`] and one of [`Nested`] provide unasked.
    #[derive(Debug)]
    struct Crosswise;

    impl Fact for Crosswise {}

    /// The only plugin providing [`Downstream`], requiring what [`OnlyProvider`] provides.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "chained",
        label = "chained",
        description = "a plugin requiring the settled fact",
        declare_static = Self::declare_static
    )]
    struct Chained;

    impl Chained {
        fn declare_static(declaration: &mut Declaration) {
            declaration
                .provide(Downstream)
                .provide(Crosswise)
                .require::<Settled>("the settled fact");
        }
    }

    #[register]
    impl Tested for Chained {}

    /// A plugin whose `Plugin` and `PluginType` are written by hand rather than derived,
    /// registered as a derived one is.
    #[derive(Debug, Serialize, Deserialize, Configurable)]
    struct Handwritten;

    impl Plugin for Handwritten {}

    impl PluginType for Handwritten {
        const ID: &'static str = "handwritten";
        const LABEL: &'static str = "handwritten";
    }

    #[register]
    impl Tested for Handwritten {}

    /// The entry of the plugin `P`.
    fn entry_of<P: PluginType>() -> &'static PluginEntry {
        AnyAxis::of::<dyn Tested>()
            .entry(P::ID)
            .expect("the plugin is registered")
    }

    #[test]
    fn an_entry_describes_the_plugin_it_registers() {
        let entry = entry_of::<OnlyProvider>();

        assert_eq!(entry.id(), "only_provider");
        assert_eq!(entry.label(), OnlyProvider::LABEL);
        assert_eq!(
            entry.description(),
            Some("the one plugin providing the settled fact")
        );
        assert!(entry.declaration().provides_type(TypeId::of::<Settled>()));
        assert!(entry.schema().fields().is_empty());
        assert_eq!(entry_of::<Handwritten>().description(), None);
    }

    #[test]
    fn a_plugin_is_built_from_the_answers_to_its_schema() {
        let entry = entry_of::<ConfiguredProvider>();
        let configured = PluginConfig::of::<ConfiguredProvider>;

        assert_eq!(
            entry.schema().fields().iter().map(Field::name).next(),
            Some("rounds")
        );
        // a built plugin is boxed behind the trait of the axis, so its Debug form is what a test
        // can compare
        let built = configured().with("rounds", 5).build::<dyn Tested>();
        assert_eq!(format!("{built:?}"), "Ok(ConfiguredProvider { rounds: 5 })");
        assert!(configured().build::<dyn Tested>().is_err());
    }

    #[test]
    fn a_plugin_of_another_axis_is_not_built() {
        assert_eq!(
            PluginConfig::of::<DeepProvider>()
                .build::<dyn Tested>()
                .err(),
            Some(ConfigError::UnknownPlugin { path: Path::ROOT })
        );
    }

    #[test]
    fn an_axis_as_a_value_finds_the_entries_of_the_axis() {
        let axis = AnyAxis::of::<dyn Tested>();

        assert_eq!(
            axis.entry("wrapper").map(PluginEntry::label),
            Some("wrapper")
        );
        assert!(axis.entry("deep_provider").is_none());
        assert_eq!(axis, AnyAxis::of::<dyn Tested>());
        assert_ne!(axis, AnyAxis::of::<dyn Nested>());
    }

    #[test]
    fn a_configured_declaration_includes_the_sub_plugins() {
        let entry = entry_of::<Wrapper>();
        let wrapping = Config::new().with(
            "nested",
            PluginConfig::of::<DeepProvider>().with("inner", Value::Null),
        );
        let deep = TypeId::of::<Deep>();

        assert!(!entry.declaration().provides_type(deep));
        assert!(
            !entry
                .declaration_for(&Config::new().with("nested", Value::Null))
                .unwrap()
                .provides_type(deep)
        );
        assert!(
            entry
                .declaration_for(&wrapping)
                .unwrap()
                .provides_type(deep)
        );
    }

    #[test]
    fn a_configured_declaration_of_an_incomplete_config_is_an_error() {
        assert_eq!(
            entry_of::<ConfiguredProvider>()
                .declaration_for(&Config::new())
                .err(),
            Some(ConfigError::Unanswered {
                path: Path::of("rounds"),
            })
        );
    }

    #[test]
    fn an_id_no_plugin_is_registered_with_has_no_entry() {
        assert!(AnyAxis::of::<dyn Tested>().entry("unregistered").is_none());
    }

    #[test]
    fn the_plugins_of_an_axis_come_in_the_order_of_their_ids() {
        let ids: Vec<_> = AnyAxis::of::<dyn Tested>()
            .entries()
            .map(PluginEntry::id)
            .collect();

        assert_eq!(
            ids,
            [
                "another_provider",
                "chained",
                "configured_provider",
                "handwritten",
                "one_provider",
                "only_provider",
                "wrapper",
            ]
        );
    }

    #[test]
    fn a_plugin_written_by_hand_is_built_by_its_id() {
        let built = PluginConfig::of::<Handwritten>().build::<dyn Tested>();

        assert_eq!(format!("{built:?}"), "Ok(Handwritten)");
        assert_eq!(
            serde_json::to_value(built.unwrap()).unwrap(),
            serde_json::json!({"type": "handwritten", "value": null})
        );
    }

    #[test]
    fn the_facts_of_an_axis_are_what_its_plugins_and_theirs_provide() {
        let facts = OpenFacts::from_iter([
            TypeId::of::<Settled>(),
            TypeId::of::<Ambiguous>(),
            TypeId::of::<Configured>(),
            TypeId::of::<Shared>(),
            TypeId::of::<Deep>(),
            TypeId::of::<Downstream>(),
            TypeId::of::<Crosswise>(),
        ]);

        // the two axes nest each other, so either one could contribute everything
        assert_eq!(AnyAxis::of::<dyn Tested>().facts(), &facts);
        assert_eq!(AnyAxis::of::<dyn Nested>().facts(), &facts);
        assert_eq!(AnyAxis::of::<dyn Tested>().collect_facts(), facts);
    }

    /// An axis two of whose plugins share an id.
    #[axis]
    trait Clashing: Plugin {}

    /// One of the two plugins registered with the id `clash`.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "clash",
        label = "first",
        description = "the first plugin with the id"
    )]
    struct First;

    #[register]
    impl Clashing for First {}

    /// The other plugin registered with the id `clash`.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "clash",
        label = "second",
        description = "the second plugin with the id"
    )]
    struct Second;

    #[register]
    impl Clashing for Second {}

    #[test]
    #[should_panic(expected = "are both registered with the id \"clash\"")]
    fn two_plugins_registered_with_the_same_id_panic_on_access() {
        AnyAxis::of::<dyn Clashing>().entry("clash");
    }

    #[test]
    fn the_panic_on_a_shared_id_names_both_plugins() {
        let message = std::panic::catch_unwind(|| AnyAxis::of::<dyn Clashing>().entries().count())
            .unwrap_err()
            .downcast::<String>()
            .unwrap();

        assert!(message.contains(any::type_name::<First>()));
        assert!(message.contains(any::type_name::<Second>()));
    }
}
