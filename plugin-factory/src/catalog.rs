//! Every plugin a program has linked in, so that it can be found without being named.
//!
//! A plugin axis is a [`PluginKind`], declared with [`plugin_kind!`](crate::plugin_kind) next to
//! the trait of the axis. A plugin registers itself for its axis with one [`PluginEntry`], which
//! describes it, states what it has to be told and builds it from the answers. Registration is
//! open: a plugin of a third-party crate ends up in the same catalog as the ones of the crate
//! defining the axis.
//!
//! A program reads the catalog to settle a fact a composite requires but none of its plugins
//! provides, see [`add_provider_of`], and to offer the plugins of a kind to choose from.

use std::any::TypeId;

use crate::{
    Plugin,
    compatibility::{Declaration, ProvidedFact},
    config::{Config, ConfigError},
    schema::{Configurable, Schema},
};

/// One axis of plugins, i.e. a plugin trait together with the registry the plugins of that trait
/// register in.
///
/// Implemented for the type that [`plugin_kind!`](crate::plugin_kind) declares, which is also the
/// only way to get the registry.
pub trait PluginKind: Sized + 'static {
    /// The plugin trait of the axis, e.g. `dyn MetricsCalculator`. Every axis trait has [`Plugin`]
    /// as a supertrait, so that any plugin can be asked what it declares.
    type Object: ?Sized + Plugin;

    /// Every plugin registered for this axis, in no particular order.
    fn entries() -> impl Iterator<Item = &'static PluginEntry<Self>>;
}

/// Declares the plugin axis of the trait `$object`: the type `$kind` implementing [`PluginKind`],
/// which is at the same time the registry of the axis — one value of it is one registered plugin,
/// built by `$kind::register` and submitted with `inventory::submit!`.
///
/// Generating that type is what the macro exists for: `inventory` collects by type, and only a
/// type of the crate declaring the collection can be collected, so an axis brings its own. It is
/// also where the trait of the axis is written out, so that registering a plugin takes no
/// turbofish and no import besides the axis itself.
///
/// # Examples
/// ```
/// use plugin_factory::{
///     Plugin,
///     config::{Config, ConfigError},
///     inventory, plugin_kind,
///     schema::{Configurable, Schema},
/// };
///
/// /// How the entrants of a phase are seeded, an axis of a crate building on the engine.
/// pub trait Seeding: Plugin {
///     fn seed(&self);
/// }
///
/// plugin_kind! {
///     /// The seedings, i.e. the axis of [`Seeding`].
///     pub SeedingKind for dyn Seeding
/// }
///
/// #[derive(Debug)]
/// struct ByRank;
///
/// impl Plugin for ByRank {
///     fn label() -> &'static str {
///         "by rank"
///     }
///
///     fn description() -> &'static str {
///         "the best entrant is seeded first"
///     }
/// }
///
/// impl Configurable for ByRank {
///     fn schema() -> Schema {
///         Schema::new()
///     }
///
///     fn build(_: &Config) -> Result<Self, ConfigError> {
///         Ok(Self)
///     }
/// }
///
/// impl Seeding for ByRank {
///     fn seed(&self) {}
/// }
///
/// inventory::submit! {
///     SeedingKind::register::<ByRank>()
/// }
/// # fn main() {}
/// ```
#[macro_export]
macro_rules! plugin_kind {
    ($(#[$attribute:meta])* $visibility:vis $kind:ident for dyn $object:path) => {
        $(#[$attribute])*
        $visibility struct $kind($crate::catalog::PluginEntry<$kind>);

        impl $kind {
            /// Registers the plugin `P` for this axis, which is configured by answering the
            /// schema of its `Configurable` impl.
            pub const fn register<P>() -> Self
            where
                P: $crate::schema::Configurable + $object,
            {
                Self($crate::catalog::PluginEntry::new::<P>(Self::build::<P>))
            }

            /// Builds the plugin `P` as a plugin of this axis, which is what the entry of `P`
            /// holds.
            // the entry holds a plain function pointer, so coercing `P` to the trait object of
            // the axis needs a named generic function to point at
            #[doc(hidden)]
            pub fn build<P>(
                config: &$crate::config::Config,
            ) -> ::core::result::Result<
                ::std::boxed::Box<dyn $object>,
                $crate::config::ConfigError,
            >
            where
                P: $crate::schema::Configurable + $object,
            {
                ::core::result::Result::Ok(::std::boxed::Box::new(P::build(config)?))
            }
        }

        $crate::inventory::collect!($kind);

        impl $crate::catalog::PluginKind for $kind {
            type Object = dyn $object;

            fn entries() -> impl ::core::iter::Iterator<
                Item = &'static $crate::catalog::PluginEntry<Self>,
            > {
                ::core::iter::IntoIterator::into_iter($crate::inventory::iter::<Self>)
                    .map(|registered| &registered.0)
            }
        }
    };
}

/// One plugin of the axis `K`, as it is registered in the catalog.
///
/// Everything in it comes from the plugin's type, so registering one takes no strings; created
/// through the axis itself, see [`plugin_kind!`](crate::plugin_kind).
pub struct PluginEntry<K: PluginKind> {
    id: TypeId,
    label: fn() -> &'static str,
    description: fn() -> &'static str,
    declare: fn(&mut Declaration),
    schema: fn() -> Schema,
    build: fn(&Config) -> Result<Box<K::Object>, ConfigError>,
}

impl<K: PluginKind> PluginEntry<K> {
    /// The entry of the plugin `P`, built from the answers to its schema by `build`.
    pub const fn new<P: Plugin + Configurable>(
        build: fn(&Config) -> Result<Box<K::Object>, ConfigError>,
    ) -> Self {
        Self {
            id: TypeId::of::<P>(),
            label: P::label,
            description: P::description,
            declare: P::static_declare,
            schema: P::schema,
            build,
        }
    }

    /// What identifies the plugin, i.e. the type of the plugin this is the entry of.
    pub fn id(&self) -> TypeId {
        self.id
    }

    /// What an operator picking a plugin reads.
    pub fn label(&self) -> &'static str {
        (self.label)()
    }

    /// What an operator picking a plugin reads about what it does.
    pub fn description(&self) -> &'static str {
        (self.description)()
    }

    /// What the plugin provides and requires whatever its configuration, see
    /// [`Plugin::static_declare`]. Built on every call.
    pub fn declaration(&self) -> Declaration {
        Declaration::of(self.declare)
    }

    /// What has to be answered in order to create the plugin, see
    /// [`Configurable::schema`]. Built on every call.
    pub fn schema(&self) -> Schema {
        (self.schema)()
    }

    /// The plugin as `config` configures it, where every field of its schema that `config` leaves
    /// unanswered is answered by the default of the field.
    ///
    /// A plugin every field of whose schema has a default needs no configuration, so that
    /// [`Config::new`] creates it, see [`add_provider_of`].
    ///
    /// # Errors
    /// Returns [`ConfigError::Unanswered`] if a field without a default is unanswered, and
    /// whatever completing `config` or building the plugin from it fails with otherwise, see
    /// [`Schema::complete`].
    pub fn build(&self, config: &Config) -> Result<Box<K::Object>, ConfigError> {
        (self.build)(&self.schema().complete(config)?)
    }
}

/// A composite of plugins that a plugin of kind `K` may be added to, see [`add_provider_of`].
///
/// A composite with a single slot for a kind — a phase is played in one format — leaves the impl
/// for that kind out, so that nothing can be added to it.
pub trait Accepts<K: PluginKind> {
    /// Adds `plugin` to the composite.
    fn add(&mut self, plugin: Box<K::Object>);
}

/// Adds a plugin providing a fact of type `fact` to `composite`, if exactly one plugin of kind `K`
/// is registered that provides it and needs no configuration.
///
/// Adds nothing if none does, or if several do: which of them to use is then not a program's
/// choice to make, and the fact stays unprovided.
pub fn add_provider_of<K: PluginKind, C: Accepts<K> + ?Sized>(composite: &mut C, fact: TypeId) {
    let mut providers =
        providers_of::<K>(fact).filter_map(|entry| entry.build(&Config::new()).ok());
    if let Some(plugin) = providers.next()
        && providers.next().is_none()
    {
        composite.add(plugin);
    }
}

/// Every registered plugin of kind `K` that provides a fact of type `fact`, whatever its
/// configuration.
pub fn providers_of<K: PluginKind>(fact: TypeId) -> impl Iterator<Item = &'static PluginEntry<K>> {
    K::entries().filter(move |entry| entry.declaration().provides_type(fact))
}

/// The types of the facts the registered plugins of kind `K` provide, i.e. what a slot for that
/// kind could contribute while it is still to be filled, see
/// [`OpenFacts`](crate::compatibility::OpenFacts).
pub fn facts_of_kind<K: PluginKind>() -> impl Iterator<Item = TypeId> {
    K::entries().flat_map(|entry| {
        entry
            .declaration()
            .provided()
            .iter()
            .map(ProvidedFact::type_id)
            // collected because the declaration is built per call and the ids outlive it
            .collect::<Vec<_>>()
    })
}

/// The catalog itself, tested on an axis of its own so that it does not depend on what the plugins
/// of a program happen to register.
#[cfg(test)]
mod tests {
    use std::{any::Any, fmt::Debug};

    use super::*;
    use crate::{compatibility::OpenFacts, schema::Field};

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

    /// A plugin axis with no behavior at all, so that a test plugin is its declaration. [`Debug`]
    /// is what names a plugin in an error message, so an axis asks for it.
    trait Tested: Plugin + Debug {}

    plugin_kind! {
        /// The axis of [`Tested`].
        TestedKind for dyn Tested
    }

    /// The only plugin providing [`Settled`], and one of the two providing [`Shared`].
    #[derive(Debug)]
    struct OnlyProvider;

    impl Plugin for OnlyProvider {
        fn label() -> &'static str {
            "only provider"
        }

        fn description() -> &'static str {
            "the one plugin providing the settled fact"
        }

        fn static_declare(declaration: &mut Declaration) {
            declaration.provide(Settled).provide(Shared);
        }
    }

    impl Configurable for OnlyProvider {
        fn schema() -> Schema {
            Schema::new()
        }

        fn build(_: &Config) -> Result<Self, ConfigError> {
            Ok(Self)
        }
    }

    impl Tested for OnlyProvider {}

    inventory::submit! {
        TestedKind::register::<OnlyProvider>()
    }

    /// One of the two plugins providing [`Ambiguous`].
    #[derive(Debug)]
    struct OneProvider;

    impl Plugin for OneProvider {
        fn label() -> &'static str {
            "one provider"
        }

        fn description() -> &'static str {
            "a plugin providing the ambiguous fact"
        }

        fn static_declare(declaration: &mut Declaration) {
            declaration.provide(Ambiguous);
        }
    }

    impl Configurable for OneProvider {
        fn schema() -> Schema {
            Schema::new()
        }

        fn build(_: &Config) -> Result<Self, ConfigError> {
            Ok(Self)
        }
    }

    impl Tested for OneProvider {}

    inventory::submit! {
        TestedKind::register::<OneProvider>()
    }

    /// The other plugin providing [`Ambiguous`].
    #[derive(Debug)]
    struct AnotherProvider;

    impl Plugin for AnotherProvider {
        fn label() -> &'static str {
            "another provider"
        }

        fn description() -> &'static str {
            "the other plugin providing the ambiguous fact"
        }

        fn static_declare(declaration: &mut Declaration) {
            declaration.provide(Ambiguous);
        }
    }

    impl Configurable for AnotherProvider {
        fn schema() -> Schema {
            Schema::new()
        }

        fn build(_: &Config) -> Result<Self, ConfigError> {
            Ok(Self)
        }
    }

    impl Tested for AnotherProvider {}

    inventory::submit! {
        TestedKind::register::<AnotherProvider>()
    }

    /// A plugin with a field without a default, so that the catalog cannot create it. The answer
    /// is only read in its [`Debug`] form, since a boxed plugin cannot be downcast.
    #[derive(Debug)]
    struct ConfiguredProvider(#[expect(dead_code, reason = "read as part of the Debug form")] i64);

    impl Plugin for ConfiguredProvider {
        fn label() -> &'static str {
            "configured provider"
        }

        fn description() -> &'static str {
            "a plugin that has to be configured"
        }

        fn static_declare(declaration: &mut Declaration) {
            declaration.provide(Configured).provide(Shared);
        }
    }

    impl Configurable for ConfiguredProvider {
        fn schema() -> Schema {
            Schema::new().field(Field::integer("rounds", "Rounds"))
        }

        fn build(config: &Config) -> Result<Self, ConfigError> {
            Ok(Self(config.read("rounds")?))
        }
    }

    impl Tested for ConfiguredProvider {}

    inventory::submit! {
        TestedKind::register::<ConfiguredProvider>()
    }

    /// A composite that every plugin of [`TestedKind`] can be added to.
    #[derive(Debug, Default)]
    struct Composite(Vec<Box<dyn Tested>>);

    impl Accepts<TestedKind> for Composite {
        fn add(&mut self, plugin: Box<dyn Tested>) {
            self.0.push(plugin);
        }
    }

    impl Composite {
        /// The [`Debug`] forms of the plugins added to it, since a boxed plugin cannot be
        /// downcast.
        fn plugins(&self) -> Vec<String> {
            self.0.iter().map(|plugin| format!("{plugin:?}")).collect()
        }
    }

    /// The composite the plugin providing a fact of the type of `fact` is added to.
    fn with_provider_of(fact: &dyn Any) -> Composite {
        let mut composite = Composite::default();
        add_provider_of::<TestedKind, _>(&mut composite, fact.type_id());
        composite
    }

    /// The entry of the plugin `P`.
    fn entry_of<P: Plugin>() -> &'static PluginEntry<TestedKind> {
        TestedKind::entries()
            .find(|entry| entry.id() == TypeId::of::<P>())
            .expect("the plugin is registered")
    }

    #[test]
    fn an_entry_describes_the_plugin_it_registers() {
        let entry = entry_of::<OnlyProvider>();

        assert_eq!(entry.label(), OnlyProvider::label());
        assert_eq!(entry.description(), OnlyProvider::description());
        assert!(entry.declaration().provides_type(TypeId::of::<Settled>()));
        assert!(entry.schema().fields().is_empty());
    }

    #[test]
    fn an_entry_builds_its_plugin_from_the_answers_to_its_schema() {
        let entry = entry_of::<ConfiguredProvider>();

        assert_eq!(
            entry.schema().fields().iter().map(Field::name).next(),
            Some("rounds")
        );
        // a built plugin is boxed behind the trait of the axis, so its Debug form is what a test
        // can compare
        let built = entry.build(&Config::new().with("rounds", 5i64));
        assert_eq!(format!("{built:?}"), "Ok(ConfiguredProvider(5))");
        assert!(entry.build(&Config::new()).is_err());
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
        let mut providers: Vec<_> = providers_of::<TestedKind>(TypeId::of::<Shared>())
            .map(PluginEntry::label)
            .collect();
        // the entries of a kind come in no particular order
        providers.sort_unstable();

        assert_eq!(providers, ["configured provider", "only provider"]);
    }

    #[test]
    fn every_plugin_of_the_axis_is_registered_once() {
        let mut ids: Vec<_> = TestedKind::entries().map(PluginEntry::id).collect();
        let registered = ids.len();
        ids.sort();
        ids.dedup();

        assert_eq!(registered, 4);
        assert_eq!(ids.len(), registered);
    }

    #[test]
    fn the_facts_of_a_kind_are_what_its_plugins_provide() {
        assert_eq!(
            facts_of_kind::<TestedKind>().collect::<OpenFacts>(),
            OpenFacts::from_iter([
                TypeId::of::<Settled>(),
                TypeId::of::<Ambiguous>(),
                TypeId::of::<Configured>(),
                TypeId::of::<Shared>(),
            ])
        );
    }
}
