use std::num::NonZeroU32;

use crate::{
    entities::competition::{MatchId, MatchResult, Phase, RankingId},
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator},
        ranker::{Compare, Comparison},
    },
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub struct RoundOfFirstLoss(pub Option<NonZeroU32>);

// Higher values are better, None is best.
impl Compare for RoundOfFirstLoss {
    fn compare(&self, other: &Self) -> Comparison {
        match (self.0, other.0) {
            (None, None) => Comparison::Equal,
            (None, Some(_)) => Comparison::Better,
            (Some(_), None) => Comparison::Worse,
            (Some(x), Some(y)) => Comparison::greater_is_better(x.cmp(&y)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RoundOfFirstLossCalculator;

impl MetricsCalculator for RoundOfFirstLossCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(RoundOfFirstLoss::default())
    }

    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        match_id: MatchId,
        old_match_result: MatchResult,
    ) -> Vec<(RankingId, Metric)> {
        // for now we only support setting the match result, not updating it
        assert_eq!(old_match_result, MatchResult::NotFinished);

        let r#match = phase.matches().get(&match_id).unwrap();
        assert!(r#match.is_finished());
        let losing_team_id = match r#match.result() {
            MatchResult::AWins => r#match.team_b()[0],
            MatchResult::BWins => r#match.team_a()[0],
            _ => return vec![], // nothing to update
        };
        let metric = RoundOfFirstLoss(Some(r#match.round()));
        vec![(losing_team_id, Box::new(metric))]
    }
}
