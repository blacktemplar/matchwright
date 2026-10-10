//! Plugins that describe themselves, so that a program configuring them never names one.
//!
//! A [`Plugin`] says what to call it and what it provides to and requires of the composite it is
//! part of, see [`compatibility`]. It registers itself in the [`catalog`], which is where a
//! program looking for a plugin finds it.

use crate::compatibility::Declaration;

pub mod catalog;
pub mod compatibility;

/// Re-exported so that a crate registering a plugin in the [catalog] does not have to depend on
/// `inventory` itself.
pub use inventory;

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

pub trait Plugin: TotalDeclare + 'static {
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

    /// The plugins this plugin is made of, whose declarations are part of its own.
    fn sub_plugins(&self) -> Option<Box<dyn Iterator<Item = &dyn Plugin> + '_>> {
        None
    }
}
