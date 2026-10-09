use std::{fmt::Debug, mem};

use dyn_clone::DynClone;

use crate::plugins::{
    compatibility::{Declaration, Incompatibility, declaration_of, order, unprovided_with_default},
    metrics_calculator::MetricsCalculator,
    phase_format::PhaseFormat,
    ranker::Ranker,
};

pub mod compatibility;
pub mod metrics_calculator;
pub mod phase_format;
pub mod ranker;

/// The supertrait of every plugin trait.
///
/// A plugin may keep state in `self` and update it in its `&mut self` methods. That state is
/// serialized together with the phase, and it must only depend on the [`PhaseData`] and the calls
/// the plugin has seen, so that replaying the transition log recomputes it.
///
/// A plugin also declares what it provides to and requires of its phase, see [`Self::declare`].
///
/// [`PhaseData`]: crate::entities::competition::PhaseData
pub trait Plugin: Debug + DynClone {
    /// Declares the facts this plugin provides to and requires of the phase it is part of, see
    /// [`Declaration`].
    ///
    /// The declaration must depend on the plugin's configuration alone, since it is made before
    /// the phase exists.
    fn declare(&self, declaration: &mut Declaration);
}

/// The plugins of one phase, which a
/// [default provider](compatibility::DefaultProvider::add_default_provider) adds to.
#[derive(Debug, Clone)]
pub struct PhasePlugins {
    pub(crate) phase_format: Box<dyn PhaseFormat>,
    pub(crate) ranker: Box<dyn Ranker>,
    pub(crate) metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

impl PhasePlugins {
    pub(crate) fn new(
        phase_format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Self {
        Self {
            phase_format,
            ranker,
            metrics_calculators,
        }
    }

    pub fn phase_format(&self) -> &dyn PhaseFormat {
        &*self.phase_format
    }

    pub fn ranker(&self) -> &dyn Ranker {
        &*self.ranker
    }

    /// The metrics calculators, in the order they run in.
    pub fn metrics_calculators(&self) -> &[Box<dyn MetricsCalculator>] {
        &self.metrics_calculators
    }

    /// Adds `metrics_calculator` after the metrics calculators already there.
    pub fn add_metrics_calculator(&mut self, metrics_calculator: Box<dyn MetricsCalculator>) {
        self.metrics_calculators.push(metrics_calculator);
    }

    /// The declarations of all plugins, each together with the name of its plugin.
    pub(crate) fn declarations(&self) -> Vec<(String, Declaration)> {
        [
            declaration_of(&*self.phase_format),
            declaration_of(&*self.ranker),
        ]
        .into_iter()
        .chain(self.declarations_of_metrics_calculators())
        .collect()
    }

    fn declarations_of_metrics_calculators(&self) -> Vec<(String, Declaration)> {
        self.metrics_calculators
            .iter()
            .map(|metrics_calculator| declaration_of(&**metrics_calculator))
            .collect()
    }

    /// Adds the default provider of every required fact nothing provides, including those the
    /// added providers require, and orders the metrics calculators so that each runs after the
    /// ones providing the facts it requires, keeping their order otherwise.
    ///
    /// # Errors
    /// Returns an error if metrics calculators require facts of each other in a cycle.
    pub(crate) fn resolve(&mut self) -> Result<(), Incompatibility> {
        // every fact gets one try, so that a fact without default provider, or with one that does
        // not provide it, does not stop the loop from ending
        let mut tried = Vec::new();
        loop {
            let declarations = self.declarations();
            let Some((fact, add_default_provider)) =
                unprovided_with_default(&declarations).find(|(fact, _)| !tried.contains(fact))
            else {
                break;
            };
            tried.push(fact);
            add_default_provider(self);
        }

        let order = order(&self.declarations_of_metrics_calculators())?;
        let mut metrics_calculators: Vec<_> = mem::take(&mut self.metrics_calculators)
            .into_iter()
            .map(Some)
            .collect();
        self.metrics_calculators = order
            .into_iter()
            .map(|index| {
                metrics_calculators[index]
                    .take()
                    .expect("an order holds every index once")
            })
            .collect();
        Ok(())
    }

    /// Checks that the metrics calculators are ordered as [`Self::resolve`] orders them.
    ///
    /// # Errors
    /// Returns an error if they are not, or if they require facts of each other in a cycle.
    pub(crate) fn check_order(&self) -> Result<(), Incompatibility> {
        let order = order(&self.declarations_of_metrics_calculators())?;
        match order
            .into_iter()
            .enumerate()
            .find(|(position, index)| position != index)
        {
            Some((position, _)) => Err(Incompatibility::OutOfOrder {
                plugin: format!("{:?}", self.metrics_calculators[position]),
            }),
            None => Ok(()),
        }
    }
}
