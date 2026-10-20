use std::{
    fmt::{self, Display, Formatter},
    num::NonZeroU32,
};

use plugin_factory::{Plugin, compatibility::Declaration, register};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Bye, Match, PhaseData, RankingId},
    plugins::{
        facts::CalculatesMetric,
        metrics_calculator::{Metric, MetricColumn, MetricsCalculator, OldMetrics},
        ranker::{Comparison, RankingCriterion, requires_metric},
    },
    type_map::TypeMapValue,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RoundOfFirstLoss(pub Option<NonZeroU32>);

/// Shows the round as e.g. `round 3`, and `-` if there was no loss.
impl Display for RoundOfFirstLoss {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(round) => write!(f, "round {round}"),
            None => f.write_str("-"),
        }
    }
}

#[typetag::serde]
impl TypeMapValue for RoundOfFirstLoss {}

/// Ranks by the [`RoundOfFirstLoss`]: the later the better, never having lost is best.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::later_first_loss",
    label = "later first loss",
    description = "by the round of the first loss; never having lost is best",
    declare_static = requires_metric::<Self>
)]
pub struct LaterFirstLoss;

impl RankingCriterion for LaterFirstLoss {
    type Metric = RoundOfFirstLoss;

    fn compare(&self, first: &RoundOfFirstLoss, second: &RoundOfFirstLoss) -> Comparison {
        Comparison::none_or_greater_is_better(first.0, second.0)
    }
}

crate::register_criterion!(LaterFirstLoss);

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
        declaration.provide(CalculatesMetric::<RoundOfFirstLoss>::new());
    }
}

#[register]
impl MetricsCalculator for RoundOfFirstLossCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(RoundOfFirstLoss::default())
    }

    fn update_after_match_result(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        let result = r#match.result().expect("a reported match is finished");
        let Some(loser) = result.loser() else {
            return vec![]; // nothing to update
        };
        r#match
            .rankings_of(loser)
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
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        vec![]
    }

    fn column(&self) -> Option<MetricColumn> {
        Some(MetricColumn::new::<RoundOfFirstLoss>("lost in"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::{MatchResult, Phase},
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

        assert_eq!(phase.data().byes().len(), 1);
        assert_eq!(metric_of(&phase, 0), UNBEATEN);
    }

    #[test]
    fn a_later_first_loss_is_better_and_never_having_lost_is_best() {
        let compare = |first, second| LaterFirstLoss.compare(&first, &second);
        assert_eq!(compare(UNBEATEN, lost_in(3)), Comparison::Better);
        assert_eq!(compare(lost_in(3), UNBEATEN), Comparison::Worse);
        assert_eq!(compare(lost_in(3), lost_in(2)), Comparison::Better);
        assert_eq!(compare(lost_in(2), lost_in(3)), Comparison::Worse);
        assert_eq!(compare(lost_in(2), lost_in(2)), Comparison::Equal);
        assert_eq!(compare(UNBEATEN, UNBEATEN), Comparison::Equal);
    }
}
