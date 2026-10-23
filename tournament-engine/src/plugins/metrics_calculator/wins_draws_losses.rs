use std::fmt::{self, Display, Formatter};

use plugin_factory::{Plugin, compatibility::Declaration, register};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Bye, Match, PhaseData, RankingId, ResultOfTeam, Team},
    plugins::{
        facts::CalculatesMetric,
        metrics_calculator::{Metric, MetricColumn, MetricsCalculator, OldMetrics},
        ranker::{Comparison, RankingCriterion, requires_metric},
    },
    type_map::TypeMapValue,
};

/// How many of its matches a ranking won, drew and lost, and how many byes it had.
///
/// Each of the first three counts orders a ranking on its own, as does the number of matches they
/// add up to, see [`MoreWins`], [`MoreDraws`], [`FewerLosses`] and [`FewerMatchesPlayed`]. A bye is
/// not a match, so it counts as none of them.
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct WinsDrawsLosses {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub byes: u32,
}

impl WinsDrawsLosses {
    /// How many matches the ranking played, i.e. how many of its matches got a result reported,
    /// byes not included.
    ///
    /// The same count as the length of [`Ranking::played_matches`](crate::Ranking::played_matches).
    pub const fn matches_played(&self) -> u32 {
        self.wins + self.draws + self.losses
    }
}

/// Shows the wins, draws and losses as e.g. `2-1-0`, without the byes.
impl Display for WinsDrawsLosses {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.wins, self.draws, self.losses)
    }
}

#[typetag::serde(name = "tournament_engine::wins_draws_losses")]
impl TypeMapValue for WinsDrawsLosses {}

/// Ranks by the wins of [`WinsDrawsLosses`], where more wins are better.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::more_wins",
    label = "more wins",
    declare_static = requires_metric::<Self>
)]
pub struct MoreWins;

impl RankingCriterion for MoreWins {
    type Metric = WinsDrawsLosses;

    fn compare(&self, first: &WinsDrawsLosses, second: &WinsDrawsLosses) -> Comparison {
        Comparison::greater_is_better(first.wins.cmp(&second.wins))
    }
}

crate::register_criterion!(MoreWins);

/// Ranks by the draws of [`WinsDrawsLosses`], where more draws are better.
///
/// Rank by [`FewerLosses`] instead where drawing a match should not rank above winning one.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::more_draws",
    label = "more draws",
    declare_static = requires_metric::<Self>
)]
pub struct MoreDraws;

impl RankingCriterion for MoreDraws {
    type Metric = WinsDrawsLosses;

    fn compare(&self, first: &WinsDrawsLosses, second: &WinsDrawsLosses) -> Comparison {
        Comparison::greater_is_better(first.draws.cmp(&second.draws))
    }
}

crate::register_criterion!(MoreDraws);

/// Ranks by the losses of [`WinsDrawsLosses`], where fewer losses are better.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::fewer_losses",
    label = "fewer losses",
    declare_static = requires_metric::<Self>
)]
pub struct FewerLosses;

impl RankingCriterion for FewerLosses {
    type Metric = WinsDrawsLosses;

    fn compare(&self, first: &WinsDrawsLosses, second: &WinsDrawsLosses) -> Comparison {
        Comparison::less_is_better(first.losses.cmp(&second.losses))
    }
}

crate::register_criterion!(FewerLosses);

/// Ranks by the matches a ranking played, see [`WinsDrawsLosses::matches_played`], where having
/// played fewer of them is better.
///
/// Meant as a tie break: of two rankings with the same result, the one that needed fewer matches
/// for it ranks higher. A phase that means to reward lasting longer instead, e.g. a knockout, needs
/// a criterion of its own that sorts the other way round.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::fewer_matches_played",
    label = "fewer matches played",
    description = "the same result out of fewer matches ranks higher",
    declare_static = requires_metric::<Self>
)]
pub struct FewerMatchesPlayed;

impl RankingCriterion for FewerMatchesPlayed {
    type Metric = WinsDrawsLosses;

    fn compare(&self, first: &WinsDrawsLosses, second: &WinsDrawsLosses) -> Comparison {
        Comparison::less_is_better(first.matches_played().cmp(&second.matches_played()))
    }
}

crate::register_criterion!(FewerMatchesPlayed);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::wins_draws_losses",
    label = "win-draw-loss record",
    description = "how many matches were won, drawn and lost",
    declare_static = Self::declare_static
)]
pub struct WinsDrawsLossesCalculator;

impl WinsDrawsLossesCalculator {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(CalculatesMetric::<WinsDrawsLosses>::new());
    }
}

#[register]
impl MetricsCalculator for WinsDrawsLossesCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(WinsDrawsLosses::default())
    }

    fn update_after_match_result(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        let result = r#match.result().expect("a reported match is finished");
        [Team::A, Team::B]
            .into_iter()
            .flat_map(|team| {
                let result = result.for_team(team);
                r#match.rankings_of(team).iter().map(move |r_id| {
                    let mut metric = *phase_data.rankings()[r_id]
                        .metric::<WinsDrawsLosses>()
                        .unwrap();
                    match result {
                        ResultOfTeam::Win => metric.wins += 1,
                        ResultOfTeam::Draw => metric.draws += 1,
                        ResultOfTeam::Loss => metric.losses += 1,
                    }
                    (*r_id, Box::new(metric) as Metric)
                })
            })
            .collect()
    }

    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        bye: &Bye,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        let mut metric = *phase_data.rankings()[&bye.ranking()]
            .metric::<WinsDrawsLosses>()
            .unwrap();
        metric.byes += 1;
        vec![(bye.ranking(), Box::new(metric) as Metric)]
    }

    fn column(&self) -> Option<MetricColumn> {
        Some(MetricColumn::new::<WinsDrawsLosses>("W-D-L"))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;
    use crate::{
        entities::competition::{MatchResult, Phase, PhaseBuilder},
        plugins::metrics_calculator::test_utils::{self, MatchSpec, match_id},
    };

    /// Builds a phase with `num_players` rankings whose wins/draws/losses got initialized and
    /// with all matches of `matches` already scheduled.
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(
            PhaseBuilder::new()
                .add_players(num_players)
                .add_metric(WinsDrawsLossesCalculator),
            matches,
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
            byes: 0,
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

    #[test]
    fn a_bye_is_counted_as_soon_as_it_is_scheduled_but_not_as_a_match() {
        let mut phase = test_utils::phase_with_byes(
            PhaseBuilder::new()
                .add_players(3)
                .add_metric(WinsDrawsLossesCalculator),
            vec![(1, vec![0], vec![1])],
            vec![(1, 2), (2, 2)],
        );
        assert_eq!(metric_of(&phase, 2).byes, 1);

        // finishing the first round schedules the bye of the second one
        let match_id = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(match_id, MatchResult::AWins);

        let with_two_byes = metric_of(&phase, 2);
        assert_eq!(with_two_byes.byes, 2);
        assert_eq!(with_two_byes.matches_played(), 0);
        assert_eq!(metric_of(&phase, 0), wins_draws_losses(1, 0, 0));
    }

    /// How `criterion` orders a ranking with `first` against one with `second`.
    fn comparison(
        criterion: &impl RankingCriterion<Metric = WinsDrawsLosses>,
        first: WinsDrawsLosses,
        second: WinsDrawsLosses,
    ) -> Comparison {
        criterion.compare(&first, &second)
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
}
