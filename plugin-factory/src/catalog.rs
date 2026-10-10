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

use crate::{Plugin, compatibility::Declaration};

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
    fn entries() -> impl Iterator<Item = &'static PluginEntry>;
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
        $visibility struct $kind($crate::catalog::PluginEntry);

        impl $kind {
            /// Registers the plugin `id` of this axis, shown as `label` and described by
            /// `description`; a plugin that needs no configuration adds `with_default`.
            ///
            /// See `PluginEntry::new` for what the three strings are for.
            pub fn register<P: $crate::Plugin>() -> Self {
                Self($crate::catalog::PluginEntry::new::<P>())
            }
        }

        $crate::inventory::collect!($kind);

        impl $crate::catalog::PluginKind for $kind {
            type Object = $object;

            fn entries() -> impl ::core::iter::Iterator<
                Item = &'static $crate::catalog::PluginEntry,
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
pub struct PluginEntry {
    id: TypeId,
    label: &'static str,
    description: &'static str,
    declaration: Declaration,
}

impl PluginEntry {
    /// The entry of the plugin `id`, shown as `label` and described by `description`, which has to
    /// be configured in order to be created.
    ///
    /// `id` identifies the plugin for good and has to be unique over every plugin of the axis, so
    /// namespace it with the crate it lives in. `label` and `description` are what an operator
    /// picking a plugin reads.
    pub fn new<P: Plugin>() -> Self {
        let declaration = {
            let mut declaration = Declaration::new();
            P::static_declare(&mut declaration);
            declaration
        };
        Self {
            id: TypeId::of::<P>(),
            label: P::label(),
            description: P::description(),
            declaration,
        }
    }

    pub fn id(&self) -> TypeId {
        self.id
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn description(&self) -> &'static str {
        self.description
    }

    pub fn declaration(&self) -> &Declaration {
        &self.declaration
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

/// Every registered plugin of kind `K` that provides a fact of type `fact` with its default
/// configuration.
fn providers_of<K: PluginKind>(fact: TypeId) -> impl Iterator<Item = &'static PluginEntry> {
    K::entries().filter(move |entry| entry.declaration().provides_type(fact))
}
