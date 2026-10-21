use std::fmt::{self, Display, Formatter};

use plugin_factory::{Plugin, compatibility::Declaration, register};
use serde::{Deserialize, Serialize};

use super::{Metric, MetricColumn, OldMetrics};
use crate::{
    entities::competition::{Bye, Match, PhaseData, RankingId},
    plugins::{
        facts::CalculatesMetric,
        metrics_calculator::{MetricsCalculator, wins_draws_losses::WinsDrawsLosses},
        ranker::{Comparison, RankingCriterion, requires_metric},
    },
    type_map::TypeMapValue,
};

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct Points(pub f32);

#[typetag::serde]
impl TypeMapValue for Points {}

impl Display for Points {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<f32> for Points {
    fn as_ref(&self) -> &f32 {
        &self.0
    }
}

/// Ranks by the [`Points`], where more points are better.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::more_points",
    label = "more points",
    declare_static = requires_metric::<Self>
)]
pub struct MorePoints;

impl RankingCriterion for MorePoints {
    type Metric = Points;

    fn compare(&self, first: &Points, second: &Points) -> Comparison {
        Comparison::greater_is_better(first.0.total_cmp(&second.0))
    }
}

crate::register_criterion!(MorePoints);

/// Calculates the [`Points`] of a ranking from its [`WinsDrawsLosses`], which therefore have to
/// be calculated before.
///
/// Offered with the scoring system of chess and most other games scoring a draw as half a win,
/// with a bye worth as much as a win.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::points",
    label = "points",
    description = "scored for every win, every draw and every bye",
    declare_static = Self::declare_static
)]
pub struct PointsCalculator {
    #[plugin(label = "points for a win", default = 1.0)]
    points_for_win: f32,
    #[plugin(label = "points for a draw", default = 0.5)]
    points_for_draw: f32,
    #[plugin(
        label = "points for a bye",
        description = "a round without a match, e.g. with an odd number of entrants",
        default = 1.0
    )]
    points_for_bye: f32,
}

impl PointsCalculator {
    /// Scores every win with `points_for_win` and every draw with `points_for_draw` points, a
    /// loss and a bye are worth no points.
    ///
    /// Requires the `test-util` feature.
    #[cfg(feature = "test-util")]
    pub const fn new(points_for_win: f32, points_for_draw: f32) -> Self {
        Self {
            points_for_win,
            points_for_draw,
            points_for_bye: 0.0,
        }
    }

    /// Returns this calculator scoring every bye with `points_for_bye` points.
    ///
    /// Requires the `test-util` feature.
    #[cfg(feature = "test-util")]
    pub const fn with_points_for_bye(mut self, points_for_bye: f32) -> Self {
        self.points_for_bye = points_for_bye;
        self
    }

    fn points_of(&self, ranking_id: &RankingId, phase_data: &PhaseData) -> Metric {
        // calculated from the counts rather than added up result by result, so that a number of
        // points not exactly representable as a float always comes out the same
        let WinsDrawsLosses {
            wins, draws, byes, ..
        } = *phase_data.rankings()[ranking_id]
            .metric::<WinsDrawsLosses>()
            .unwrap();
        Box::new(Points(
            self.points_for_win * wins as f32
                + self.points_for_draw * draws as f32
                + self.points_for_bye * byes as f32,
        ))
    }

    fn declare_static(declaration: &mut Declaration) {
        declaration
            .provide(CalculatesMetric::<Points>::new())
            .require::<CalculatesMetric<WinsDrawsLosses>>("the wins, draws and losses");
    }
}

#[register]
impl MetricsCalculator for PointsCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(Points::default())
    }

    fn update_after_match_result(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        r#match
            .non_losing_rankings()
            .map(|r_id| (*r_id, self.points_of(r_id, phase_data)))
            .collect()
    }

    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        bye: &Bye,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        if self.points_for_bye == 0.0 {
            return vec![];
        }
        vec![(bye.ranking(), self.points_of(&bye.ranking(), phase_data))]
    }

    fn column(&self) -> Option<MetricColumn> {
        Some(MetricColumn::new::<Points>("points"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::{MatchResult, Phase, PhaseBuilder},
        plugins::metrics_calculator::{
            test_utils::{self, MatchSpec, match_id},
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
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
            PhaseBuilder::new()
                .add_players(num_players)
                .add_metric(WinsDrawsLossesCalculator)
                .add_metric(calculator),
            matches,
        )
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> Points {
        test_utils::metric_of(phase, ranking_index)
    }

    /// Plays all of [`SCENARIO`](test_utils::SCENARIO), scoring with `calculator`.
    fn play_scenario(calculator: PointsCalculator) -> Phase {
        test_utils::play_scenario(
            phase_with(4, test_utils::scenario_matches(), calculator),
            0..test_utils::SCENARIO.len(),
        )
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

    /// Builds a phase with three rankings, in which `2` has a bye in round 1 and `0` beats `1`
    /// in round 2, scoring with `calculator`.
    fn phase_with_a_bye(calculator: PointsCalculator) -> Phase {
        let mut phase = test_utils::phase_with_byes(
            PhaseBuilder::new()
                .add_players(3)
                .add_metric(WinsDrawsLossesCalculator)
                .add_metric(calculator),
            vec![(2, vec![0], vec![1])],
            vec![(1, 2)],
        );
        let match_id = match_id(&phase, 2, &[0], &[1]);
        phase.report_match_result(match_id, MatchResult::AWins);
        phase
    }

    #[test]
    fn a_bye_is_worth_no_points_by_default() {
        let phase = phase_with_a_bye(ONE_HALF);

        assert_eq!(metric_of(&phase, 2), Points(0.0));
    }

    #[test]
    fn a_bye_is_worth_the_points_for_a_bye() {
        let phase = phase_with_a_bye(ONE_HALF.with_points_for_bye(1.0));

        assert_eq!(metric_of(&phase, 0), Points(1.0));
        assert_eq!(metric_of(&phase, 1), Points(0.0));
        assert_eq!(metric_of(&phase, 2), Points(1.0));
    }

    #[test]
    fn the_points_of_a_bye_add_to_those_of_matches() {
        let mut phase = test_utils::phase_with_byes(
            PhaseBuilder::new()
                .add_players(2)
                .add_metric(WinsDrawsLossesCalculator)
                .add_metric(THREE_ONE.with_points_for_bye(2.0)),
            vec![(2, vec![0], vec![1])],
            vec![(1, 0)],
        );
        let match_id = match_id(&phase, 2, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::Draw);

        assert_eq!(metric_of(&phase, 0), Points(3.0));
        assert_eq!(metric_of(&phase, 1), Points(1.0));
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn scoring_without_the_wins_draws_losses_metric_panics() {
        let mut phase = test_utils::phase_with(
            PhaseBuilder::new().add_players(2).add_metric(THREE_ONE),
            vec![(1, vec![0], vec![1])],
        );
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);
    }

    #[test]
    fn more_points_are_better() {
        let compare = |first, second| MorePoints.compare(&Points(first), &Points(second));
        assert_eq!(compare(3.0, 1.0), Comparison::Better);
        assert_eq!(compare(1.0, 3.0), Comparison::Worse);
        assert_eq!(compare(1.5, 1.0), Comparison::Better);
        assert_eq!(compare(1.5, 1.5), Comparison::Equal);
    }
}
