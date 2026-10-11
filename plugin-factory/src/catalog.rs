//! Every plugin a program has linked in, so that it can be found without being named.
//!
//! A plugin axis is a trait declared with [`axis`](crate::axis), named by its trait object, which
//! implements [`Axis`]. A plugin registers itself for its axis with [`register`](crate::register),
//! as one [`PluginEntry`], which describes it and states what it has to be told. Registration is
//! open: a plugin of a third-party crate ends up in the same catalog as the ones of the crate
//! defining the axis.
//!
//! A program reads the catalog to settle a fact a composite requires but none of its plugins
//! provides, see [`resolve`], and to offer the plugins of an axis to choose from.

use std::{
    any::{self, TypeId},
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
    sync::OnceLock,
};

use serde::de::DeserializeOwned;
use serde_json::Map;

use crate::{
    Plugin, PluginType,
    compatibility::{self, Declaration, OpenFacts, ProvidedFact},
    config::{self, Config, ConfigError, Path, PluginConfig, Value},
    schema::Schema,
};

/// One axis of plugins, i.e. a plugin trait together with the registry the plugins of that trait
/// register in, implemented for the trait object, e.g. `dyn Metric`.
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
/// use plugin_factory::{Plugin, axis, catalog::Axis, register};
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
///     <dyn Seeding>::entry("my_crate::by_rank").unwrap().label(),
///     "by rank"
/// );
/// # }
/// ```
pub trait Axis: Plugin {
    /// Every plugin registered for this axis, in the order of their ids.
    ///
    /// # Panics
    /// Panics if two plugins are registered for this axis with the same id, see [`Self::entry`].
    fn entries() -> impl Iterator<Item = &'static PluginEntry> {
        Self::__index().values().copied()
    }

    /// The plugin registered for this axis with the id `id`, see [`PluginType::ID`], or [`None`]
    /// if none is.
    ///
    /// # Panics
    /// Panics if two plugins are registered for this axis with the same id, naming the id and
    /// both plugins. Every access to the catalog of the axis checks this.
    fn entry(id: &str) -> Option<&'static PluginEntry> {
        Self::__index().get(id).copied()
    }

    /// The plugins registered for this axis by id, see [`Index`].
    #[doc(hidden)]
    fn __index() -> &'static BTreeMap<&'static str, &'static PluginEntry>;
}

/// The plugins registered for one axis by id, built on first access. Held by the code
/// [`axis`](crate::axis) generates; not part of the API.
#[doc(hidden)]
pub struct Index(OnceLock<BTreeMap<&'static str, &'static PluginEntry>>);

impl Index {
    #[expect(clippy::new_without_default, reason = "only ever built in a static")]
    pub const fn new() -> Self {
        Self(OnceLock::new())
    }

    /// The plugins of `registered` by id, built from it on the first call only.
    ///
    /// # Panics
    /// Panics if two of them share an id.
    pub fn get(
        &self,
        registered: impl IntoIterator<Item = &'static PluginEntry>,
    ) -> &BTreeMap<&'static str, &'static PluginEntry> {
        self.0.get_or_init(|| {
            let mut index = BTreeMap::new();
            for entry in registered {
                if let Some(other) = index.insert(entry.id, entry) {
                    panic!(
                        "the plugins {} and {} are both registered with the id {:?}",
                        (other.type_name)(),
                        (entry.type_name)(),
                        entry.id,
                    );
                }
            }
            index
        })
    }
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
    description: &'static str,
    declare: fn(&mut Declaration),
    instantiate: fn(&Value) -> Result<Box<dyn Plugin>, ConfigError>,
    schema: fn() -> Schema,
    is_unit: bool,
    type_name: fn() -> &'static str,
}

impl PluginEntry {
    /// The entry of the plugin `P`, built by the code [`register`](crate::register) generates.
    #[doc(hidden)]
    pub const fn new<P: PluginType>() -> Self {
        Self {
            id: P::ID,
            label: P::LABEL,
            description: P::DESCRIPTION,
            declare: P::declare_static,
            instantiate: instantiate::<P>,
            schema: P::schema,
            is_unit: P::IS_UNIT,
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

    /// What an operator picking a plugin reads about what it does.
    pub fn description(&self) -> &'static str {
        self.description
    }

    /// What the plugin provides and requires whatever its configuration, see
    /// [`PluginType::declare_static`]. Built on every call.
    pub fn declaration(&self) -> Declaration {
        Declaration::of(self.declare)
    }

    /// What the plugin as `config` configures it provides and requires, its sub-plugins included:
    /// the [static declaration](Self::declaration), what this configuration adds to it, see
    /// [`Plugin::declare`], and what the plugins in its fields declare.
    ///
    /// Builds the plugin from `config` completed, see [`Self::complete`], and drops it again.
    ///
    /// # Errors
    /// Returns [`ConfigError::Rejected`] if the plugin's `Deserialize` impl rejects the completed
    /// config, and whatever completing `config` fails with otherwise.
    pub fn declaration_for(&self, config: &Config) -> Result<Declaration, ConfigError> {
        let instance = self.instance(config)?;
        Ok(Declaration::of(|declaration| {
            instance.declare_all(declaration);
        }))
    }

    /// The plugin built from `config` completed, see [`Self::complete`], for what only a
    /// configured plugin can say: what it declares on top of its static declaration, and what it
    /// recommends.
    ///
    /// # Errors
    /// As [`Self::declaration_for`].
    pub(crate) fn instance(&self, config: &Config) -> Result<Box<dyn Plugin>, ConfigError> {
        (self.instantiate)(&self.complete(config)?)
    }

    /// What has to be answered in order to create the plugin, see
    /// [`Configurable::schema`](crate::schema::Configurable::schema). Built on every call.
    pub fn schema(&self) -> Schema {
        (self.schema)()
    }

    /// The axes of the plugins the fields of the plugin's schema take, in schema order, an axis
    /// once per field taking it.
    pub fn sub_axes(&self) -> Vec<AnyAxis> {
        self.schema()
            .fields()
            .iter()
            .filter_map(|field| field.kind().axis())
            .collect()
    }

    /// `config` completed against the schema of the plugin, see [`Schema::complete`], as the
    /// value the plugin is deserialized from: the object of its answers, or `null` for a plugin
    /// that is a unit struct, which is how serde writes one.
    ///
    /// A plugin every field of whose schema has a default needs no configuration, i.e. completes
    /// [`Config::new`], see [`add_provider_of`].
    ///
    /// # Errors
    /// Returns whatever completing `config` fails with, see [`Schema::complete`].
    pub fn complete(&self, config: &Config) -> Result<Value, ConfigError> {
        let completed = self.schema().complete(config)?;
        if self.is_unit {
            return Ok(Value::Null);
        }
        Ok(completed.into())
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

/// The plugin `P` deserialized from `config`, its completed config, see
/// [`PluginEntry::instance`].
fn instantiate<P: PluginType>(config: &Value) -> Result<Box<dyn Plugin>, ConfigError> {
    Ok(Box::new(config::deserialize::<P>(config)?))
}

/// An [`Axis`] as a value, i.e. the axis of the plugins a field takes, see
/// [`FieldKind::Plugin`](crate::schema::FieldKind::Plugin), through which its registered plugins
/// can be browsed without naming the axis.
///
/// Two values are equal if they are of the same axis.
#[derive(Clone, Copy)]
pub struct AnyAxis {
    id: TypeId,
    index: fn() -> &'static BTreeMap<&'static str, &'static PluginEntry>,
}

impl AnyAxis {
    /// The axis `A`, e.g. `AnyAxis::of::<dyn Metric>()`.
    pub const fn of<A: Axis + ?Sized>() -> Self {
        Self {
            id: TypeId::of::<A>(),
            index: A::__index,
        }
    }

    /// Every plugin registered for this axis, in the order of their ids, see [`Axis::entries`].
    ///
    /// # Panics
    /// As [`Axis::entries`].
    pub fn entries(&self) -> impl Iterator<Item = &'static PluginEntry> {
        (self.index)().values().copied()
    }

    /// The plugin registered for this axis with the id `id`, or [`None`] if none is, see
    /// [`Axis::entry`].
    ///
    /// # Panics
    /// As [`Axis::entry`].
    pub fn entry(&self, id: &str) -> Option<&'static PluginEntry> {
        (self.index)().get(id).copied()
    }

    /// `plugin` as a plugin of this axis with its config completed, see
    /// [`PluginEntry::complete`], written as [`PluginConfig`] writes a plugin answer.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownPlugin`] if no plugin of this axis is registered with the id
    /// of `plugin`, and whatever completing its config fails with otherwise.
    pub fn complete(&self, plugin: &PluginConfig) -> Result<Value, ConfigError> {
        let entry = self
            .entry(plugin.plugin())
            .ok_or(ConfigError::UnknownPlugin { path: Path::ROOT })?;
        Ok(Value::Object(Map::from_iter([
            ("type".to_owned(), Value::String(plugin.plugin().to_owned())),
            ("value".to_owned(), entry.complete(plugin.config())?),
        ])))
    }

    /// `plugin` as a plugin of this axis with its config checked but not completed, see
    /// [`Schema::check`], written as [`PluginConfig`] writes a plugin answer.
    ///
    /// # Errors
    /// As [`Self::complete`], except that an unanswered field is no error.
    pub(crate) fn check(&self, plugin: &PluginConfig) -> Result<Value, ConfigError> {
        let entry = self
            .entry(plugin.plugin())
            .ok_or(ConfigError::UnknownPlugin { path: Path::ROOT })?;
        let checked = entry.schema().check(plugin.config())?;
        Ok(PluginConfig::new(plugin.plugin(), checked).into())
    }

    /// The types of the facts the plugins of this axis provide, i.e. what a slot for this axis
    /// could contribute while it is still to be filled, see [`OpenFacts`].
    ///
    /// Includes what the plugins of their [sub-axes](PluginEntry::sub_axes) provide,
    /// transitively, since a plugin filling the slot can have slots of its own. Read from the
    /// static declarations, so it holds whatever the plugins end up configured with.
    pub fn facts(&self) -> OpenFacts {
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

/// A composite of plugins that a plugin of the axis `A` may be added to, see
/// [`add_provider_of`].
///
/// A composite with a single slot for an axis — a phase is played in one format — leaves the impl
/// for that axis out, so that nothing can be added to it.
pub trait Accepts<A: Axis + ?Sized> {
    /// Adds `plugin` to the composite.
    fn add(&mut self, plugin: Box<A>);
}

/// Adds a plugin providing a fact of type `fact` to `composite`, if exactly one plugin of the
/// axis `A` is registered that provides it and needs no configuration.
///
/// Adds nothing if none does, or if several do: which of them to use is then not a program's
/// choice to make, and the fact stays unprovided.
pub fn add_provider_of<A, C>(composite: &mut C, fact: TypeId)
where
    A: Axis + ?Sized,
    C: Accepts<A> + ?Sized,
    Box<A>: DeserializeOwned,
{
    let mut providers = providers_of::<A>(fact).filter_map(|entry| {
        PluginConfig::new(entry.id(), Config::new())
            .build::<A>()
            .ok()
    });
    if let Some(plugin) = providers.next()
        && providers.next().is_none()
    {
        composite.add(plugin);
    }
}

/// A composite of plugins as [`resolve`] sees it: what its plugins declare, and the axes a plugin
/// may be added for.
pub trait Composite: Sized + 'static {
    /// The axes the catalog may add plugins of to the composite, each with how it is added, see
    /// [`Acceptor::of`]. An axis listed twice is never added, since which of its slots to add to
    /// is not a program's choice to make.
    const ACCEPTS: &'static [Acceptor<Self>];

    /// What the plugins of the composite declare, each named as the checks report it, see
    /// [`declaration_of`](crate::compatibility::declaration_of).
    fn declarations(&self) -> Vec<(String, Declaration)>;
}

/// An axis a [`Composite`] `C` accepts plugins of, see [`Composite::ACCEPTS`].
pub struct Acceptor<C> {
    axis: AnyAxis,
    add: fn(&mut C, &PluginEntry),
}

impl<C> Acceptor<C> {
    /// The axis `A`, whose plugins are added to `C` through its [`Accepts`] impl.
    pub const fn of<A: Axis + ?Sized>() -> Self
    where
        C: Accepts<A>,
        Box<A>: DeserializeOwned,
    {
        Self {
            axis: AnyAxis::of::<A>(),
            add: add_unasked::<A, C>,
        }
    }

    /// The axis.
    pub fn axis(&self) -> AnyAxis {
        self.axis
    }
}

/// Adds the plugin of `entry`, a plugin of the axis `A` that needs no configuration, to
/// `composite`.
fn add_unasked<A: Axis + ?Sized, C: Accepts<A>>(composite: &mut C, entry: &PluginEntry)
where
    Box<A>: DeserializeOwned,
{
    // the entry was found among the plugins that build unasked, and register tags a plugin with
    // its id, so this does not fail
    if let Ok(plugin) = PluginConfig::new(entry.id(), Config::new()).build::<A>() {
        composite.add(plugin);
    }
}

/// Adds a plugin to `composite` for every fact its plugins require and neither they nor `context`
/// provide, including the facts the added plugins require themselves.
///
/// A fact is settled only if exactly one registered plugin of all the axes the composite
/// [accepts](Composite::ACCEPTS) provides it and needs no configuration; otherwise it stays
/// unprovided, which [`check_facts`](crate::compatibility::check_facts) reports. Each fact is
/// tried once. Plugins are added in the order their facts are first required in, so ordering
/// them by what they require of each other is left to the caller, see
/// [`order`](crate::compatibility::order).
pub fn resolve<C: Composite>(context: &Declaration, composite: &mut C) {
    let mut tried = Vec::new();
    loop {
        let declarations = composite.declarations();
        let Some(fact) = compatibility::unprovided(&declarations)
            .find(|&fact| !context.provides_type(fact) && !tried.contains(&fact))
        else {
            return;
        };
        tried.push(fact);
        let mut providers = C::ACCEPTS.iter().flat_map(|acceptor| {
            acceptor
                .axis
                .entries()
                .filter(|entry| {
                    entry.declaration().provides_type(fact)
                        && entry.instance(&Config::new()).is_ok()
                })
                .map(move |entry| (acceptor, entry))
        });
        if let Some((acceptor, entry)) = providers.next()
            && providers.next().is_none()
        {
            (acceptor.add)(composite, entry);
        }
    }
}

/// Every registered plugin of the axis `A` that provides a fact of type `fact`, whatever its
/// configuration.
pub fn providers_of<A: Axis + ?Sized>(fact: TypeId) -> impl Iterator<Item = &'static PluginEntry> {
    A::entries().filter(move |entry| entry.declaration().provides_type(fact))
}

/// The types of the facts the plugins of the axis `A` provide, see [`AnyAxis::facts`].
pub fn facts_of_axis<A: Axis + ?Sized>() -> OpenFacts {
    AnyAxis::of::<A>().facts()
}

/// The catalog itself, tested on an axis of its own so that it does not depend on what the plugins
/// of a program happen to register.
#[cfg(test)]
mod tests {
    use std::{any::Any, iter};

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        axis,
        compatibility::{OpenFacts, declaration_of},
        register,
        schema::{Configurable, Field},
    };

    /// A fact exactly one registered plugin provides unasked.
    #[derive(Debug)]
    struct Settled;

    /// A fact two registered plugins provide unasked.
    #[derive(Debug)]
    struct Ambiguous;

    /// A fact only a plugin that has to be configured provides.
    #[derive(Debug)]
    struct Configured;

    /// A fact a plugin provides unasked and a plugin that has to be configured provides too.
    #[derive(Debug)]
    struct Shared;

    /// A fact no registered plugin provides.
    #[derive(Debug)]
    struct Unknown;

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

    /// A fact a plugin of [`Tested`] and one of [`Nested`] provide unasked.
    #[derive(Debug)]
    struct Crosswise;

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
                .require::<Settled>("the settled fact", None);
        }
    }

    #[register]
    impl Tested for Chained {}

    /// A plugin written by hand rather than derived, registered as a derived one is.
    #[derive(Debug, Serialize, Deserialize)]
    struct Handwritten;

    impl Plugin for Handwritten {}

    impl Configurable for Handwritten {
        // written by hand, the impl has to say what the derive reads from the syntax
        const IS_UNIT: bool = true;

        fn schema() -> Schema {
            Schema::new()
        }
    }

    impl PluginType for Handwritten {
        const ID: &'static str = "handwritten";
        const LABEL: &'static str = "handwritten";
        const DESCRIPTION: &'static str = "a plugin written by hand";
    }

    #[register]
    impl Tested for Handwritten {}

    /// A collection that every plugin of [`Tested`] can be added to.
    #[derive(Debug, Default)]
    struct Plugins(Vec<Box<dyn Tested>>);

    impl Accepts<dyn Tested> for Plugins {
        fn add(&mut self, plugin: Box<dyn Tested>) {
            self.0.push(plugin);
        }
    }

    impl Plugins {
        /// The [`Debug`] forms of the plugins added to it, since a boxed plugin cannot be
        /// downcast.
        fn plugins(&self) -> Vec<String> {
            self.0.iter().map(|plugin| format!("{plugin:?}")).collect()
        }
    }

    /// The composite the plugin providing a fact of the type of `fact` is added to.
    fn with_provider_of(fact: &dyn Any) -> Plugins {
        let mut composite = Plugins::default();
        add_provider_of::<dyn Tested, _>(&mut composite, fact.type_id());
        composite
    }

    /// The entry of the plugin `P`.
    fn entry_of<P: PluginType>() -> &'static PluginEntry {
        <dyn Tested>::entry(P::ID).expect("the plugin is registered")
    }

    #[test]
    fn an_entry_describes_the_plugin_it_registers() {
        let entry = entry_of::<OnlyProvider>();

        assert_eq!(entry.id(), "only_provider");
        assert_eq!(entry.label(), OnlyProvider::LABEL);
        assert_eq!(entry.description(), OnlyProvider::DESCRIPTION);
        assert!(entry.declaration().provides_type(TypeId::of::<Settled>()));
        assert!(entry.schema().fields().is_empty());
    }

    #[test]
    fn a_plugin_is_built_from_the_answers_to_its_schema() {
        let entry = entry_of::<ConfiguredProvider>();
        let configured = |config| PluginConfig::of::<ConfiguredProvider>(config);

        assert_eq!(
            entry.schema().fields().iter().map(Field::name).next(),
            Some("rounds")
        );
        // a built plugin is boxed behind the trait of the axis, so its Debug form is what a test
        // can compare
        let built = configured(Config::new().with("rounds", 5)).build::<dyn Tested>();
        assert_eq!(format!("{built:?}"), "Ok(ConfiguredProvider { rounds: 5 })");
        assert!(configured(Config::new()).build::<dyn Tested>().is_err());
    }

    #[test]
    fn a_plugin_of_another_axis_is_not_built() {
        assert_eq!(
            PluginConfig::of::<DeepProvider>(Config::new())
                .build::<dyn Tested>()
                .err(),
            Some(ConfigError::UnknownPlugin { path: Path::ROOT })
        );
    }

    #[test]
    fn an_axis_as_a_value_finds_the_entries_of_the_axis() {
        let axis = AnyAxis::of::<dyn Tested>();

        assert_eq!(axis.entries().count(), <dyn Tested>::entries().count());
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
        let wrapping =
            Config::new().with("nested", PluginConfig::of::<DeepProvider>(Config::new()));
        let deep = TypeId::of::<Deep>();

        assert!(!entry.declaration().provides_type(deep));
        assert!(
            !entry
                .declaration_for(&Config::new())
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
        assert!(<dyn Tested>::entry("unregistered").is_none());
    }

    #[test]
    fn a_registered_plugin_is_found_by_the_fact_it_provides() {
        assert_eq!(
            with_provider_of(&Settled).plugins(),
            [format!("{OnlyProvider:?}")]
        );
    }

    #[test]
    fn a_fact_several_registered_plugins_provide_is_left_to_be_configured() {
        assert!(with_provider_of(&Ambiguous).plugins().is_empty());
    }

    #[test]
    fn a_fact_no_registered_plugin_provides_adds_nothing() {
        assert!(with_provider_of(&Unknown).plugins().is_empty());
    }

    #[test]
    fn a_plugin_that_has_to_be_configured_is_not_added() {
        assert!(with_provider_of(&Configured).plugins().is_empty());
    }

    #[test]
    fn a_plugin_that_has_to_be_configured_does_not_make_a_fact_ambiguous() {
        assert_eq!(
            with_provider_of(&Shared).plugins(),
            [format!("{OnlyProvider:?}")]
        );
    }

    #[test]
    fn a_fact_is_provided_by_every_plugin_declaring_it() {
        let providers: Vec<_> = providers_of::<dyn Tested>(TypeId::of::<Shared>())
            .map(PluginEntry::label)
            .collect();

        assert_eq!(providers, ["configured provider", "only provider"]);
    }

    #[test]
    fn the_plugins_of_an_axis_come_in_the_order_of_their_ids() {
        let ids: Vec<_> = <dyn Tested>::entries().map(PluginEntry::id).collect();

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
        assert!(
            AnyAxis::of::<dyn Tested>()
                .entries()
                .map(PluginEntry::id)
                .eq(ids)
        );
    }

    #[test]
    fn a_plugin_written_by_hand_is_built_by_its_id() {
        let built = PluginConfig::of::<Handwritten>(Config::new()).build::<dyn Tested>();

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
        assert_eq!(facts_of_axis::<dyn Tested>(), facts);
        assert_eq!(facts_of_axis::<dyn Nested>(), facts);
    }

    /// A composite accepting plugins of both axes, whose own requirements are `requires`.
    struct Resolved {
        requires: fn(&mut Declaration),
        tested: Vec<Box<dyn Tested>>,
        nested: Vec<Box<dyn Nested>>,
    }

    impl Accepts<dyn Tested> for Resolved {
        fn add(&mut self, plugin: Box<dyn Tested>) {
            self.tested.push(plugin);
        }
    }

    impl Accepts<dyn Nested> for Resolved {
        fn add(&mut self, plugin: Box<dyn Nested>) {
            self.nested.push(plugin);
        }
    }

    impl Composite for Resolved {
        const ACCEPTS: &'static [Acceptor<Self>] =
            &[Acceptor::of::<dyn Tested>(), Acceptor::of::<dyn Nested>()];

        fn declarations(&self) -> Vec<(String, Declaration)> {
            iter::once(("requirements".to_owned(), Declaration::of(self.requires)))
                .chain(self.tested.iter().map(|plugin| declaration_of(&**plugin)))
                .chain(self.nested.iter().map(|plugin| declaration_of(&**plugin)))
                .collect()
        }
    }

    /// The [`Debug`] forms of the plugins [`resolve`] adds to a composite requiring `requires`
    /// with `context`, those of [`Tested`] first.
    fn resolved(context: &Declaration, requires: fn(&mut Declaration)) -> Vec<String> {
        let mut composite = Resolved {
            requires,
            tested: Vec::new(),
            nested: Vec::new(),
        };
        resolve(context, &mut composite);
        let tested = composite.tested.iter().map(|plugin| format!("{plugin:?}"));
        let nested = composite.nested.iter().map(|plugin| format!("{plugin:?}"));
        tested.chain(nested).collect()
    }

    #[test]
    fn resolving_adds_the_providers_of_what_the_added_plugins_require() {
        assert_eq!(
            resolved(&Declaration::new(), |declaration| {
                declaration.require::<Downstream>("the downstream fact", None);
            }),
            [format!("{Chained:?}"), format!("{OnlyProvider:?}")]
        );
    }

    #[test]
    fn resolving_adds_a_provider_of_any_accepted_axis() {
        assert_eq!(
            resolved(&Declaration::new(), |declaration| {
                declaration.require::<Deep>("the deep fact", None);
            }),
            [format!("{:?}", DeepProvider { inner: None })]
        );
    }

    #[test]
    fn resolving_leaves_a_fact_provided_by_plugins_of_two_axes_unprovided() {
        assert!(
            resolved(&Declaration::new(), |declaration| {
                declaration.require::<Crosswise>("the crosswise fact", None);
            })
            .is_empty()
        );
    }

    #[test]
    fn resolving_adds_nothing_for_a_fact_the_context_provides() {
        let context = Declaration::of(|declaration| {
            declaration.provide(Settled);
        });

        assert!(
            resolved(&context, |declaration| {
                declaration.require::<Settled>("the settled fact", None);
            })
            .is_empty()
        );
    }

    #[test]
    fn resolving_leaves_an_ambiguous_fact_unprovided_and_settles_the_others() {
        assert_eq!(
            resolved(&Declaration::new(), |declaration| {
                declaration
                    .require::<Ambiguous>("the ambiguous fact", None)
                    .require::<Settled>("the settled fact", None);
            }),
            [format!("{OnlyProvider:?}")]
        );
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
        <dyn Clashing>::entry("clash");
    }

    #[test]
    fn the_panic_on_a_shared_id_names_both_plugins() {
        let message = std::panic::catch_unwind(|| <dyn Clashing>::entries().count())
            .unwrap_err()
            .downcast::<String>()
            .unwrap();

        assert!(message.contains(any::type_name::<First>()));
        assert!(message.contains(any::type_name::<Second>()));
    }
}
