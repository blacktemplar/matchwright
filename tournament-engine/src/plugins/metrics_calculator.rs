use std::any::TypeId;

use crate::{
    entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
    map::Map,
    plugins::{Plugin, compatibility::Declaration},
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

/// A metric plugins can require, see [`Declaration::requires_metric`].
pub trait CalculatedMetric: TypeMapValue {
    /// The calculator added to a phase that requires this metric but configures no calculator for
    /// it, or [`None`] if no single calculator fits every phase, so that it has to be configured.
    ///
    /// The returned calculator must calculate this metric.
    fn default_calculator() -> Option<Box<dyn MetricsCalculator>> {
        None
    }
}

#[typetag::serde(tag = "type", content = "value")]
pub trait MetricsCalculator: Plugin {
    /// Declares what this calculator requires of its phase, in particular every metric besides its
    /// own that it reads, see [`Declaration`]. A calculator runs after the calculators of the
    /// metrics it requires.
    ///
    /// The metric this calculator calculates is declared by [`Self::default_metric_value`].
    fn declare(&self, declaration: &mut Declaration);

    /// The value every ranking of a phase starts out with.
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
