use std::any::TypeId;

use crate::{
    entities::competition::{Match, MatchResult, PhaseData, RankingId},
    map::Map,
    plugins::Plugin,
    type_map::TypeMapValue,
};

pub mod buchholz;
pub mod matches;
pub mod points;
pub mod round_of_first_loss;
pub mod sonneborn_berger;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod wins_draws_losses;

pub type MetricId = TypeId;

pub type Metric = Box<dyn TypeMapValue>;

#[typetag::serde(tag = "type", content = "value")]
pub trait MetricsCalculator: Plugin {
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
}

impl Clone for Box<dyn MetricsCalculator> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}
