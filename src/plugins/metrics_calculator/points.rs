use std::any::TypeId;

use serde::{Deserialize, Serialize};

use super::Metric;
use crate::{
    entities::competition::{Match, MatchResult, Phase, RankingId},
    hash_map::HashMap,
    plugins::{
        metrics_calculator::{MetricsCalculator, wins_draws_losses::WinsDrawsLosses},
        ranker::{Compare, Comparison, MetricGetter, MetricStorage},
    },
    type_map::TypeMapValue,
};

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct Points(pub f32);

#[typetag::serde]
impl TypeMapValue for Points {}

// Higher values are better
impl Compare for Points {
    fn compare(&self, other: &Self) -> Comparison {
        Comparison::greater_is_better(self.0.total_cmp(&other.0))
    }
}

impl AsRef<f32> for Points {
    fn as_ref(&self) -> &f32 {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsGetter;

#[typetag::serde]
impl MetricGetter for PointsGetter {
    fn store_metric<'a, 'b>(&self, storage: MetricStorage<'a, 'b>) {
        storage.set_metric::<Points>();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PointsCalculator {
    points_for_win: f32,
    points_for_draw: f32,
}

impl PointsCalculator {
    /// Scores every win with `points_for_win` and every draw with `points_for_draw` points, a
    /// loss is always worth no points.
    pub const fn new(points_for_win: f32, points_for_draw: f32) -> Self {
        Self {
            points_for_win,
            points_for_draw,
        }
    }
}

#[typetag::serde]
impl MetricsCalculator for PointsCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(Points::default())
    }

    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        r#match: &Match,
        _: MatchResult,
        _: &HashMap<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        r#match
            .non_losing_rankings()
            .map(|r_id| {
                let WinsDrawsLosses { wins, draws, .. } =
                    *phase.rankings()[r_id].metric::<WinsDrawsLosses>().unwrap();
                let points =
                    Points(self.points_for_win * wins as f32 + self.points_for_draw * draws as f32);
                (*r_id, Box::new(points) as Metric)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::metrics_calculator::{
        test_utils::{self, MatchSpec, match_id},
        wins_draws_losses::WinsDrawsLossesCalculator,
    };

    const fn calculator(points_for_win: Points, points_for_draw: Points) -> PointsCalculator {
        PointsCalculator::new(points_for_win.0, points_for_draw.0)
    }

    /// The scoring system of most association football leagues.
    const THREE_ONE: PointsCalculator = calculator(Points(3.0), Points(1.0));
    /// The scoring system association football used before it moved to [`THREE_ONE`], still in
    /// use e.g. in ice hockey.
    const TWO_ONE: PointsCalculator = calculator(Points(2.0), Points(1.0));
    /// The scoring system of chess and most other games scoring a draw as half a win.
    const ONE_HALF: PointsCalculator = calculator(Points(1.0), Points(0.5));

    /// Builds a phase with `num_players` rankings and all matches of `matches` already scheduled,
    /// scoring finished matches with `calculator`.
    ///
    /// [`PointsCalculator`] derives its metric from [`WinsDrawsLosses`], so the phase has to
    /// update that metric first.
    fn phase_with(
        num_players: u32,
        matches: Vec<MatchSpec>,
        calculator: PointsCalculator,
    ) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![Box::new(WinsDrawsLossesCalculator), Box::new(calculator)],
        )
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> Points {
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

    /// Plays [`SCENARIO`] between four players, which leaves them with the following
    /// wins/draws/losses: `0`: 1/0/1, `1`: 1/0/2, `2`: 2/1/0, `3`: 0/1/1.
    fn play_scenario(calculator: PointsCalculator) -> Phase {
        let matches = SCENARIO
            .iter()
            .map(|(round, team_a, team_b, _)| (*round, team_a.to_vec(), team_b.to_vec()))
            .collect();
        let mut phase = phase_with(4, matches, calculator);
        for (round, team_a, team_b, result) in SCENARIO {
            let match_id = match_id(&phase, round, team_a, team_b);
            phase.report_match_result(match_id, result);
        }
        phase
    }

    #[test]
    fn initialize_metrics_starts_every_ranking_at_zero() {
        let phase = phase_with(4, vec![], THREE_ONE);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), Points(0.0));
        }
    }

    #[test]
    fn three_points_for_a_win_and_one_for_a_draw() {
        let phase = play_scenario(THREE_ONE);

        assert_eq!(metric_of(&phase, 0), Points(3.0));
        assert_eq!(metric_of(&phase, 1), Points(3.0));
        assert_eq!(metric_of(&phase, 2), Points(7.0));
        assert_eq!(metric_of(&phase, 3), Points(1.0));
    }

    #[test]
    fn two_points_for_a_win_and_one_for_a_draw() {
        let phase = play_scenario(TWO_ONE);

        assert_eq!(metric_of(&phase, 0), Points(2.0));
        assert_eq!(metric_of(&phase, 1), Points(2.0));
        assert_eq!(metric_of(&phase, 2), Points(5.0));
        assert_eq!(metric_of(&phase, 3), Points(1.0));
    }

    #[test]
    fn one_point_for_a_win_and_half_a_point_for_a_draw() {
        let phase = play_scenario(ONE_HALF);

        assert_eq!(metric_of(&phase, 0), Points(1.0));
        assert_eq!(metric_of(&phase, 1), Points(1.0));
        assert_eq!(metric_of(&phase, 2), Points(2.5));
        assert_eq!(metric_of(&phase, 3), Points(0.5));
    }

    #[test]
    fn half_points_for_draws_add_up_without_rounding() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (2, vec![0], vec![1]),
            (3, vec![0], vec![1]),
        ];
        let mut phase = phase_with(2, matches, ONE_HALF);

        for round in 1..=3 {
            let match_id = match_id(&phase, round, &[0], &[1]);
            phase.report_match_result(match_id, MatchResult::Draw);
        }

        assert_eq!(metric_of(&phase, 0), Points(1.5));
        assert_eq!(metric_of(&phase, 1), Points(1.5));
    }

    #[test]
    fn a_loss_is_worth_no_points() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])], THREE_ONE);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), Points(0.0));
        assert_eq!(metric_of(&phase, 1), Points(3.0));
    }

    #[test]
    fn only_the_rankings_of_the_reported_match_get_points() {
        let matches = vec![(1, vec![0], vec![1]), (1, vec![2], vec![3])];
        let mut phase = phase_with(4, matches, THREE_ONE);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), Points(3.0));
        assert_eq!(metric_of(&phase, 1), Points(0.0));
        // the rankings of the other match are still at their initial value
        assert_eq!(metric_of(&phase, 2), Points(0.0));
        assert_eq!(metric_of(&phase, 3), Points(0.0));
    }

    #[test]
    fn every_ranking_of_a_team_gets_the_points_of_the_team() {
        let mut phase = phase_with(4, vec![(1, vec![0, 1], vec![2, 3])], THREE_ONE);
        let match_id = match_id(&phase, 1, &[0, 1], &[2, 3]);

        phase.report_match_result(match_id, MatchResult::Draw);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), Points(1.0));
        }
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn scoring_without_the_wins_draws_losses_metric_panics() {
        let mut phase =
            test_utils::phase_with(2, vec![(1, vec![0], vec![1])], vec![Box::new(THREE_ONE)]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);
    }

    #[test]
    fn more_points_are_better() {
        assert_eq!(Points(3.0).compare(&Points(1.0)), Comparison::Better);
        assert_eq!(Points(1.0).compare(&Points(3.0)), Comparison::Worse);
        assert_eq!(Points(1.5).compare(&Points(1.0)), Comparison::Better);
        assert_eq!(Points(1.5).compare(&Points(1.5)), Comparison::Equal);
    }
}
