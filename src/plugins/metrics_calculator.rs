use std::{any::TypeId, collections::HashSet};

use crate::{
    entities::competition::{MatchId, MatchResult, Phase, RankingId},
    plugins::Plugin,
    type_map::DebugAny,
};

pub mod points;
pub mod round_of_first_loss;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod wins_draws_losses;

pub type MetricId = TypeId;

pub type Metric = Box<dyn DebugAny>;

pub trait MetricsCalculator: Plugin {
    /// Declared statically, so disjointness can be validated without
    /// running any computation.
    fn produced_metrics(&self) -> HashSet<MetricId>;
    /// The metrics in the result vec must all have a type id that is present in
    /// [`Self::produced_metrics`].
    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        match_id: MatchId,
        old_match_result: MatchResult,
    ) -> Vec<(RankingId, Metric)>;
    /// The metrics in the result vec must all have a type id that is present in
    /// [`Self::produced_metrics`]. Furhtermore for each ranking in `phase` and each metric in
    /// [`Self::produced_metrics`] there must be exactly one entry in the result vec.
    fn initialize_metrics(&self, phase: &Phase) -> Vec<(RankingId, Metric)>;
}

impl Clone for Box<dyn MetricsCalculator> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}
