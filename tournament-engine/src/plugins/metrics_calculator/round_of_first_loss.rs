use std::{any::TypeId, num::NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
    map::Map,
    plugins::{
        Plugin,
        compatibility::Declaration,
        metrics_calculator::{Metric, MetricsCalculator},
        ranker::{Compare, Comparison, RankingCriterion, RankingOrder},
    },
    type_map::TypeMapValue,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RoundOfFirstLoss(pub Option<NonZeroU32>);

#[typetag::serde]
impl TypeMapValue for RoundOfFirstLoss {}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaterFirstLoss;

#[typetag::serde]
impl RankingCriterion for LaterFirstLoss {
    fn order_rankings(&self, order: RankingOrder<'_, '_>) {
        order.by_metric::<RoundOfFirstLoss>();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RoundOfFirstLossCalculator;

impl Plugin for RoundOfFirstLossCalculator {
    fn declare(&self, declaration: &mut Declaration) {
        declaration.provides(RoundOfFirstLoss::default());
    }
}

#[typetag::serde]
impl MetricsCalculator for RoundOfFirstLossCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(RoundOfFirstLoss::default())
    }

    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        old_match_result: MatchResult,
        _: &Map<TypeId, Vec<(RankingId, Metric)>>,
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
                let old_value = phase_data.rankings()[r_id]
                    .metric::<RoundOfFirstLoss>()
                    .unwrap();
                let new_value = RoundOfFirstLoss(Some(match old_value.0 {
                    None => r#match.round(),
                    Some(x) => x.min(r#match.round()),
                }));
                (*r_id, Box::new(new_value) as Metric)
            })
            .collect()
    }

    /// A bye is never lost.
    fn update_after_bye(
        &mut self,
        _: &PhaseData,
        _: &Bye,
        _: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        vec![]
    }
}
