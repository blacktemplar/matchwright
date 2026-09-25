use std::any::TypeId;

use crate::{
    entities::competition::{Match, MatchResult, Phase, RankingId},
    hash_map::HashMap,
    plugins::Plugin,
    type_map::DebugAny,
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

pub type Metric = Box<dyn DebugAny>;

pub trait MetricsCalculator: Plugin {
    fn default_metric_value(&self) -> Metric;

    /// The metrics in the result vec must all have the type id returned by
    /// [`Self::produced_metric`].
    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        r#match: &Match,
        old_match_result: MatchResult,
        old_metrics: &HashMap<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;
}

impl Clone for Box<dyn MetricsCalculator> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}
