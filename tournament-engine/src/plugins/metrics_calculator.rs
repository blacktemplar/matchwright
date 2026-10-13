use std::any::TypeId;

use dyn_clone::DynClone;
use plugin_factory::{Plugin, axis};

use crate::{
    entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
    map::Map,
    type_map::TypeMapValue,
};

pub mod buchholz;
pub mod matches;
pub mod points;
pub mod round_of_elimination;
pub mod round_of_first_loss;
pub mod sonneborn_berger;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod wins_draws_losses;

pub type MetricId = TypeId;

pub type Metric = Box<dyn TypeMapValue>;

/// Calculates one metric of every ranking of a phase.
///
/// A calculator has to declare that it provides the [`CalculatesMetric`] of its metric, and that it
/// requires every fact it reads, in particular the [`CalculatesMetric`] of every other metric it
/// reads. It runs after the calculators providing the facts it requires.
///
/// [`CalculatesMetric`]: super::facts::CalculatesMetric
///
/// A calculator may keep state as a [`PhaseFormat`](super::phase_format::PhaseFormat) does.
#[axis]
pub trait MetricsCalculator: Plugin + DynClone {
    /// The value every ranking of a phase starts out with, whose type is the metric this
    /// calculator calculates.
    fn default_metric_value(&self) -> Metric;

    /// The new values of all rankings whose metric the reported result of `match` changed, where
    /// `old_match_result` is the result the match had before.
    ///
    /// `old_metrics` holds the values replaced by the calculators that already ran, keyed by metric
    /// type, so that a calculator deriving its metric from another one can see what changed there.
    ///
    /// All returned metrics must have the same concrete type as
    /// [`Self::default_metric_value`].
    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        old_match_result: MatchResult,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;

    /// The new values of all rankings whose metric the newly scheduled `bye` changed.
    ///
    /// A bye is not a match: it has no opponent and no result, and it never changes once
    /// scheduled. Whether it counts for this metric at all is up to the calculator.
    ///
    /// `old_metrics` and the returned metrics are as for
    /// [`Self::update_after_match_result_changed`].
    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        bye: &Bye,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;
}

impl Clone for Box<dyn MetricsCalculator> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}
