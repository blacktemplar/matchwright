use std::{any::TypeId, num::NonZeroU32};

use plugin_factory::{Plugin, compatibility::Declaration, register};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Bye, Match, MatchResult, PhaseData, Ranking, RankingId},
    map::Map,
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator},
        ranker::{Compare, Comparison, RankingCriterion, SliceRanker, rank_by, requires_metric},
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

/// Ranks by the [`RoundOfFirstLoss`]: the later the better, never having lost is best.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::later_first_loss",
    label = "round of the first loss",
    description = "the later the better, never having lost is best",
    declare_static = requires_metric::<Self>
)]
pub struct LaterFirstLoss;

impl RankingCriterion for LaterFirstLoss {
    type Metric = RoundOfFirstLoss;

    fn compare(&self, first: &RoundOfFirstLoss, second: &RoundOfFirstLoss) -> Comparison {
        first.compare(second)
    }
}

#[register]
impl SliceRanker for LaterFirstLoss {
    fn update_ranks(&mut self, _: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]) {
        rank_by(self, slice);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::round_of_first_loss",
    label = "round of the first loss",
    description = "the round in which an entrant lost for the first time",
    declare_static = Self::declare_static
)]
pub struct RoundOfFirstLossCalculator;

impl RoundOfFirstLossCalculator {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(RoundOfFirstLoss::default());
    }
}

#[register]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::Phase,
        plugins::metrics_calculator::test_utils::{self, MatchSpec, match_id},
    };

    /// A ranking that has not lost a match yet, which is the best this metric gets.
    const UNBEATEN: RoundOfFirstLoss = RoundOfFirstLoss(None);

    /// Builds a phase with `num_players` rankings and all matches of `matches` already scheduled.
    ///
    /// [`RoundOfFirstLossCalculator`] reads nothing but the result and the round of the match it is
    /// called for, so it is the only calculator of the phase.
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![Box::new(RoundOfFirstLossCalculator)],
        )
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> RoundOfFirstLoss {
        test_utils::metric_of(phase, ranking_index)
    }

    /// The metric of a ranking whose first loss was in round `round`.
    fn lost_in(round: u32) -> RoundOfFirstLoss {
        RoundOfFirstLoss(NonZeroU32::new(round))
    }

    /// Reports `result` for the match of `round` between the two single-ranking teams.
    fn report(phase: &mut Phase, round: u32, result: MatchResult) {
        let match_id = match_id(phase, round, &[0], &[1]);
        phase.report_match_result(match_id, result);
    }

    #[test]
    fn initialize_metrics_leaves_every_ranking_unbeaten() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), UNBEATEN);
        }
    }

    #[test]
    fn the_loser_of_a_match_lost_in_the_round_of_that_match() {
        let mut phase = phase_with(2, vec![(3, vec![0], vec![1])]);

        report(&mut phase, 3, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), UNBEATEN);
        assert_eq!(metric_of(&phase, 1), lost_in(3));
    }

    #[test]
    fn a_draw_is_not_a_loss() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);

        report(&mut phase, 1, MatchResult::Draw);

        assert_eq!(metric_of(&phase, 0), UNBEATEN);
        assert_eq!(metric_of(&phase, 1), UNBEATEN);
    }

    #[test]
    fn every_ranking_of_a_losing_team_lost() {
        let mut phase = phase_with(4, vec![(1, vec![0, 1], vec![2, 3])]);
        let match_id = match_id(&phase, 1, &[0, 1], &[2, 3]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), lost_in(1));
        assert_eq!(metric_of(&phase, 1), lost_in(1));
        assert_eq!(metric_of(&phase, 2), UNBEATEN);
        assert_eq!(metric_of(&phase, 3), UNBEATEN);
    }

    #[test]
    fn a_later_loss_does_not_replace_the_first_one() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1]), (2, vec![0], vec![1])]);

        report(&mut phase, 1, MatchResult::AWins);
        report(&mut phase, 2, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 1), lost_in(1));
    }

    /// Results arrive in whatever order they are reported in, so a loss of an earlier round can
    /// follow one of a later round and is then the first loss.
    #[test]
    fn the_earliest_round_lost_counts_whenever_it_is_reported() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1]), (2, vec![0], vec![1])]);

        report(&mut phase, 2, MatchResult::AWins);
        assert_eq!(metric_of(&phase, 1), lost_in(2));

        report(&mut phase, 1, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 1), lost_in(1));
    }

    #[test]
    fn a_bye_is_never_lost() {
        let phase = test_utils::phase_with_byes(
            2,
            vec![],
            vec![(1, 0)],
            vec![Box::new(RoundOfFirstLossCalculator)],
        );

        assert_eq!(phase.byes().len(), 1);
        assert_eq!(metric_of(&phase, 0), UNBEATEN);
    }

    #[test]
    fn a_later_first_loss_is_better_and_never_having_lost_is_best() {
        assert_eq!(UNBEATEN.compare(&lost_in(3)), Comparison::Better);
        assert_eq!(lost_in(3).compare(&UNBEATEN), Comparison::Worse);
        assert_eq!(lost_in(3).compare(&lost_in(2)), Comparison::Better);
        assert_eq!(lost_in(2).compare(&lost_in(3)), Comparison::Worse);
        assert_eq!(lost_in(2).compare(&lost_in(2)), Comparison::Equal);
        assert_eq!(UNBEATEN.compare(&UNBEATEN), Comparison::Equal);
    }

    #[test]
    fn the_criterion_ranks_by_the_round_of_the_first_loss() {
        assert_eq!(
            LaterFirstLoss.compare(&UNBEATEN, &lost_in(1)),
            Comparison::Better
        );
    }
}
