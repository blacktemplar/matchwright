use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use super::Metric;
use crate::{
    entities::competition::{Match, MatchResult, Phase, RankingId},
    plugins::{
        metrics_calculator::{
            MetricsCalculator,
            matches::{Matches, Team},
            points::Points,
        },
        ranker::{Compare, Comparison},
    },
};

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub struct Buchholz(pub f32);

// Higher values are better
impl Compare for Buchholz {
    fn compare(&self, other: &Self) -> Comparison {
        Comparison::greater_is_better(self.0.total_cmp(&other.0))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BuchholzCalculator;

impl MetricsCalculator for BuchholzCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(Buchholz::default())
    }

    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        r#match: &Match,
        _: MatchResult,
        old_metrics: &HashMap<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        let mut buchholz_changes: HashMap<_, f32> = HashMap::new();
        for (r_id, old_metric) in &old_metrics[&TypeId::of::<Points>()] {
            let old_metric: &Points = (&**old_metric as &dyn Any).downcast_ref().unwrap();
            let ranking = &phase.rankings()[r_id];
            let new_metric = ranking.metric::<Points>().unwrap();
            let points_diff = new_metric.0 - old_metric.0;
            if points_diff != 0. {
                // buchholz of all opponents of this ranking change
                let matches = ranking.metric::<Matches>().unwrap();
                for (match_id, team) in &matches.0 {
                    if match_id == &r#match.id() {
                        // we don't consider the newly reported match here as we will account for it
                        // later separately
                        continue;
                    }
                    let match_of_r = &phase.matches()[match_id];
                    let opponents = match team {
                        Team::A => match_of_r.team_b(),
                        Team::B => match_of_r.team_a(),
                    };
                    for opponent in opponents {
                        *buchholz_changes.entry(*opponent).or_default() += points_diff;
                    }
                }
            }
        }
        // now also account for the new match
        for (team1, team2) in [
            (r#match.team_a(), r#match.team_b()),
            (r#match.team_b(), r#match.team_a()),
        ] {
            // add points of team2 to buchholz of team1
            let points: f32 = team2
                .iter()
                .map(|r_id| phase.rankings()[r_id].metric::<Points>().unwrap().0)
                .sum();
            for r_id in team1 {
                *buchholz_changes.entry(*r_id).or_default() += points;
            }
        }
        buchholz_changes
            .into_iter()
            .map(|(r_id, change)| {
                (
                    r_id,
                    Box::new(Buchholz(
                        phase.rankings()[&r_id].metric::<Buchholz>().unwrap().0 + change,
                    )) as Metric,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::metrics_calculator::{
        matches::MatchesCalculator,
        points::PointsCalculator,
        test_utils::{self, MatchSpec, match_id},
        wins_draws_losses::WinsDrawsLossesCalculator,
    };

    /// The scoring system of most association football leagues, used by every test here.
    const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

    /// Builds a phase with `num_players` rankings and all matches of `matches` already scheduled,
    /// scoring finished matches with [`THREE_ONE`].
    ///
    /// [`BuchholzCalculator`] derives its metric from [`Points`] and [`Matches`], so the phase has
    /// to update both of them first, and [`Points`] in turn derives from
    /// [`WinsDrawsLosses`](super::wins_draws_losses::WinsDrawsLosses).
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE),
                Box::new(MatchesCalculator),
                Box::new(BuchholzCalculator),
            ],
        )
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> Buchholz {
        test_utils::metric_of(phase, ranking_index)
    }

    /// The schedule of [`play_scenario`], as (round, team a, team b, result).
    const SCENARIO: [(u32, &[usize], &[usize], MatchResult); 5] = [
        (1, &[0], &[1], MatchResult::AWins),
        (1, &[2], &[3], MatchResult::Draw),
        (2, &[0], &[2], MatchResult::BWins),
        (2, &[1], &[3], MatchResult::AWins),
        (3, &[1], &[2], MatchResult::BWins),
    ];

    /// Plays [`SCENARIO`] between four players, reporting its matches in the given order, which
    /// leaves them with the following points: `0`: 3, `1`: 3, `2`: 7, `3`: 1.
    fn play_scenario(report_order: impl Iterator<Item = usize>) -> Phase {
        let matches = SCENARIO
            .iter()
            .map(|(round, team_a, team_b, _)| (*round, team_a.to_vec(), team_b.to_vec()))
            .collect();
        let mut phase = phase_with(4, matches);
        for index in report_order {
            let (round, team_a, team_b, result) = SCENARIO[index];
            let match_id = match_id(&phase, round, team_a, team_b);
            phase.report_match_result(match_id, result);
        }
        phase
    }

    #[test]
    fn initialize_metrics_starts_every_ranking_at_zero() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), Buchholz(0.0));
        }
    }

    #[test]
    fn the_buchholz_of_a_ranking_is_the_sum_of_the_points_of_its_opponents() {
        let phase = play_scenario(0..SCENARIO.len());

        // opponents of `0` are `1` (3 points) and `2` (7 points)
        assert_eq!(metric_of(&phase, 0), Buchholz(10.0));
        // opponents of `1` are `0` (3), `3` (1) and `2` (7)
        assert_eq!(metric_of(&phase, 1), Buchholz(11.0));
        // opponents of `2` are `3` (1), `0` (3) and `1` (3)
        assert_eq!(metric_of(&phase, 2), Buchholz(7.0));
        // opponents of `3` are `2` (7) and `1` (3)
        assert_eq!(metric_of(&phase, 3), Buchholz(10.0));
    }

    #[test]
    fn the_order_in_which_results_are_reported_does_not_change_the_buchholz() {
        let phase = play_scenario((0..SCENARIO.len()).rev());

        assert_eq!(metric_of(&phase, 0), Buchholz(10.0));
        assert_eq!(metric_of(&phase, 1), Buchholz(11.0));
        assert_eq!(metric_of(&phase, 2), Buchholz(7.0));
        assert_eq!(metric_of(&phase, 3), Buchholz(10.0));
    }

    #[test]
    fn points_an_opponent_wins_later_are_added_retroactively() {
        let mut phase = phase_with(3, vec![(1, vec![0], vec![1]), (2, vec![1], vec![2])]);

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::AWins);
        // `1` has no points yet, so beating it is worth nothing so far
        assert_eq!(metric_of(&phase, 0), Buchholz(0.0));

        let second_match = match_id(&phase, 2, &[1], &[2]);
        phase.report_match_result(second_match, MatchResult::AWins);

        // the three points `1` won in its second match count for `0` as well
        assert_eq!(metric_of(&phase, 0), Buchholz(3.0));
        assert_eq!(metric_of(&phase, 1), Buchholz(3.0));
        assert_eq!(metric_of(&phase, 2), Buchholz(3.0));
    }

    #[test]
    fn only_opponents_of_reported_matches_count() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (1, vec![2], vec![3]),
            (2, vec![0], vec![2]),
        ];
        let mut phase = phase_with(4, matches);

        for (round, team_a, team_b) in [(1, [0], [1]), (1, [2], [3])] {
            let match_id = match_id(&phase, round, &team_a, &team_b);
            phase.report_match_result(match_id, MatchResult::AWins);
        }

        // the scheduled but unreported match of `0` against `2` contributes nothing, even though
        // `2` won its first match
        assert_eq!(metric_of(&phase, 0), Buchholz(0.0));
        assert_eq!(metric_of(&phase, 1), Buchholz(3.0));
        assert_eq!(metric_of(&phase, 2), Buchholz(0.0));
        assert_eq!(metric_of(&phase, 3), Buchholz(3.0));
    }

    #[test]
    fn every_ranking_of_a_team_counts_the_points_of_every_opponent_ranking() {
        let matches = vec![(1, vec![0, 1], vec![2, 3]), (2, vec![0, 1], vec![2, 3])];
        let mut phase = phase_with(4, matches);

        for (round, result) in [(1, MatchResult::AWins), (2, MatchResult::Draw)] {
            let match_id = match_id(&phase, round, &[0, 1], &[2, 3]);
            phase.report_match_result(match_id, result);
        }

        // `2` and `3` end up with one point each, counted once for each of the two matches
        for ranking_index in [0, 1] {
            assert_eq!(metric_of(&phase, ranking_index), Buchholz(4.0));
        }
        // `0` and `1` end up with four points each, again counted for both matches
        for ranking_index in [2, 3] {
            assert_eq!(metric_of(&phase, ranking_index), Buchholz(16.0));
        }
    }

    #[test]
    #[should_panic(expected = "no entry found for key")]
    fn calculating_the_buchholz_without_the_points_metric_panics() {
        let mut phase = test_utils::phase_with(
            2,
            vec![(1, vec![0], vec![1])],
            vec![Box::new(MatchesCalculator), Box::new(BuchholzCalculator)],
        );
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);
    }

    #[test]
    fn a_higher_buchholz_is_better() {
        assert_eq!(Buchholz(7.0).compare(&Buchholz(3.0)), Comparison::Better);
        assert_eq!(Buchholz(3.0).compare(&Buchholz(7.0)), Comparison::Worse);
        assert_eq!(Buchholz(0.5).compare(&Buchholz(0.0)), Comparison::Better);
        assert_eq!(Buchholz(3.0).compare(&Buchholz(3.0)), Comparison::Equal);
    }
}
