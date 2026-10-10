//! Every plugin a program has linked in, so that it can be found without being named.
//!
//! A plugin axis is a [`PluginKind`], declared with [`plugin_kind!`](crate::plugin_kind) next to
//! the trait of the axis. A plugin registers itself for its axis with one [`PluginEntry`], which
//! says what to call it and, unless it has to be configured, how to create it. Registration is
//! open: a plugin of a third-party crate ends up in the same catalog as the ones here.
//!
//! The engine reads the catalog to settle a fact a phase requires but none of its plugins
//! provides, see [`add_provider_of`].

use std::any::TypeId;

use itertools::Itertools;

use crate::plugins::{Plugin, compatibility::Declaration};

/// One axis of plugins, i.e. a plugin trait together with the registry the plugins of that trait
/// register in.
///
/// Implemented for the type that [`plugin_kind!`](crate::plugin_kind) declares, which is also the
/// only way to get the registry.
pub trait PluginKind: Sized + 'static {
    /// The plugin trait of the axis, e.g. `dyn MetricsCalculator`. Every axis trait has [`Plugin`]
    /// as a supertrait, so that the engine can ask any plugin what it declares.
    type Object: ?Sized + Plugin + 'static;

    /// Every plugin registered for this axis, in no particular order.
    fn entries() -> impl Iterator<Item = &'static PluginEntry<Self>>;
}

/// Declares the plugin axis of the trait `$object`: the type `$kind` implementing [`PluginKind`],
/// which is at the same time the registry of the axis — one value of it is one registered plugin,
/// built by `$kind::register` and submitted with `inventory::submit!`.
///
/// Generating that type is what the macro exists for: `inventory` collects by type, and only a
/// type of the crate declaring the collection can be collected, so an axis brings its own. It is
/// also where the trait of the axis is written out, so that registering a plugin takes no turbofish
/// and no import besides the axis itself.
///
/// # Examples
/// ```
/// use tournament_engine::{
///     inventory, plugin_kind,
///     plugins::{Plugin, compatibility::Declaration},
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
/// #[derive(Debug, Clone)]
/// struct ByRank;
///
/// impl Plugin for ByRank {
///     fn declare(&self, _: &mut Declaration) {}
/// }
///
/// impl Seeding for ByRank {
///     fn seed(&self) {}
/// }
///
/// inventory::submit! {
///     SeedingKind::register("my_crate::by_rank", "by rank", "the best entrant is seeded first")
///         .with_default(|| Box::new(ByRank))
/// }
/// # fn main() {}
/// ```
#[macro_export]
macro_rules! plugin_kind {
    ($(#[$attribute:meta])* $visibility:vis $kind:ident for $object:ty) => {
        $(#[$attribute])*
        $visibility struct $kind($crate::plugins::catalog::PluginEntry<$kind>);

        impl $kind {
            /// Registers the plugin `id` of this axis, shown as `label` and described by
            /// `description`; a plugin that needs no configuration adds `with_default`.
            ///
            /// See `PluginEntry::new` for what the three strings are for.
            pub const fn register(
                id: &'static str,
                label: &'static str,
                description: &'static str,
            ) -> Self {
                Self($crate::plugins::catalog::PluginEntry::new(id, label, description))
            }

            /// This registration for a plugin that `default` creates with its default
            /// configuration, which is what lets the engine create it unasked.
            pub const fn with_default(
                self,
                default: fn() -> ::std::boxed::Box<$object>,
            ) -> Self {
                Self(self.0.with_default(default))
            }
        }

        $crate::inventory::collect!($kind);

        impl $crate::plugins::catalog::PluginKind for $kind {
            type Object = $object;

            fn entries() -> impl ::core::iter::Iterator<
                Item = &'static $crate::plugins::catalog::PluginEntry<Self>,
            > {
                ::core::iter::IntoIterator::into_iter($crate::inventory::iter::<Self>)
                    .map(|registered| &registered.0)
            }
        }
    };
}

/// One plugin of the axis `K`, as it is registered in the catalog.
///
/// Created through the axis itself, see [`plugin_kind!`](crate::plugin_kind):
/// ```
/// use tournament_engine::{
///     inventory,
///     plugins::metrics_calculator::{MetricsCalculatorKind, matches::MatchesCalculator},
/// };
///
/// inventory::submit! {
///     MetricsCalculatorKind::register(
///         "my_crate::matches",
///         "match list",
///         "the matches an entrant played",
///     )
///     .with_default(|| Box::new(MatchesCalculator))
/// }
/// # fn main() {}
/// ```
pub struct PluginEntry<K: PluginKind> {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    default: Option<fn() -> Box<K::Object>>,
}

impl<K: PluginKind> PluginEntry<K> {
    /// The entry of the plugin `id`, shown as `label` and described by `description`, which has to
    /// be configured in order to be created.
    ///
    /// `id` identifies the plugin for good and has to be unique over every plugin of the axis, so
    /// namespace it with the crate it lives in. `label` and `description` are what an operator
    /// picking a plugin reads.
    pub const fn new(id: &'static str, label: &'static str, description: &'static str) -> Self {
        Self {
            id,
            label,
            description,
            default: None,
        }
    }

    /// This entry for a plugin that `default` creates with its default configuration, which is
    /// what lets the engine create it unasked, see [`add_provider_of`].
    pub const fn with_default(mut self, default: fn() -> Box<K::Object>) -> Self {
        self.default = Some(default);
        self
    }

    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn description(&self) -> &'static str {
        self.description
    }

    /// The plugin with its default configuration, or [`None`] if it has to be configured.
    pub fn create_default(&self) -> Option<Box<K::Object>> {
        Some(self.default?())
    }
}

/// A composite of plugins the engine may add a plugin of kind `K` to, see [`add_provider_of`].
///
/// A composite with a single slot for a kind — a phase is played in one format — leaves the impl
/// for that kind out, so that nothing can be added to it.
pub trait Accepts<K: PluginKind> {
    /// Adds `plugin` to the composite.
    fn add(&mut self, plugin: Box<K::Object>);
}

/// Adds a plugin providing a fact of type `fact` to `composite`, if exactly one plugin of kind `K`
/// is registered that provides it with its default configuration.
///
/// Adds nothing if none does, or if several do: which of them to use is then not the engine's
/// choice to make, and the fact stays unprovided.
pub fn add_provider_of<K: PluginKind, C: Accepts<K> + ?Sized>(composite: &mut C, fact: TypeId) {
    if let Ok(plugin) = providers_of::<K>(fact).exactly_one() {
        composite.add(plugin);
    }
}

/// Every registered plugin of kind `K` that provides a fact of type `fact` with its default
/// configuration.
fn providers_of<K: PluginKind>(fact: TypeId) -> impl Iterator<Item = Box<K::Object>> {
    K::entries().filter_map(move |entry| {
        let plugin = entry.create_default()?;
        let declaration = Declaration::of(|declaration| plugin.declare(declaration));
        declaration.provides_type(fact).then_some(plugin)
    })
}

/// The catalog itself, tested on an axis of its own so that it does not depend on which plugins
/// this crate happens to register. What they register is tested in [`super::tests`].
#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;

    /// A fact exactly one registered plugin provides.
    #[derive(Debug)]
    struct Settled;

    /// A fact two registered plugins provide.
    #[derive(Debug)]
    struct Ambiguous;

    /// A fact no registered plugin provides.
    #[derive(Debug)]
    struct Unknown;

    /// A plugin axis with no behavior at all, so that a test plugin is its declaration.
    trait Tested: Plugin {}

    plugin_kind! {
        /// The axis of [`Tested`].
        TestedKind for dyn Tested
    }

    /// The only plugin providing [`Settled`].
    #[derive(Debug, Clone)]
    struct OnlyProvider;

    impl Plugin for OnlyProvider {
        fn declare(&self, declaration: &mut Declaration) {
            declaration.provides(Settled);
        }
    }

    impl Tested for OnlyProvider {}

    inventory::submit! {
        TestedKind::register(
            "tournament_engine::catalog::tests::only_provider",
            "only provider",
            "the one plugin providing the settled fact",
        )
        .with_default(|| Box::new(OnlyProvider))
    }

    /// One of the two plugins providing [`Ambiguous`].
    #[derive(Debug, Clone)]
    struct OneProvider;

    impl Plugin for OneProvider {
        fn declare(&self, declaration: &mut Declaration) {
            declaration.provides(Ambiguous);
        }
    }

    impl Tested for OneProvider {}

    inventory::submit! {
        TestedKind::register(
            "tournament_engine::catalog::tests::one_provider",
            "one provider",
            "a plugin providing the ambiguous fact",
        )
        .with_default(|| Box::new(OneProvider))
    }

    /// The other plugin providing [`Ambiguous`].
    #[derive(Debug, Clone)]
    struct AnotherProvider;

    impl Plugin for AnotherProvider {
        fn declare(&self, declaration: &mut Declaration) {
            declaration.provides(Ambiguous);
        }
    }

    impl Tested for AnotherProvider {}

    inventory::submit! {
        TestedKind::register(
            "tournament_engine::catalog::tests::another_provider",
            "another provider",
            "the other plugin providing the ambiguous fact",
        )
        .with_default(|| Box::new(AnotherProvider))
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
    fn every_plugin_of_the_axis_is_registered_once() {
        let ids: Vec<_> = TestedKind::entries().map(PluginEntry::id).collect();

        assert_eq!(ids.len(), 3, "got {ids:?}");
        assert!(ids.iter().all_unique(), "got {ids:?}");
    }
}
