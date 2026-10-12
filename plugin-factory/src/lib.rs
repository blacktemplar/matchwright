//! Plugins that describe themselves, so that a program configuring them never names one.
//!
//! A plugin, see [`PluginType`], says what to call it, what it has to be told in order to be
//! built, see [`schema`] and [`config`], and what it provides to and requires of the composite it
//! is part of, see [`compatibility`]. It registers itself in the [`catalog`], which is where a
//! program looking for a plugin finds it. A [`Builder`](builder::Builder) holds a composite of
//! plugins while it is being configured.
//!
//! An axis of plugins is a trait declared with [`axis`]. A plugin is usually derived, which reads
//! its schema from its fields and their `serde` attributes, and registered with [`register`],
//! which tags it for `typetag` with its id. A composite derives its schema the same way, see
//! [`derive@schema::Configurable`]:
//!
//! ```
//! use plugin_factory::{Plugin, axis, register, schema::Configurable};
//! use serde::{Deserialize, Serialize};
//!
//! #[axis]
//! pub trait Seeding: Plugin {}
//!
//! #[derive(Debug, Serialize, Deserialize, Plugin)]
//! #[plugin(
//!     id = "my_crate::random",
//!     label = "random",
//!     description = "entrants are shuffled"
//! )]
//! struct Random {
//!     #[serde(default)]
//!     #[plugin(label = "Keep the top seeds")]
//!     keep_top: u32,
//! }
//!
//! #[register]
//! impl Seeding for Random {}
//!
//! #[derive(Deserialize, Configurable)]
//! struct Phase {
//!     #[plugin(label = "Seeding")]
//!     seeding: Box<dyn Seeding>,
//! }
//! # fn main() {
//! # use plugin_factory::catalog::Axis;
//! # assert_eq!(<dyn Seeding>::entry("my_crate::random").unwrap().label(), "random");
//! # assert_eq!(Phase::schema().fields()[0].name(), "seeding");
//! # }
//! ```

use std::fmt::Debug;

use crate::{compatibility::Declaration, config::PluginConfig, schema::Configurable};

// so that the code the derive generates for the crate's own tests names it as any other crate does
extern crate self as plugin_factory;

pub mod builder;
pub mod catalog;
pub mod compatibility;
pub mod config;
pub mod schema;

/// Re-exported so that a crate registering a plugin in the [catalog] does not have to depend on
/// `inventory` itself.
pub use inventory;
pub use plugin_factory_derive::{Plugin, axis, register};

/// What the code [`derive@Plugin`], [`axis`] and [`register`] generate refers to, and what a
/// plugin behind a `dyn` is asked through. Not part of the API.
#[doc(hidden)]
pub mod __private {
    use std::{any::Any, fmt::Debug};

    pub use serde;
    use serde_json::value::Serializer;

    use crate::{
        Plugin, PluginType, catalog::Axis, compatibility::Declaration, config::Value,
        schema::Configurable,
    };

    /// A fact, see [`compatibility`](crate::compatibility): a value of any `'static + Debug +
    /// Sync` type, identified by its type.
    pub trait Fact: Any + Debug + Sync {}

    impl<T: Any + Debug + Sync> Fact for T {}

    /// How the plugin `P` is registered for an axis: the value of the axis's registry that
    /// [`register`](crate::register) submits. Implemented by [`axis`](crate::axis) for every
    /// plugin of the axis.
    pub trait Register<P>: Axis {
        /// The registry of the axis, a type of the crate declaring it.
        type Registry: 'static;

        /// The entry of `P`.
        const REGISTRY: Self::Registry;
    }

    /// What only the type of a plugin knows, asked of a plugin behind a `dyn`. Implemented for
    /// every [`PluginType`].
    pub trait Erased {
        /// The plugin's [`PluginType::ID`].
        fn id(&self) -> &'static str;

        /// The plugin as a plugin of no particular axis.
        fn as_plugin(&self) -> &dyn Plugin;

        /// Adds everything the plugin declares to `declaration`: its static declaration, what its
        /// configuration declares, and then what its sub-plugins declare, recursively.
        fn declare_all(&self, declaration: &mut Declaration);

        /// The plugins held by its fields, see [`Configurable::plugins`].
        fn sub_plugins(&self) -> Vec<&dyn Plugin>;
    }

    impl<T: PluginType> Erased for T {
        fn id(&self) -> &'static str {
            T::ID
        }

        fn as_plugin(&self) -> &dyn Plugin {
            self
        }

        // qualified throughout, since a plugin may have inherent functions of the same names
        fn declare_all(&self, declaration: &mut Declaration) {
            <T as PluginType>::declare_static(declaration);
            Plugin::declare(self, declaration);
            for sub_plugin in Configurable::plugins(self) {
                sub_plugin.declare_all(declaration);
            }
        }

        fn sub_plugins(&self) -> Vec<&dyn Plugin> {
            Configurable::plugins(self)
        }
    }

    /// The default `value` of a field, written by `serialize` as the field is serialized.
    ///
    /// # Panics
    /// Panics if `value` cannot be written as JSON.
    pub fn default_value<T>(
        value: &T,
        serialize: impl FnOnce(&T, Serializer) -> Result<Value, serde_json::Error>,
    ) -> Value {
        serialize(value, Serializer)
            .unwrap_or_else(|error| panic!("the default of a field cannot be written: {error}"))
    }
}

/// A plugin of some axis as it is asked behind a `dyn`, see [`catalog`]: what its configuration
/// declares and recommends. Everything about its type is in [`PluginType`].
///
/// Usually derived, see [`derive@Plugin`]. The [`Debug`] form is what names the plugin in an
/// [`Incompatibility`](compatibility::Incompatibility).
pub trait Plugin: Debug + __private::Erased + 'static {
    /// Declares the facts this configuration provides and requires on top of
    /// [`PluginType::declare_static`], and the values of the facts declared there; nothing by
    /// default.
    ///
    /// The declaration must depend on the plugin's configuration alone, since it is made before
    /// the composite exists.
    fn declare(&self, _: &mut Declaration) {}

    /// The plugins this configuration suggests for the composite it is part of, which is what a
    /// field taking plugins of their axis offers as its default; nothing by default.
    ///
    /// A recommendation of a plugin of an axis the composite takes nowhere has no effect.
    fn recommends(&self) -> Vec<PluginConfig> {
        Vec::new()
    }
}

/// What a plugin is as a type: what identifies and describes it, and what every configuration of
/// it declares. Usually derived, see [`derive@Plugin`].
///
/// A plugin is configured through its [`Configurable`] impl, which is also where the plugins it
/// is made of come from.
pub trait PluginType: Plugin + Configurable {
    /// What identifies the plugin among the plugins of its axis, in a config as in a saved file,
    /// e.g. `"my_crate::points"`. Never change it once configurations naming it are saved.
    ///
    /// A plugin persisted through `typetag` has to be tagged with this id, which [`register`]
    /// does.
    const ID: &'static str;

    /// What an operator picking a plugin reads.
    const LABEL: &'static str;

    /// What an operator picking a plugin reads about what it does.
    const DESCRIPTION: &'static str;

    /// Declares the facts every configuration of this plugin provides to and requires of the
    /// composite it is part of, see [`Declaration`]; nothing by default.
    ///
    /// A program reads this declaration from the [catalog] before the plugin is configured.
    fn declare_static(_: &mut Declaration) {}
}
