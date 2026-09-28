use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use serde::{Deserialize, Serialize};

use super::Metric;
use crate::{
    entities::phase::{Match, MatchResult, Phase, RankingId},
    map::Map,
    plugins::{
        metrics_calculator::{MetricsCalculator, matches::Matches, points::Points},
        ranker::{Compare, Comparison, MetricGetter, MetricStorage},
    },
    type_map::TypeMapValue,
};

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct Buchholz(pub f32);

#[typetag::serde]
impl TypeMapValue for Buchholz {}

// Higher values are better
impl Compare for Buchholz {
    fn compare(&self, other: &Self) -> Comparison {
        Comparison::greater_is_better(self.0.total_cmp(&other.0))
    }
}

impl AsRef<f32> for Buchholz {
    fn as_ref(&self) -> &f32 {
        &self.0
    }
}

impl From<f32> for Buchholz {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuchholzGetter;

#[typetag::serde]
impl MetricGetter for BuchholzGetter {
    fn store_metric(&self, storage: MetricStorage<'_>) {
        storage.set_metric::<Buchholz>();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BuchholzCalculator;

#[typetag::serde]
impl MetricsCalculator for BuchholzCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(Buchholz::default())
    }

    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        r#match: &Match,
        _: MatchResult,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        buchholz_changes_based_on::<Points, Buchholz>(phase, r#match, old_metrics)
    }
}

/// The updates of the buchholz like metric `B`, which sums the metric `M` of all opponents of a
/// ranking, after the result of `match` changed.
fn buchholz_changes_based_on<M, B>(
    phase: &Phase,
    r#match: &Match,
    old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
) -> Vec<(RankingId, Metric)>
where
    M: 'static + AsRef<f32>,
    B: TypeMapValue + AsRef<f32> + From<f32>,
{
    let mut buchholz_changes: Map<_, f32> = Map::default();
    for (r_id, old_metric) in &old_metrics[&TypeId::of::<M>()] {
        let old_metric: &M = (&**old_metric as &dyn Any).downcast_ref().unwrap();
        let metrics = phase.metrics_of(r_id);
        let new_metric = metrics.metric::<M>().unwrap();
        let m_diff = *new_metric.as_ref() - *old_metric.as_ref();
        if m_diff != 0. {
            // buchholz of all opponents of this ranking change
            let matches = metrics.metric::<Matches>().unwrap();
            for match_id in &matches.0 {
                if match_id == &r#match.id() {
                    // we don't consider the newly reported match here as we will account for it
                    // later separately
                    continue;
                }
                let match_of_r = &phase.matches()[match_id];
                let opponents =
                    match_of_r.rankings_of(match_of_r.team_of(r_id).unwrap().opponent());
                for opponent in opponents {
                    *buchholz_changes.entry(*opponent).or_default() += m_diff;
                }
            }
        }
    }
    // now also account for the new match
    for (team1, team2) in [
        (r#match.team_a(), r#match.team_b()),
        (r#match.team_b(), r#match.team_a()),
    ] {
        // add metric M of team2 to buchholz of team1
        let m_sum: f32 = team2
            .iter()
            .map(|r_id| *phase.metric_of::<M>(r_id).unwrap().as_ref())
            .sum();
        for r_id in team1 {
            *buchholz_changes.entry(*r_id).or_default() += m_sum;
        }
    }
    buchholz_changes
        .into_iter()
        .map(|(r_id, change)| {
            let old_buchholz = *phase.metric_of::<B>(&r_id).unwrap().as_ref();
            (r_id, Box::new(B::from(old_buchholz + change)) as Metric)
        })
        .collect()
}

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct BuchholzOfBuchholz(pub f32);

#[typetag::serde]
impl TypeMapValue for BuchholzOfBuchholz {}

// Higher values are better
impl Compare for BuchholzOfBuchholz {
    fn compare(&self, other: &Self) -> Comparison {
        Comparison::greater_is_better(self.0.total_cmp(&other.0))
    }
}

impl AsRef<f32> for BuchholzOfBuchholz {
    fn as_ref(&self) -> &f32 {
        &self.0
    }
}

impl From<f32> for BuchholzOfBuchholz {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuchholzOfBuchholzGetter;

#[typetag::serde]
impl MetricGetter for BuchholzOfBuchholzGetter {
    fn store_metric(&self, storage: MetricStorage<'_>) {
        storage.set_metric::<BuchholzOfBuchholz>();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BuchholzOfBuchholzCalculator;

#[typetag::serde]
impl MetricsCalculator for BuchholzOfBuchholzCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(BuchholzOfBuchholz::default())
    }

    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        r#match: &Match,
        _: MatchResult,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        buchholz_changes_based_on::<Buchholz, BuchholzOfBuchholz>(phase, r#match, old_metrics)
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
    /// [`BuchholzOfBuchholzCalculator`] builds on [`Buchholz`] and therefore comes last.
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE),
                Box::new(MatchesCalculator),
                Box::new(BuchholzCalculator),
                Box::new(BuchholzOfBuchholzCalculator),
            ],
        )
    }

    fn buchholz_of(phase: &Phase, ranking_index: usize) -> Buchholz {
        test_utils::metric_of(phase, ranking_index)
    }

    fn buchholz_of_buchholz_of(phase: &Phase, ranking_index: usize) -> BuchholzOfBuchholz {
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
            assert_eq!(buchholz_of(&phase, ranking_index), Buchholz(0.0));
        }
    }

    #[test]
    fn the_buchholz_of_a_ranking_is_the_sum_of_the_points_of_its_opponents() {
        let phase = play_scenario(0..SCENARIO.len());

        // opponents of `0` are `1` (3 points) and `2` (7 points)
        assert_eq!(buchholz_of(&phase, 0), Buchholz(10.0));
        // opponents of `1` are `0` (3), `3` (1) and `2` (7)
        assert_eq!(buchholz_of(&phase, 1), Buchholz(11.0));
        // opponents of `2` are `3` (1), `0` (3) and `1` (3)
        assert_eq!(buchholz_of(&phase, 2), Buchholz(7.0));
        // opponents of `3` are `2` (7) and `1` (3)
        assert_eq!(buchholz_of(&phase, 3), Buchholz(10.0));
    }

    #[test]
    fn the_order_in_which_results_are_reported_does_not_change_the_buchholz() {
        let phase = play_scenario((0..SCENARIO.len()).rev());

        assert_eq!(buchholz_of(&phase, 0), Buchholz(10.0));
        assert_eq!(buchholz_of(&phase, 1), Buchholz(11.0));
        assert_eq!(buchholz_of(&phase, 2), Buchholz(7.0));
        assert_eq!(buchholz_of(&phase, 3), Buchholz(10.0));
    }

    #[test]
    fn points_an_opponent_wins_later_are_added_retroactively() {
        let mut phase = phase_with(3, vec![(1, vec![0], vec![1]), (2, vec![1], vec![2])]);

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::AWins);
        // `1` has no points yet, so beating it is worth nothing so far
        assert_eq!(buchholz_of(&phase, 0), Buchholz(0.0));

        let second_match = match_id(&phase, 2, &[1], &[2]);
        phase.report_match_result(second_match, MatchResult::AWins);

        // the three points `1` won in its second match count for `0` as well
        assert_eq!(buchholz_of(&phase, 0), Buchholz(3.0));
        assert_eq!(buchholz_of(&phase, 1), Buchholz(3.0));
        assert_eq!(buchholz_of(&phase, 2), Buchholz(3.0));
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
        assert_eq!(buchholz_of(&phase, 0), Buchholz(0.0));
        assert_eq!(buchholz_of(&phase, 1), Buchholz(3.0));
        assert_eq!(buchholz_of(&phase, 2), Buchholz(0.0));
        assert_eq!(buchholz_of(&phase, 3), Buchholz(3.0));
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
            assert_eq!(buchholz_of(&phase, ranking_index), Buchholz(4.0));
        }
        // `0` and `1` end up with four points each, again counted for both matches
        for ranking_index in [2, 3] {
            assert_eq!(buchholz_of(&phase, ranking_index), Buchholz(16.0));
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

    #[test]
    fn initialize_metrics_starts_every_buchholz_of_buchholz_at_zero() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(
                buchholz_of_buchholz_of(&phase, ranking_index),
                BuchholzOfBuchholz::default()
            );
        }
    }

    #[test]
    fn the_buchholz_of_buchholz_of_a_ranking_is_the_sum_of_the_buchholz_of_its_opponents() {
        let phase = play_scenario(0..SCENARIO.len());

        // the buchholz of the scenario are `0`: 10, `1`: 11, `2`: 7, `3`: 10

        // opponents of `0` are `1` (11) and `2` (7)
        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(18.0));
        // opponents of `1` are `0` (10), `3` (10) and `2` (7)
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(27.0));
        // opponents of `2` are `3` (10), `0` (10) and `1` (11)
        assert_eq!(buchholz_of_buchholz_of(&phase, 2), BuchholzOfBuchholz(31.0));
        // opponents of `3` are `2` (7) and `1` (11)
        assert_eq!(buchholz_of_buchholz_of(&phase, 3), BuchholzOfBuchholz(18.0));
    }

    #[test]
    fn the_order_in_which_results_are_reported_does_not_change_the_buchholz_of_buchholz() {
        let phase = play_scenario((0..SCENARIO.len()).rev());

        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(18.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(27.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 2), BuchholzOfBuchholz(31.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 3), BuchholzOfBuchholz(18.0));
    }

    #[test]
    fn buchholz_an_opponent_gains_later_is_added_retroactively() {
        let mut phase = phase_with(3, vec![(1, vec![0], vec![1]), (2, vec![1], vec![2])]);

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::AWins);
        // `1` lost against `0`, so its buchholz is already 3 and counts for `0`, whereas the
        // buchholz of `0` itself is still 0, as `1` has no points
        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(3.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(0.0));

        let second_match = match_id(&phase, 2, &[1], &[2]);
        phase.report_match_result(second_match, MatchResult::AWins);

        // every ranking now faced exactly one opponent with a buchholz of 3, except `1`, which
        // faced two of them
        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(3.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(6.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 2), BuchholzOfBuchholz(3.0));
    }

    #[test]
    fn a_match_changes_the_buchholz_of_buchholz_of_rankings_two_matches_away() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (2, vec![1], vec![2]),
            (3, vec![2], vec![3]),
        ];
        let mut phase = phase_with(4, matches);

        for (round, team_a, team_b) in [(1, [0], [1]), (2, [1], [2]), (3, [2], [3])] {
            let match_id = match_id(&phase, round, &team_a, &team_b);
            phase.report_match_result(match_id, MatchResult::AWins);
        }

        // the last match gave `2` its three points, which raised the buchholz of `1` from 3 to 6
        // and with it the buchholz of buchholz of `0`, even though `0` never faced `2`
        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(6.0));
        // opponents of `1` are `0` (buchholz 3) and `2` (buchholz 3)
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(6.0));
        // opponents of `2` are `1` (buchholz 6) and `3` (buchholz 3)
        assert_eq!(buchholz_of_buchholz_of(&phase, 2), BuchholzOfBuchholz(9.0));
        // the only opponent of `3` is `2` (buchholz 3)
        assert_eq!(buchholz_of_buchholz_of(&phase, 3), BuchholzOfBuchholz(3.0));
    }

    #[test]
    fn only_opponents_of_reported_matches_count_for_the_buchholz_of_buchholz() {
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
        // `2` has a buchholz of 3 by now
        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(3.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(0.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 2), BuchholzOfBuchholz(3.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 3), BuchholzOfBuchholz(0.0));
    }

    #[test]
    fn every_ranking_of_a_team_counts_the_buchholz_of_every_opponent_ranking() {
        let matches = vec![(1, vec![0, 1], vec![2, 3]), (2, vec![0, 1], vec![2, 3])];
        let mut phase = phase_with(4, matches);

        for (round, result) in [(1, MatchResult::AWins), (2, MatchResult::Draw)] {
            let match_id = match_id(&phase, round, &[0, 1], &[2, 3]);
            phase.report_match_result(match_id, result);
        }

        // `2` and `3` end up with a buchholz of 16 each, counted once for each of the two matches
        for ranking_index in [0, 1] {
            assert_eq!(
                buchholz_of_buchholz_of(&phase, ranking_index),
                BuchholzOfBuchholz(64.0)
            );
        }
        // `0` and `1` end up with a buchholz of 4 each, again counted for both matches
        for ranking_index in [2, 3] {
            assert_eq!(
                buchholz_of_buchholz_of(&phase, ranking_index),
                BuchholzOfBuchholz(16.0)
            );
        }
    }

    #[test]
    #[should_panic(expected = "no entry found for key")]
    fn calculating_the_buchholz_of_buchholz_without_the_buchholz_metric_panics() {
        let mut phase = test_utils::phase_with(
            2,
            vec![(1, vec![0], vec![1])],
            vec![
                Box::new(MatchesCalculator),
                Box::new(BuchholzOfBuchholzCalculator),
            ],
        );
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);
    }

    #[test]
    fn a_higher_buchholz_of_buchholz_is_better() {
        assert_eq!(
            BuchholzOfBuchholz(7.0).compare(&BuchholzOfBuchholz(3.0)),
            Comparison::Better
        );
        assert_eq!(
            BuchholzOfBuchholz(3.0).compare(&BuchholzOfBuchholz(7.0)),
            Comparison::Worse
        );
        assert_eq!(
            BuchholzOfBuchholz(0.5).compare(&BuchholzOfBuchholz(0.0)),
            Comparison::Better
        );
        assert_eq!(
            BuchholzOfBuchholz(3.0).compare(&BuchholzOfBuchholz(3.0)),
            Comparison::Equal
        );
    }
}
