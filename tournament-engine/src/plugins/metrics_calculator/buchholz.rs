use std::{fmt::Debug, num::NonZeroU32};

use plugin_factory::{Plugin, compatibility::Declaration, register};
use serde::{Deserialize, Serialize};

use super::{Metric, OldMetrics};
use crate::{
    entities::competition::{
        Bye, Match, MatchResult, PhaseData, Ranking, RankingId, ResultOfTeam, Team,
    },
    map::Map,
    plugins::{
        facts::CalculatesMetric,
        metrics_calculator::{MetricsCalculator, matches::Matches, points::Points},
        ranker::{Comparison, RankingCriterion, SliceRanker, rank_by, requires_metric},
    },
    type_map::TypeMapValue,
};

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct Buchholz(pub f32);

#[typetag::serde]
impl TypeMapValue for Buchholz {}

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

/// Ranks by the [`Buchholz`], where more is better.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::more_buchholz",
    label = "more Buchholz",
    declare_static = requires_metric::<Self>
)]
pub struct MoreBuchholz;

impl RankingCriterion for MoreBuchholz {
    type Metric = Buchholz;

    fn compare(&self, first: &Buchholz, second: &Buchholz) -> Comparison {
        Comparison::greater_is_better(first.0.total_cmp(&second.0))
    }
}

#[register]
impl SliceRanker for MoreBuchholz {
    fn update_ranks(&mut self, _: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]) {
        rank_by(self, slice);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::buchholz",
    label = "Buchholz",
    description = "the points of all opponents",
    declare_static = Self::declare_static
)]
pub struct BuchholzCalculator;

impl BuchholzCalculator {
    fn declare_static(declaration: &mut Declaration) {
        declaration
            .provide(CalculatesMetric::<Buchholz>::new())
            .require::<CalculatesMetric<Points>>("the points")
            .require::<CalculatesMetric<Matches>>("the matches");
    }
}

#[register]
impl MetricsCalculator for BuchholzCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(Buchholz::default())
    }

    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: MatchResult,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        weighted_opponent_sum_changes::<Points, Buchholz>(
            phase_data,
            Some(r#match),
            old_metrics,
            |_| 1.,
        )
    }

    /// A bye has no opponent to count, but the points it is worth count for the opponents of its
    /// ranking.
    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        _: &Bye,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        weighted_opponent_sum_changes::<Points, Buchholz>(phase_data, None, old_metrics, |_| 1.)
    }
}

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct BuchholzOfBuchholz(pub f32);

#[typetag::serde]
impl TypeMapValue for BuchholzOfBuchholz {}

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

/// Ranks by the [`BuchholzOfBuchholz`], where more is better.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::more_buchholz_of_buchholz",
    label = "more Buchholz of Buchholz",
    declare_static = requires_metric::<Self>
)]
pub struct MoreBuchholzOfBuchholz;

impl RankingCriterion for MoreBuchholzOfBuchholz {
    type Metric = BuchholzOfBuchholz;

    fn compare(&self, first: &BuchholzOfBuchholz, second: &BuchholzOfBuchholz) -> Comparison {
        Comparison::greater_is_better(first.0.total_cmp(&second.0))
    }
}

#[register]
impl SliceRanker for MoreBuchholzOfBuchholz {
    fn update_ranks(&mut self, _: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]) {
        rank_by(self, slice);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::buchholz_of_buchholz",
    label = "Buchholz of Buchholz",
    description = "the Buchholz of all opponents",
    declare_static = Self::declare_static
)]
pub struct BuchholzOfBuchholzCalculator;

impl BuchholzOfBuchholzCalculator {
    fn declare_static(declaration: &mut Declaration) {
        declaration
            .provide(CalculatesMetric::<BuchholzOfBuchholz>::new())
            .require::<CalculatesMetric<Buchholz>>("the Buchholz")
            .require::<CalculatesMetric<Matches>>("the matches");
    }
}

#[register]
impl MetricsCalculator for BuchholzOfBuchholzCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(BuchholzOfBuchholz::default())
    }

    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: MatchResult,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        weighted_opponent_sum_changes::<Buchholz, BuchholzOfBuchholz>(
            phase_data,
            Some(r#match),
            old_metrics,
            |_| 1.,
        )
    }

    /// A bye has no opponent to count, but a change of the buchholz of its ranking counts for the
    /// opponents of that ranking.
    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        _: &Bye,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        weighted_opponent_sum_changes::<Buchholz, BuchholzOfBuchholz>(
            phase_data,
            None,
            old_metrics,
            |_| 1.,
        )
    }
}

#[derive(Default, Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct SonnebornBerger(pub f32);

#[typetag::serde]
impl TypeMapValue for SonnebornBerger {}

impl AsRef<f32> for SonnebornBerger {
    fn as_ref(&self) -> &f32 {
        &self.0
    }
}

impl From<f32> for SonnebornBerger {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

/// Ranks by the [`SonnebornBerger`], where more is better.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::more_sonneborn_berger",
    label = "more Sonneborn-Berger",
    declare_static = requires_metric::<Self>
)]
pub struct MoreSonnebornBerger;

impl RankingCriterion for MoreSonnebornBerger {
    type Metric = SonnebornBerger;

    fn compare(&self, first: &SonnebornBerger, second: &SonnebornBerger) -> Comparison {
        Comparison::greater_is_better(first.0.total_cmp(&second.0))
    }
}

#[register]
impl SliceRanker for MoreSonnebornBerger {
    fn update_ranks(&mut self, _: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]) {
        rank_by(self, slice);
    }
}

/// Offered with the classical weights: the points of a beaten opponent count fully, those of an
/// opponent that was played to a draw count half.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::sonneborn_berger",
    label = "Sonneborn-Berger",
    description = "the points of the opponents that were beaten or played to a draw",
    declare_static = Self::declare_static
)]
pub struct SonnebornBergerCalculator {
    #[plugin(
        label = "weight of a beaten opponent",
        description = "the points of that opponent are multiplied by this",
        default = 1.0
    )]
    weight_for_win: f32,
    #[plugin(
        label = "weight of an opponent played to a draw",
        description = "the points of that opponent are multiplied by this",
        default = 0.5
    )]
    weight_for_draw: f32,
}

impl SonnebornBergerCalculator {
    /// Counts the points of every beaten opponent with `weight_for_win` and the points of every
    /// opponent that was played to a draw with `weight_for_draw`, the points of an opponent that
    /// won are never counted.
    pub const fn new(weight_for_win: f32, weight_for_draw: f32) -> Self {
        Self {
            weight_for_win,
            weight_for_draw,
        }
    }

    fn declare_static(declaration: &mut Declaration) {
        declaration
            .provide(CalculatesMetric::<SonnebornBerger>::new())
            .require::<CalculatesMetric<Points>>("the points")
            .require::<CalculatesMetric<Matches>>("the matches");
    }

    fn weight(&self, result: ResultOfTeam) -> f32 {
        match result {
            ResultOfTeam::Win => self.weight_for_win,
            ResultOfTeam::Draw => self.weight_for_draw,
            ResultOfTeam::Loss | ResultOfTeam::NotFinished => 0.,
        }
    }
}

#[register]
impl MetricsCalculator for SonnebornBergerCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(SonnebornBerger::default())
    }

    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: MatchResult,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        weighted_opponent_sum_changes::<Points, SonnebornBerger>(
            phase_data,
            Some(r#match),
            old_metrics,
            |result| self.weight(result),
        )
    }

    /// A bye has no opponent to count, but the points it is worth count for the opponents that beat
    /// its ranking or played it to a draw.
    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        _: &Bye,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        weighted_opponent_sum_changes::<Points, SonnebornBerger>(
            phase_data,
            None,
            old_metrics,
            |result| self.weight(result),
        )
    }
}

/// The updates of the metric `B`, which sums the metric `M` of all opponents of a ranking, each
/// multiplied by the `weight` of the result the ranking achieved against that opponent, after the
/// result of `match` changed, or, without a match, after a bye was scheduled.
fn weighted_opponent_sum_changes<M, B>(
    phase_data: &PhaseData,
    r#match: Option<&Match>,
    old_metrics: &OldMetrics,
    weight: impl Fn(ResultOfTeam) -> f32,
) -> Vec<(RankingId, Metric)>
where
    M: TypeMapValue + AsRef<f32>,
    B: TypeMapValue + AsRef<f32> + From<f32>,
{
    let mut changes: Map<_, f32> = Map::default();
    for (r_id, old_metric) in old_metrics.of::<M>() {
        let ranking = &phase_data.rankings()[&r_id];
        let new_metric = ranking.metric::<M>().unwrap();
        let m_diff = *new_metric.as_ref() - *old_metric.as_ref();
        if m_diff != 0. {
            // `B` of all opponents of this ranking change, according to their result against it
            let matches = ranking.metric::<Matches>().unwrap();
            for match_id in &matches.0 {
                if r#match.is_some_and(|r#match| match_id == &r#match.id()) {
                    // we don't consider the newly reported match here as we will account for it
                    // later separately
                    continue;
                }
                let match_of_r = &phase_data.matches()[match_id];
                let opponent_team = match_of_r.team_of(&r_id).unwrap().opponent();
                let opponent_weight = weight(match_of_r.result_of_team(opponent_team));
                if opponent_weight != 0. {
                    let change = opponent_weight * m_diff;
                    for opponent in match_of_r.rankings_of(opponent_team) {
                        *changes.entry(*opponent).or_default() += change;
                    }
                }
            }
        }
    }
    // now also account for the new match
    for (r#match, team) in r#match
        .into_iter()
        .flat_map(|r#match| [(r#match, Team::A), (r#match, Team::B)])
    {
        let team_weight = weight(r#match.result_of_team(team));
        if team_weight == 0. {
            continue;
        }
        // add metric `M` of the opponent team, weighted, to `B` of team
        let m_sum: f32 = r#match
            .rankings_of(team.opponent())
            .iter()
            .map(|r_id| *phase_data.rankings()[r_id].metric::<M>().unwrap().as_ref())
            .sum();
        let change = team_weight * m_sum;
        for r_id in r#match.rankings_of(team) {
            *changes.entry(*r_id).or_default() += change;
        }
    }
    changes
        .into_iter()
        .map(|(r_id, change)| {
            let old_b = *phase_data.rankings()[&r_id].metric::<B>().unwrap().as_ref();
            (r_id, Box::new(B::from(old_b + change)) as Metric)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::Phase,
        plugins::metrics_calculator::{
            matches::MatchesCalculator,
            points::PointsCalculator,
            test_utils::{self, MatchSpec, match_id},
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
    };

    /// The scoring system of most association football leagues, used by every test here but
    /// [`the_neustadtl_scores_of_a_published_cross_table_are_reproduced`].
    const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);
    /// The scoring system of chess: a win is worth a point, a draw half of one.
    const ONE_HALF: PointsCalculator = PointsCalculator::new(1.0, 0.5);
    /// The classic Sonneborn-Berger weights: a beaten opponent counts fully, an opponent played to
    /// a draw counts half.
    const FULL_HALF: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);

    /// Builds a phase with `num_players` rankings and all matches of `matches` already scheduled,
    /// scoring finished matches with [`THREE_ONE`].
    ///
    /// [`BuchholzCalculator`] derives its metric from [`Points`] and [`Matches`], so the phase has
    /// to update both of them first, and [`Points`] in turn derives from
    /// [`WinsDrawsLosses`](super::wins_draws_losses::WinsDrawsLosses). Those are the only metrics
    /// it declares, so the phase calculates nothing else.
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

    /// Like [`phase_with`], with the buchholz of buchholz calculated as well, which reads the
    /// [`Buchholz`] and therefore runs last.
    fn phase_with_buchholz_of_buchholz(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
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

    /// Like [`phase_with_points`], scoring finished matches with [`THREE_ONE`].
    fn phase_with_sonneborn_berger(
        num_players: u32,
        matches: Vec<MatchSpec>,
        calculator: SonnebornBergerCalculator,
    ) -> Phase {
        phase_with_points(num_players, matches, THREE_ONE, calculator)
    }

    /// Builds a phase with `num_players` rankings and all matches of `matches` already scheduled,
    /// scoring finished matches with `points` and weighting them with `calculator`.
    ///
    /// [`SonnebornBergerCalculator`] derives its metric from [`Points`] and [`Matches`], so the
    /// phase has to update both of them first, and [`Points`] in turn derives from
    /// [`WinsDrawsLosses`](super::wins_draws_losses::WinsDrawsLosses).
    fn phase_with_points(
        num_players: u32,
        matches: Vec<MatchSpec>,
        points: PointsCalculator,
        calculator: SonnebornBergerCalculator,
    ) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(points),
                Box::new(MatchesCalculator),
                Box::new(calculator),
            ],
        )
    }

    fn buchholz_of(phase: &Phase, ranking_index: usize) -> Buchholz {
        test_utils::metric_of(phase, ranking_index)
    }

    fn buchholz_of_buchholz_of(phase: &Phase, ranking_index: usize) -> BuchholzOfBuchholz {
        test_utils::metric_of(phase, ranking_index)
    }

    fn sonneborn_berger_of(phase: &Phase, ranking_index: usize) -> SonnebornBerger {
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

    /// The matches of [`SCENARIO`], as the schedule of a phase playing it.
    fn scenario_matches() -> Vec<MatchSpec> {
        SCENARIO
            .iter()
            .map(|(round, team_a, team_b, _)| (*round, team_a.to_vec(), team_b.to_vec()))
            .collect()
    }

    /// Plays [`SCENARIO`] in `phase`, which has to be a phase of four players scheduled with
    /// [`scenario_matches`], reporting its matches in the given order. That leaves them with the
    /// following points: `0`: 3, `1`: 3, `2`: 7, `3`: 1.
    fn play_scenario(mut phase: Phase, report_order: impl Iterator<Item = usize>) -> Phase {
        for index in report_order {
            let (round, team_a, team_b, result) = SCENARIO[index];
            let match_id = match_id(&phase, round, team_a, team_b);
            phase.report_match_result(match_id, result);
        }
        phase
    }

    /// Plays [`SCENARIO`] in a phase calculating the buchholz, see [`play_scenario`].
    fn scenario(report_order: impl Iterator<Item = usize>) -> Phase {
        play_scenario(phase_with(4, scenario_matches()), report_order)
    }

    /// Plays [`SCENARIO`] in a phase calculating the buchholz of buchholz as well, see
    /// [`play_scenario`].
    fn scenario_with_buchholz_of_buchholz(report_order: impl Iterator<Item = usize>) -> Phase {
        play_scenario(
            phase_with_buchholz_of_buchholz(4, scenario_matches()),
            report_order,
        )
    }

    /// Plays [`SCENARIO`] in a phase calculating the Sonneborn-Berger with `calculator`, see
    /// [`play_scenario`].
    fn scenario_with_sonneborn_berger(
        calculator: SonnebornBergerCalculator,
        report_order: impl Iterator<Item = usize>,
    ) -> Phase {
        play_scenario(
            phase_with_sonneborn_berger(4, scenario_matches(), calculator),
            report_order,
        )
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
        let phase = scenario(0..SCENARIO.len());

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
        let phase = scenario((0..SCENARIO.len()).rev());

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

    /// `2` has a bye in round 1 and `1` one in round 2, each worth as much as a win, so that the
    /// bye of `1` is scheduled only after `0` faced it.
    #[test]
    fn the_points_of_a_bye_count_for_the_opponents_of_its_ranking() {
        let mut phase = test_utils::phase_with_byes(
            3,
            vec![(1, vec![0], vec![1]), (2, vec![0], vec![2])],
            vec![(1, 2), (2, 1)],
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE.with_points_for_bye(3.0)),
                Box::new(MatchesCalculator),
                Box::new(BuchholzCalculator),
            ],
        );

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::AWins);
        // the bye `1` got after its loss gave it three points, which count for `0` already
        assert_eq!(buchholz_of(&phase, 0), Buchholz(3.0));
        // a bye has no opponent whose points could count
        assert_eq!(buchholz_of(&phase, 1), Buchholz(3.0));

        let second_match = match_id(&phase, 2, &[0], &[2]);
        phase.report_match_result(second_match, MatchResult::Draw);

        // `0` has 4 points, `1` 3 and `2` 4
        assert_eq!(buchholz_of(&phase, 0), Buchholz(7.0));
        assert_eq!(buchholz_of(&phase, 1), Buchholz(4.0));
        assert_eq!(buchholz_of(&phase, 2), Buchholz(4.0));
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
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
        let compare = |first, second| MoreBuchholz.compare(&Buchholz(first), &Buchholz(second));
        assert_eq!(compare(7.0, 3.0), Comparison::Better);
        assert_eq!(compare(3.0, 7.0), Comparison::Worse);
        assert_eq!(compare(0.5, 0.0), Comparison::Better);
        assert_eq!(compare(3.0, 3.0), Comparison::Equal);
    }

    #[test]
    fn initialize_metrics_starts_every_buchholz_of_buchholz_at_zero() {
        let phase = phase_with_buchholz_of_buchholz(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(
                buchholz_of_buchholz_of(&phase, ranking_index),
                BuchholzOfBuchholz::default()
            );
        }
    }

    #[test]
    fn the_buchholz_of_buchholz_of_a_ranking_is_the_sum_of_the_buchholz_of_its_opponents() {
        let phase = scenario_with_buchholz_of_buchholz(0..SCENARIO.len());

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
        let phase = scenario_with_buchholz_of_buchholz((0..SCENARIO.len()).rev());

        assert_eq!(buchholz_of_buchholz_of(&phase, 0), BuchholzOfBuchholz(18.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 1), BuchholzOfBuchholz(27.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 2), BuchholzOfBuchholz(31.0));
        assert_eq!(buchholz_of_buchholz_of(&phase, 3), BuchholzOfBuchholz(18.0));
    }

    #[test]
    fn buchholz_an_opponent_gains_later_is_added_retroactively() {
        let mut phase =
            phase_with_buchholz_of_buchholz(3, vec![(1, vec![0], vec![1]), (2, vec![1], vec![2])]);

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
        let mut phase = phase_with_buchholz_of_buchholz(4, matches);

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
        let mut phase = phase_with_buchholz_of_buchholz(4, matches);

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
        let mut phase = phase_with_buchholz_of_buchholz(4, matches);

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
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
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
        let compare = |first, second| {
            MoreBuchholzOfBuchholz.compare(&BuchholzOfBuchholz(first), &BuchholzOfBuchholz(second))
        };
        assert_eq!(compare(7.0, 3.0), Comparison::Better);
        assert_eq!(compare(3.0, 7.0), Comparison::Worse);
        assert_eq!(compare(0.5, 0.0), Comparison::Better);
        assert_eq!(compare(3.0, 3.0), Comparison::Equal);
    }

    #[test]
    fn initialize_metrics_starts_every_sonneborn_berger_at_zero() {
        let phase = phase_with_sonneborn_berger(4, vec![], FULL_HALF);

        for ranking_index in 0..4 {
            assert_eq!(
                sonneborn_berger_of(&phase, ranking_index),
                SonnebornBerger(0.0)
            );
        }
    }

    #[test]
    fn beaten_opponents_count_fully_and_opponents_of_a_draw_count_half() {
        let phase = scenario_with_sonneborn_berger(FULL_HALF, 0..SCENARIO.len());

        // `0` beat `1` (3 points) and lost against `2`
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(3.0));
        // `1` beat `3` (1 point) and lost against `0` and `2`
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(1.0));
        // `2` beat `0` (3) and `1` (3) and drew against `3` (1)
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(6.5));
        // `3` drew against `2` (7) and lost against `1`
        assert_eq!(sonneborn_berger_of(&phase, 3), SonnebornBerger(3.5));
    }

    #[test]
    fn the_order_in_which_results_are_reported_does_not_change_the_sonneborn_berger() {
        let phase = scenario_with_sonneborn_berger(FULL_HALF, (0..SCENARIO.len()).rev());

        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(3.0));
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(1.0));
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(6.5));
        assert_eq!(sonneborn_berger_of(&phase, 3), SonnebornBerger(3.5));
    }

    #[test]
    fn points_a_beaten_opponent_wins_later_are_added_retroactively() {
        let mut phase = phase_with_sonneborn_berger(
            3,
            vec![(1, vec![0], vec![1]), (2, vec![1], vec![2])],
            FULL_HALF,
        );

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::AWins);
        // `1` has no points yet, so beating it is worth nothing so far
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(0.0));

        let second_match = match_id(&phase, 2, &[1], &[2]);
        phase.report_match_result(second_match, MatchResult::AWins);

        // the three points `1` won in its second match count for `0` as well
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(3.0));
        // `1` beat `2`, which is still without points
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(0.0));
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(0.0));
    }

    #[test]
    fn points_an_opponent_of_a_draw_wins_later_are_added_retroactively_by_half() {
        let mut phase = phase_with_sonneborn_berger(
            3,
            vec![(1, vec![0], vec![1]), (2, vec![1], vec![2])],
            FULL_HALF,
        );

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::Draw);
        // the draw gave `1` a single point, half of which counts for `0`
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(0.5));

        let second_match = match_id(&phase, 2, &[1], &[2]);
        phase.report_match_result(second_match, MatchResult::AWins);

        // `1` is now at four points, still counted half by `0`
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(2.0));
        // `1` drew against `0` (1 point) and beat `2` (no points)
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(0.5));
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(0.0));
    }

    #[test]
    fn the_points_of_an_opponent_that_won_are_never_counted() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (2, vec![0], vec![1]),
            (3, vec![0], vec![1]),
        ];
        let mut phase = phase_with_sonneborn_berger(2, matches, FULL_HALF);

        for (round, result) in [
            (1, MatchResult::AWins),
            (2, MatchResult::AWins),
            (3, MatchResult::BWins),
        ] {
            let match_id = match_id(&phase, round, &[0], &[1]);
            phase.report_match_result(match_id, result);
        }

        // `0` won twice against `1` (3 points), its loss adds nothing
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(6.0));
        // `1` won once against `0` (6 points), its two losses add nothing
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(6.0));
    }

    #[test]
    fn the_weights_scale_the_points_of_the_opponents() {
        let phase = scenario_with_sonneborn_berger(
            SonnebornBergerCalculator::new(2.0, 1.0),
            0..SCENARIO.len(),
        );

        // `0` beat `1` (3 points)
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(6.0));
        // `1` beat `3` (1)
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(2.0));
        // `2` beat `0` (3) and `1` (3) and drew against `3` (1)
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(13.0));
        // `3` drew against `2` (7)
        assert_eq!(sonneborn_berger_of(&phase, 3), SonnebornBerger(7.0));
    }

    #[test]
    fn a_draw_weight_of_zero_counts_beaten_opponents_only() {
        let phase = scenario_with_sonneborn_berger(
            SonnebornBergerCalculator::new(1.0, 0.0),
            0..SCENARIO.len(),
        );

        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(3.0));
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(1.0));
        // the draw of `2` against `3` no longer contributes
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(6.0));
        // `3` only ever drew, so it stays at zero
        assert_eq!(sonneborn_berger_of(&phase, 3), SonnebornBerger(0.0));
    }

    #[test]
    fn only_opponents_of_reported_matches_count_for_the_sonneborn_berger() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (1, vec![2], vec![3]),
            (2, vec![0], vec![2]),
            (2, vec![1], vec![3]),
        ];
        let mut phase = phase_with_sonneborn_berger(4, matches, FULL_HALF);

        for (round, team_a, team_b) in [(1, [0], [1]), (1, [2], [3]), (2, [1], [3])] {
            let match_id = match_id(&phase, round, &team_a, &team_b);
            phase.report_match_result(match_id, MatchResult::AWins);
        }

        // `0` beat `1`, which is at three points by now
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(3.0));
        // `1` beat `3`, which is without points
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(0.0));
        // the scheduled but unreported match of `0` against `2` contributes nothing, even though
        // `2` won its first match and `0` has three points
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(0.0));
        assert_eq!(sonneborn_berger_of(&phase, 3), SonnebornBerger(0.0));
    }

    #[test]
    fn every_ranking_of_a_team_counts_the_weighted_points_of_every_opponent_ranking() {
        let matches = vec![(1, vec![0, 1], vec![2, 3]), (2, vec![0, 1], vec![2, 3])];
        let mut phase = phase_with_sonneborn_berger(4, matches, FULL_HALF);

        for (round, result) in [(1, MatchResult::AWins), (2, MatchResult::Draw)] {
            let match_id = match_id(&phase, round, &[0, 1], &[2, 3]);
            phase.report_match_result(match_id, result);
        }

        // `2` and `3` end up with one point each, counted fully for the won match and half for the
        // drawn one
        for ranking_index in [0, 1] {
            assert_eq!(
                sonneborn_berger_of(&phase, ranking_index),
                SonnebornBerger(3.0)
            );
        }
        // `0` and `1` end up with four points each, of which only the drawn match counts, by half
        for ranking_index in [2, 3] {
            assert_eq!(
                sonneborn_berger_of(&phase, ranking_index),
                SonnebornBerger(4.0)
            );
        }
    }

    /// The cross table of the 8th World Correspondence Chess Championship that the Wikipedia
    /// article on the Neustadtl score uses as its example, where row `i` lists the results of
    /// player `i` against every other player: `1` a win, `=` a draw, `0` a loss and `-` the
    /// player itself.
    const CROSS_TABLE: [&str; 15] = [
        "-==1==11=1=1111", //  1 Sloth
        "=-0=1=111=11111", //  2 Zagorovsky
        "=1-=====11=1111", //  3 Kosenkov
        "0==-=1=011=1=1=", //  4 Khasin
        "=0==-====011=11", //  5 Kletsel
        "===0=-==01==011", //  6 De Carbonnel
        "00====-=10==11=", //  7 Arnlind
        "00=1===-0=101=1", //  8 Dunhaupt
        "=000=101-1====1", //  9 Maedler
        "0=00101=0-11101", // 10 Estrin
        "=0==0==0=0-01=1", // 11 Walther
        "00000==1=01-==1", // 12 Boey
        "000==100=00=-=1", // 13 Abramov
        "0000000==1===-1", // 14 Siklos
        "000=00=0000000-", // 15 Nun
    ];

    /// The points and the Neustadtl scores that the article lists for [`CROSS_TABLE`], in the
    /// order of its rows.
    const PUBLISHED_SCORES: [(f32, f32); 15] = [
        (11.0, 69.5),
        (11.0, 66.75),
        (10.5, 67.5),
        (8.5, 54.75),
        (8.0, 47.75),
        (7.0, 45.25),
        (7.0, 42.5),
        (7.0, 41.5),
        (7.0, 41.5),
        (7.0, 40.5),
        (5.5, 33.25),
        (5.5, 28.5),
        (4.5, 24.75),
        (4.5, 22.75),
        (1.0, 7.75),
    ];

    /// The Neustadtl score of chess is the Sonneborn-Berger of [`FULL_HALF`] over the points of
    /// [`ONE_HALF`], so a full round robin has to reproduce the published scores exactly.
    #[test]
    fn the_neustadtl_scores_of_a_published_cross_table_are_reproduced() {
        let players = CROSS_TABLE.len();
        // the circle method for an odd number of players: player `a` meets player `b` in round
        // `a + b` modulo the number of players, which pairs every two players exactly once and
        // gives the player with `2 * a == a + b` a bye in that round
        let round = |a: usize, b: usize| ((a + b) % players) as u32 + 1;
        let matches = (0..players)
            .flat_map(|a| (a + 1..players).map(move |b| (round(a, b), vec![a], vec![b])))
            .collect();
        let mut phase = phase_with_points(players as u32, matches, ONE_HALF, FULL_HALF);

        for (a, row) in CROSS_TABLE.iter().enumerate() {
            for (b, result) in row.chars().enumerate().skip(a + 1) {
                let mirrored = CROSS_TABLE[b].as_bytes()[a] as char;
                let (result, expected_mirrored) = match result {
                    '1' => (MatchResult::AWins, '0'),
                    '=' => (MatchResult::Draw, '='),
                    '0' => (MatchResult::BWins, '1'),
                    other => panic!("unexpected result {other} of player {a} against player {b}"),
                };
                assert_eq!(
                    mirrored, expected_mirrored,
                    "the two halves of the cross table disagree about player {a} against player {b}"
                );
                let match_id = match_id(&phase, round(a, b), &[a], &[b]);
                phase.report_match_result(match_id, result);
            }
        }

        for (index, (points, neustadtl)) in PUBLISHED_SCORES.into_iter().enumerate() {
            assert_eq!(
                test_utils::metric_of::<Points>(&phase, index),
                Points(points),
                "player {index} ended up with the wrong number of points"
            );
            assert_eq!(
                sonneborn_berger_of(&phase, index),
                SonnebornBerger(neustadtl),
                "player {index} ended up with the wrong Neustadtl score"
            );
        }
    }

    /// `2` has a bye in round 1 and `1` one in round 2, each worth as much as a win, so that the
    /// bye of `1` is scheduled only after `0` beat it.
    #[test]
    fn the_points_of_a_bye_count_for_the_opponents_that_beat_its_ranking() {
        let mut phase = test_utils::phase_with_byes(
            3,
            vec![(1, vec![0], vec![1]), (2, vec![0], vec![2])],
            vec![(1, 2), (2, 1)],
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE.with_points_for_bye(3.0)),
                Box::new(MatchesCalculator),
                Box::new(FULL_HALF),
            ],
        );

        let first_match = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(first_match, MatchResult::AWins);
        // the bye `1` got after its loss gave it three points, which count for `0` already
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(3.0));

        let second_match = match_id(&phase, 2, &[0], &[2]);
        phase.report_match_result(second_match, MatchResult::Draw);

        // `0` beat `1` (3 points) and drew against `2` (4 points)
        assert_eq!(sonneborn_berger_of(&phase, 0), SonnebornBerger(5.0));
        // `1` lost its only match, and a bye has no opponent whose points could count
        assert_eq!(sonneborn_berger_of(&phase, 1), SonnebornBerger(0.0));
        // `2` drew against `0` (4 points)
        assert_eq!(sonneborn_berger_of(&phase, 2), SonnebornBerger(2.0));
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn calculating_the_sonneborn_berger_without_the_points_metric_panics() {
        let mut phase = test_utils::phase_with(
            2,
            vec![(1, vec![0], vec![1])],
            vec![Box::new(MatchesCalculator), Box::new(FULL_HALF)],
        );
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);
    }

    #[test]
    fn a_higher_sonneborn_berger_is_better() {
        let compare = |first, second| {
            MoreSonnebornBerger.compare(&SonnebornBerger(first), &SonnebornBerger(second))
        };
        assert_eq!(compare(7.0, 3.0), Comparison::Better);
        assert_eq!(compare(3.0, 7.0), Comparison::Worse);
        assert_eq!(compare(0.5, 0.0), Comparison::Better);
        assert_eq!(compare(3.0, 3.0), Comparison::Equal);
    }
}
