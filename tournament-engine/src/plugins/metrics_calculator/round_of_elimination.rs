use std::{any::TypeId, num::NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Match, MatchResult, PhaseData, RankingId, Team},
    map::Map,
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator},
        ranker::{Compare, Comparison, RankingCriterion, RankingOrder},
    },
    type_map::TypeMapValue,
};

/// Who is knocked out of the phase by losing a match, attached to the match as an annotation by
/// the [`PhaseFormat`](crate::plugins::phase_format::PhaseFormat) that schedules it.
///
/// A match without this annotation knocks out nobody, and neither does a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Eliminates {
    /// The loser is out, whichever side it is.
    Loser,
    /// Only this side is out if it loses; the other side plays on after a loss.
    Only(Team),
}

#[typetag::serde(name = "tournament_engine::eliminates")]
impl TypeMapValue for Eliminates {}

/// The round in which a ranking was knocked out of the phase, see [`Eliminates`], or `None` while
/// it is still in.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RoundOfElimination(pub Option<NonZeroU32>);

#[typetag::serde]
impl TypeMapValue for RoundOfElimination {}

// Higher values are better, None is best.
impl Compare for RoundOfElimination {
    fn compare(&self, other: &Self) -> Comparison {
        match (self.0, other.0) {
            (None, None) => Comparison::Equal,
            (None, Some(_)) => Comparison::Better,
            (Some(_), None) => Comparison::Worse,
            (Some(x), Some(y)) => Comparison::greater_is_better(x.cmp(&y)),
        }
    }
}

/// Ranks by [`RoundOfElimination`]: the later a ranking was knocked out the better, still being in
/// is best.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaterElimination;

#[typetag::serde]
impl RankingCriterion for LaterElimination {
    fn order_rankings(&self, order: RankingOrder<'_, '_>) {
        order.by_metric::<RoundOfElimination>();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RoundOfEliminationCalculator;

#[typetag::serde]
impl MetricsCalculator for RoundOfEliminationCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(RoundOfElimination::default())
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

        let Some(eliminates) = r#match.annotation::<Eliminates>() else {
            return vec![];
        };
        let loser = match r#match.result() {
            MatchResult::AWins => Team::B,
            MatchResult::BWins => Team::A,
            MatchResult::Draw | MatchResult::NotFinished => return vec![],
        };
        if matches!(eliminates, Eliminates::Only(team) if *team != loser) {
            return vec![];
        }
        r#match
            .rankings_of(loser)
            .iter()
            // a ranking that is already out stays out since the round it was knocked out in
            .filter(|r_id| {
                let eliminated = phase_data.rankings()[*r_id].metric::<RoundOfElimination>();
                eliminated.unwrap().0.is_none()
            })
            .map(|r_id| {
                let eliminated = RoundOfElimination(Some(r#match.round()));
                (*r_id, Box::new(eliminated) as Metric)
            })
            .collect()
    }
}
