use std::any::TypeId;

use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Match, MatchResult, PhaseData, RankingId},
    map::Map,
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator},
        ranker::{Comparison, RankingCriterion, RankingOrder},
    },
    type_map::TypeMapValue,
};

/// How many of its matches a ranking won, drew and lost.
///
/// Each of the three counts orders a ranking on its own, as does the number of matches they add up
/// to, see [`MoreWins`], [`MoreDraws`], [`FewerLosses`] and [`FewerMatchesPlayed`]. The struct as a
/// whole has no order and so does not implement [`Compare`](crate::plugins::ranker::Compare).
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct WinsDrawsLosses {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

impl WinsDrawsLosses {
    /// How many matches the ranking played, i.e. how many of its matches got a result reported.
    ///
    /// The same count as the length of [`Matches`](super::matches::Matches), without needing that
    /// metric to be calculated.
    pub const fn matches_played(&self) -> u32 {
        self.wins + self.draws + self.losses
    }
}

#[typetag::serde]
impl TypeMapValue for WinsDrawsLosses {}

/// Ranks by the wins of [`WinsDrawsLosses`], where more wins are better.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoreWins;

#[typetag::serde]
impl RankingCriterion for MoreWins {
    fn order_rankings(&self, order: RankingOrder<'_, '_>) {
        order.by_metric_compared_by::<WinsDrawsLosses>(|wdl, other| {
            Comparison::greater_is_better(wdl.wins.cmp(&other.wins))
        });
    }
}

/// Ranks by the draws of [`WinsDrawsLosses`], where more draws are better.
///
/// Rank by [`FewerLosses`] instead where drawing a match should not rank above winning one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoreDraws;

#[typetag::serde]
impl RankingCriterion for MoreDraws {
    fn order_rankings(&self, order: RankingOrder<'_, '_>) {
        order.by_metric_compared_by::<WinsDrawsLosses>(|wdl, other| {
            Comparison::greater_is_better(wdl.draws.cmp(&other.draws))
        });
    }
}

/// Ranks by the losses of [`WinsDrawsLosses`], where fewer losses are better.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FewerLosses;

#[typetag::serde]
impl RankingCriterion for FewerLosses {
    fn order_rankings(&self, order: RankingOrder<'_, '_>) {
        order.by_metric_compared_by::<WinsDrawsLosses>(|wdl, other| {
            Comparison::less_is_better(wdl.losses.cmp(&other.losses))
        });
    }
}

/// Ranks by the matches a ranking played, see [`WinsDrawsLosses::matches_played`], where having
/// played fewer of them is better.
///
/// Meant as a tie break: of two rankings with the same result, the one that needed fewer matches
/// for it ranks higher. A phase that means to reward lasting longer instead, e.g. a knockout, needs
/// a criterion of its own that sorts the other way round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FewerMatchesPlayed;

#[typetag::serde]
impl RankingCriterion for FewerMatchesPlayed {
    fn order_rankings(&self, order: RankingOrder<'_, '_>) {
        order.by_metric_compared_by::<WinsDrawsLosses>(|wdl, other| {
            Comparison::less_is_better(wdl.matches_played().cmp(&other.matches_played()))
        });
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WinsDrawsLossesCalculator;

#[typetag::serde]
impl MetricsCalculator for WinsDrawsLossesCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(WinsDrawsLosses::default())
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
        let (winning_team, losing_team) = match r#match.result() {
            MatchResult::AWins => (r#match.team_a(), r#match.team_b()),
            MatchResult::BWins => (r#match.team_b(), r#match.team_a()),
            MatchResult::Draw => {
                return r#match
                    .rankings()
                    .map(|r_id| {
                        let mut metric = *phase_data.rankings()[r_id]
                            .metric::<WinsDrawsLosses>()
                            .unwrap();
                        metric.draws += 1;
                        (*r_id, Box::new(metric) as Metric)
                    })
                    .collect();
            }
            _ => unreachable!("We expect the match to be finished above"),
        };
        winning_team
            .iter()
            .map(|r_id| {
                let mut metric = *phase_data.rankings()[r_id]
                    .metric::<WinsDrawsLosses>()
                    .unwrap();
                metric.wins += 1;
                (*r_id, Box::new(metric) as Metric)
            })
            .chain(losing_team.iter().map(|r_id| {
                let mut metric = *phase_data.rankings()[r_id]
                    .metric::<WinsDrawsLosses>()
                    .unwrap();
                metric.losses += 1;
                (*r_id, Box::new(metric) as Metric)
            }))
            .collect()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{
        entities::competition::Phase,
        plugins::{
            metrics_calculator::test_utils::{self, MatchSpec, match_id},
            ranker::compare_by,
        },
        type_map::TypeMap,
    };

    /// Builds a phase with `num_players` rankings whose wins/draws/losses got initialized and
    /// with all matches of `matches` already scheduled.
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![Box::new(WinsDrawsLossesCalculator)],
        )
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> WinsDrawsLosses {
        test_utils::metric_of(phase, ranking_index)
    }

    const fn wins_draws_losses(wins: u32, draws: u32, losses: u32) -> WinsDrawsLosses {
        WinsDrawsLosses {
            wins,
            draws,
            losses,
        }
    }

    #[test]
    fn initialize_metrics_starts_every_ranking_at_zero() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), wins_draws_losses(0, 0, 0));
        }
    }

    #[test]
    fn a_wins_counts_as_win_for_team_a_and_as_loss_for_team_b() {
        let mut phase = phase_with(4, vec![(1, vec![0], vec![1]), (1, vec![2], vec![3])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(1, 0, 0));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(0, 0, 1));
        // the rankings of the other match are untouched
        assert_eq!(metric_of(&phase, 2), wins_draws_losses(0, 0, 0));
        assert_eq!(metric_of(&phase, 3), wins_draws_losses(0, 0, 0));
    }

    #[test]
    fn b_wins_counts_as_win_for_team_b_and_as_loss_for_team_a() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(0, 0, 1));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(1, 0, 0));
    }

    #[test]
    fn draw_counts_as_draw_for_both_teams() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::Draw);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(0, 1, 0));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(0, 1, 0));
    }

    #[test]
    fn every_ranking_of_a_team_gets_the_result_of_the_team() {
        let mut phase = phase_with(4, vec![(1, vec![0, 1], vec![2, 3])]);
        let match_id = match_id(&phase, 1, &[0, 1], &[2, 3]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(0, 0, 1));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(0, 0, 1));
        assert_eq!(metric_of(&phase, 2), wins_draws_losses(1, 0, 0));
        assert_eq!(metric_of(&phase, 3), wins_draws_losses(1, 0, 0));
    }

    #[test]
    fn results_accumulate_over_multiple_matches() {
        let mut phase = phase_with(
            4,
            vec![
                (1, vec![0], vec![1]),
                (1, vec![2], vec![3]),
                (2, vec![0], vec![2]),
                (2, vec![1], vec![3]),
                (3, vec![1], vec![2]),
            ],
        );

        for (round, team_a, team_b, result) in [
            (1, vec![0], vec![1], MatchResult::AWins),
            (1, vec![2], vec![3], MatchResult::Draw),
            (2, vec![0], vec![2], MatchResult::BWins),
            (2, vec![1], vec![3], MatchResult::AWins),
            (3, vec![1], vec![2], MatchResult::BWins),
        ] {
            let match_id = match_id(&phase, round, &team_a, &team_b);
            phase.report_match_result(match_id, result);
        }

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(1, 0, 1));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(1, 0, 2));
        assert_eq!(metric_of(&phase, 2), wins_draws_losses(2, 1, 0));
        assert_eq!(metric_of(&phase, 3), wins_draws_losses(0, 1, 1));
    }

    /// The metrics of a ranking whose only metric is `wdl`.
    fn metrics_of(wdl: WinsDrawsLosses) -> TypeMap {
        let mut metrics = TypeMap::new();
        metrics.insert(wdl);
        metrics
    }

    /// How `criterion` orders a ranking with `first` against one with `second`.
    fn comparison(
        criterion: &dyn RankingCriterion,
        first: WinsDrawsLosses,
        second: WinsDrawsLosses,
    ) -> Comparison {
        let (first, second) = (metrics_of(first), metrics_of(second));
        compare_by(criterion, &first, &second).expect("both rankings hold the metric")
    }

    #[test]
    fn more_wins_are_better() {
        assert_eq!(
            comparison(
                &MoreWins,
                wins_draws_losses(2, 0, 0),
                wins_draws_losses(1, 0, 0)
            ),
            Comparison::Better
        );
        assert_eq!(
            comparison(
                &MoreWins,
                wins_draws_losses(1, 0, 0),
                wins_draws_losses(2, 0, 0)
            ),
            Comparison::Worse
        );
        // the same number of wins ties, however the draws and losses differ
        assert_eq!(
            comparison(
                &MoreWins,
                wins_draws_losses(1, 3, 0),
                wins_draws_losses(1, 0, 3)
            ),
            Comparison::Equal
        );
    }

    #[test]
    fn more_draws_are_better() {
        assert_eq!(
            comparison(
                &MoreDraws,
                wins_draws_losses(0, 2, 0),
                wins_draws_losses(0, 1, 0)
            ),
            Comparison::Better
        );
        assert_eq!(
            comparison(
                &MoreDraws,
                wins_draws_losses(0, 1, 0),
                wins_draws_losses(0, 2, 0)
            ),
            Comparison::Worse
        );
        // the same number of draws ties, however the wins and losses differ
        assert_eq!(
            comparison(
                &MoreDraws,
                wins_draws_losses(3, 1, 0),
                wins_draws_losses(0, 1, 3)
            ),
            Comparison::Equal
        );
    }

    #[test]
    fn fewer_losses_are_better() {
        assert_eq!(
            comparison(
                &FewerLosses,
                wins_draws_losses(0, 0, 1),
                wins_draws_losses(0, 0, 2)
            ),
            Comparison::Better
        );
        assert_eq!(
            comparison(
                &FewerLosses,
                wins_draws_losses(0, 0, 2),
                wins_draws_losses(0, 0, 1)
            ),
            Comparison::Worse
        );
        // the same number of losses ties, however the wins and draws differ
        assert_eq!(
            comparison(
                &FewerLosses,
                wins_draws_losses(3, 0, 1),
                wins_draws_losses(0, 3, 1)
            ),
            Comparison::Equal
        );
    }

    #[test]
    fn matches_played_counts_every_result() {
        assert_eq!(wins_draws_losses(0, 0, 0).matches_played(), 0);
        assert_eq!(wins_draws_losses(2, 1, 3).matches_played(), 6);
    }

    #[test]
    fn fewer_played_matches_are_better() {
        assert_eq!(
            comparison(
                &FewerMatchesPlayed,
                wins_draws_losses(1, 0, 0),
                wins_draws_losses(1, 1, 0)
            ),
            Comparison::Better
        );
        assert_eq!(
            comparison(
                &FewerMatchesPlayed,
                wins_draws_losses(1, 1, 0),
                wins_draws_losses(1, 0, 0)
            ),
            Comparison::Worse
        );
        // only how many matches were played counts, not how they went
        assert_eq!(
            comparison(
                &FewerMatchesPlayed,
                wins_draws_losses(3, 0, 0),
                wins_draws_losses(0, 1, 2)
            ),
            Comparison::Equal
        );
    }

    /// All four criteria read the same metric, so none of them orders a ranking that does not hold
    /// it, which is what [`LexicographicalCriteria`] reports rather than ranking by a metric that
    /// is not there.
    ///
    /// [`LexicographicalCriteria`]: crate::plugins::ranker::LexicographicalCriteria
    #[test]
    fn a_criterion_orders_nothing_if_a_ranking_lacks_the_metric() {
        let with_metric = metrics_of(wins_draws_losses(1, 0, 0));
        let without_metric = TypeMap::new();

        for criterion in [
            &MoreWins as &dyn RankingCriterion,
            &MoreDraws,
            &FewerLosses,
            &FewerMatchesPlayed,
        ] {
            assert!(compare_by(criterion, &with_metric, &without_metric).is_none());
        }
    }

    #[test]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn updating_an_already_reported_result_is_unsupported() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(match_id, MatchResult::AWins);

        WinsDrawsLossesCalculator.update_after_match_result_changed(
            phase.data(),
            &phase.matches()[&match_id],
            MatchResult::AWins,
            &Map::default(),
        );
    }
}
