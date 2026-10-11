//! Plugins that describe themselves, so that a program configuring them never names one.
//!
//! A [`Plugin`] says what to call it, what it has to be told in order to be built, see [`schema`]
//! and [`config`], and what it provides to and requires of the composite it is part of, see
//! [`compatibility`]. It registers itself in the [`catalog`], which is where a program looking for
//! a plugin finds it. A [`Builder`](builder::Builder) holds a composite of plugins while it is
//! being configured.
//!
//! A plugin is usually derived, which reads its schema from its fields and their `serde`
//! attributes, and registered with [`register`], which tags it for `typetag` with its id:
//!
//! ```
//! use std::fmt::Debug;
//!
//! use plugin_factory::{Plugin, plugin_kind, register};
//! use serde::{Deserialize, Serialize};
//!
//! #[typetag::serde(tag = "type", content = "value")]
//! pub trait Seeding: Plugin + Debug {}
//!
//! plugin_kind! {
//!     pub SeedingKind for dyn Seeding
//! }
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
//! # fn main() {
//! # use plugin_factory::catalog::PluginKind;
//! # assert_eq!(SeedingKind::entry("my_crate::random").unwrap().label(), "random");
//! # }
//! ```

use crate::{compatibility::Declaration, config::PluginConfig, schema::Schema};

pub mod builder;
pub mod catalog;
pub mod compatibility;
pub mod config;
pub mod schema;

/// Re-exported so that a crate registering a plugin in the [catalog] does not have to depend on
/// `inventory` itself.
pub use inventory;
pub use plugin_factory_derive::{Plugin, register};

/// What the code [`derive@Plugin`] and [`register`] generate refers to. Not part of the API.
#[doc(hidden)]
pub mod __private {
    pub use serde;
    use serde_json::value::Serializer;

    use crate::config::Value;

    /// The id of a derived plugin as a constant, which is what `typetag` takes as a name.
    pub trait PluginId {
        const ID: &'static str;
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

/// Everything a plugin declares, i.e. what it and its sub-plugins provide and require.
///
/// Implemented for every [`Plugin`], and the part of a declaration that a plugin behind a `dyn`
/// can still be asked for.
pub trait TotalDeclare {
    /// Adds what this plugin and its sub-plugins declare to `declaration`.
    fn total_declare(&self, declaration: &mut Declaration);
}

impl<T: Plugin> TotalDeclare for T {
    fn total_declare(&self, declaration: &mut Declaration) {
        T::static_declare(declaration);
        self.dyn_declare(declaration);
        for sub_plugin in self.sub_plugins().into_iter().flatten() {
            sub_plugin.total_declare(declaration);
        }
    }
}

/// A plugin of some axis, see [`catalog`]. Usually derived, see [`derive@Plugin`].
pub trait Plugin: TotalDeclare + 'static {
    /// What identifies the plugin among the plugins of its kind, in a config as in a saved file,
    /// e.g. `"my_crate::points"`. Never change it once configurations naming it are saved.
    ///
    /// A plugin persisted through `typetag` has to be tagged with this id, see
    /// [`check_serialization`](catalog::check_serialization).
    fn id() -> &'static str
    where
        Self: Sized;

    fn label() -> &'static str
    where
        Self: Sized;

    fn description() -> &'static str
    where
        Self: Sized;

    /// Declares the facts this plugin provides to and requires of the phase it is part of, see
    /// [`Declaration`].
    ///
    /// Every configuration of the plugin provides the facts declared here and requires the facts
    /// declared here, so that a program can read this declaration from the [catalog] before the
    /// plugin is configured.
    fn static_declare(_: &mut Declaration)
    where
        Self: Sized,
    {
    }

    /// Declares the facts the configuration this plugin has provides and requires on top of
    /// [`Self::static_declare`], and the values of the facts declared there.
    ///
    /// The declaration must depend on the plugin's configuration alone, since it is made before
    /// the phase exists.
    fn dyn_declare(&self, _: &mut Declaration) {}

    /// What has to be answered in order to build the plugin, see [`schema`]; nothing by default.
    ///
    /// The plugin is built by deserializing a config answering every field of its schema, see
    /// [`Configurable`](schema::Configurable), which every plugin with a `Deserialize` impl is.
    fn schema() -> Schema
    where
        Self: Sized,
    {
        Schema::new()
    }

    /// The plugins this configuration suggests for the composite it is part of, which is what a
    /// field taking plugins of their kind offers as its default; nothing by default.
    ///
    /// A recommendation of a plugin of a kind the composite takes nowhere has no effect.
    fn recommends(&self) -> Vec<PluginConfig> {
        Vec::new()
    }

    /// The plugins this plugin is made of, whose declarations are part of its own.
    fn sub_plugins(&self) -> Option<Box<dyn Iterator<Item = &dyn Plugin> + '_>> {
        None
    }
}
