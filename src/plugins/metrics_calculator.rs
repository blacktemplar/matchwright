use std::{any::TypeId, collections::HashSet, fmt::Debug};

use dyn_clone::{DynClone, clone_box};

use crate::{
    entities::competition::{MatchId, MatchResult, Phase, RankingId},
    type_map::DebugAny,
};

pub mod round_of_first_loss;

pub type MetricId = TypeId;

pub type Metric = Box<dyn DebugAny>;

pub trait MetricsCalculator: Debug + DynClone {
    /// Declared statically, so disjointness can be validated without
    /// running any computation.
    fn produced_metrics(&self) -> HashSet<MetricId>;
    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        match_id: MatchId,
        old_match_result: MatchResult,
    ) -> Vec<(RankingId, Metric)>;
    fn initialize_metrics(&self, phase: &Phase) -> Vec<(RankingId, Metric)>;
}

impl Clone for Box<dyn MetricsCalculator> {
    fn clone(&self) -> Self {
        clone_box(&**self)
    }
}
