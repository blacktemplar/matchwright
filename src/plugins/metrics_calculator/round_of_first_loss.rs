use std::{any::TypeId, collections::HashMap, num::NonZeroU32};

use crate::{
    entities::competition::{Match, MatchResult, Phase, RankingId},
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
        r#match: &Match,
        old_match_result: MatchResult,
        _: &HashMap<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        // for now we only support setting the match result, not updating it
        assert_eq!(old_match_result, MatchResult::NotFinished);

        assert!(r#match.is_finished());
        let losing_ranking_ids = match r#match.result() {
            MatchResult::AWins => r#match.team_b(),
            MatchResult::BWins => r#match.team_a(),
            _ => return vec![], // nothing to update
        };
        losing_ranking_ids
            .iter()
            .map(|r_id| {
                let old_value = phase.rankings()[r_id].metric::<RoundOfFirstLoss>().unwrap();
                let new_value = RoundOfFirstLoss(Some(match old_value.0 {
                    None => r#match.round(),
                    Some(x) => x.min(r#match.round()),
                }));
                (*r_id, Box::new(new_value) as Metric)
            })
            .collect()
    }
}
