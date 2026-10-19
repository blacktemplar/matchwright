use std::any::{Any, TypeId};

use dyn_clone::DynClone;
use plugin_factory::{Plugin, axis};

use crate::{
    entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
    map::Map,
    type_map::{TypeMapValue, downcast_unchecked_ref},
};

pub mod buchholz;
pub mod matches;
pub mod points;
pub mod round_of_elimination;
pub mod round_of_first_loss;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod wins_draws_losses;

pub type MetricId = TypeId;

pub type Metric = Box<dyn TypeMapValue>;

/// The values of metrics that the calculators which already ran for an update replaced, keyed by
/// metric type.
#[derive(Debug, Default)]
pub struct OldMetrics(Map<MetricId, Vec<(RankingId, Metric)>>);

impl OldMetrics {
    /// The replaced values of the metric `M`, each with the ranking it belonged to, in the order
    /// they were replaced.
    pub fn of<M: TypeMapValue>(&self) -> impl Iterator<Item = (RankingId, &M)> {
        self.0
            .get(&TypeId::of::<M>())
            .into_iter()
            .flatten()
            .map(|(ranking_id, metric)| {
                let metric: &dyn Any = &**metric;
                // SAFETY: `push` stores every metric under the type id of its concrete type, so
                // the metrics at key `TypeId::of::<M>()` are `M`s.
                (*ranking_id, unsafe { downcast_unchecked_ref::<M>(metric) })
            })
    }

    /// Records `metric` as the value of `ranking_id` that got replaced.
    pub(crate) fn push(&mut self, ranking_id: RankingId, metric: Metric) {
        let metric_id = (&*metric as &dyn Any).type_id();
        self.0
            .entry(metric_id)
            .or_default()
            .push((ranking_id, metric));
    }
}

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
    /// `old_metrics` holds the values replaced by the calculators that already ran, so that a
    /// calculator deriving its metric from another one can see what changed there.
    ///
    /// All returned metrics must have the same concrete type as
    /// [`Self::default_metric_value`].
    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        old_match_result: MatchResult,
        old_metrics: &OldMetrics,
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
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)>;
}

dyn_clone::clone_trait_object!(MetricsCalculator);
