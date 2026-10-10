// During development only! TODO: Remove this
#![allow(dead_code)]

use crate::compatibility::Declaration;

mod catalog;
mod compatibility;

/// Re-exported so that a crate registering a plugin in the [catalog] does not have to depend on
/// `inventory` itself.
pub use inventory;

pub trait TotalDeclare {
    fn total_declare(&self, declaration: &mut Declaration);
}

impl<T: Plugin> TotalDeclare for T {
    fn total_declare(&self, declaration: &mut Declaration) {
        T::static_declare(declaration);
        self.dyn_declare(declaration);
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
    /// The declaration must depend on the plugin's configuration alone, since it is made before
    /// the phase exists.
    fn static_declare(_: &mut Declaration)
    where
        Self: Sized,
    {
    }

    fn dyn_declare(&self, _: &mut Declaration) {}

    fn sub_plugins(&self) -> Option<Box<dyn Iterator<Item = &dyn Plugin>>> {
        None
    }
}
