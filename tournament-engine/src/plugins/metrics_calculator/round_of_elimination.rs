use std::num::NonZeroU32;

use plugin_factory::{Plugin, compatibility::Declaration, register};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Bye, Match, MatchResult, PhaseData, Ranking, RankingId, Team},
    plugins::{
        facts::{CalculatesMetric, MatchAnnotation},
        metrics_calculator::{Metric, MetricsCalculator, OldMetrics},
        ranker::{Compare, Comparison, RankingCriterion, SliceRanker, rank_by, requires_metric},
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
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::later_elimination",
    label = "later elimination",
    description = "by the round of elimination; still being in is best",
    declare_static = requires_metric::<Self>
)]
pub struct LaterElimination;

impl RankingCriterion for LaterElimination {
    type Metric = RoundOfElimination;

    fn compare(&self, first: &RoundOfElimination, second: &RoundOfElimination) -> Comparison {
        first.compare(second)
    }
}

#[register]
impl SliceRanker for LaterElimination {
    fn update_ranks(&mut self, _: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]) {
        rank_by(self, slice);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::round_of_elimination",
    label = "round of elimination",
    description = "the round in which an entrant was knocked out",
    declare_static = Self::declare_static
)]
pub struct RoundOfEliminationCalculator;

impl RoundOfEliminationCalculator {
    fn declare_static(declaration: &mut Declaration) {
        declaration
            .provide(CalculatesMetric::<RoundOfElimination>::new())
            .require::<MatchAnnotation<Eliminates>>("an elimination annotation on the matches");
    }
}

#[register]
impl MetricsCalculator for RoundOfEliminationCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(RoundOfElimination::default())
    }

    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        old_match_result: MatchResult,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        // for now we only support setting the match result, not updating it
        assert_eq!(old_match_result, MatchResult::NotFinished);

        let Some(eliminates) = r#match.annotation::<Eliminates>() else {
            return vec![];
        };
        let Some(loser) = r#match.result().loser() else {
            return vec![];
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

    /// A bye knocks out nobody.
    fn update_after_bye(
        &mut self,
        _: &PhaseData,
        _: &Bye,
        _: &OldMetrics,
    ) -> Vec<(RankingId, Metric)> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::Phase,
        plugins::metrics_calculator::test_utils::{self, AnnotatedMatchSpec, MatchSpec, match_id},
        type_map::TypeMap,
    };

    /// A ranking that is still in the phase, which is the best this metric gets.
    const STILL_IN: RoundOfElimination = RoundOfElimination(None);

    /// Builds a phase with `num_players` rankings and all matches of `matches` already scheduled,
    /// each carrying the annotations given next to its schedule.
    ///
    /// [`RoundOfEliminationCalculator`] reads nothing but the [`Eliminates`] annotation and the
    /// result of the match it is called for, so it is the only calculator of the phase.
    fn phase_with(num_players: u32, matches: Vec<AnnotatedMatchSpec>) -> Phase {
        test_utils::phase_with_annotated_matches(
            num_players,
            matches,
            vec![],
            vec![Box::new(RoundOfEliminationCalculator)],
        )
    }

    /// `r#match` as a match that knocks out whoever `eliminates` names.
    fn eliminating(r#match: MatchSpec, eliminates: Eliminates) -> AnnotatedMatchSpec {
        let mut annotations = TypeMap::new();
        annotations.insert(eliminates);
        (r#match, annotations)
    }

    /// `r#match` as a match whose loser is out, whichever side it is.
    fn eliminating_the_loser(r#match: MatchSpec) -> AnnotatedMatchSpec {
        eliminating(r#match, Eliminates::Loser)
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> RoundOfElimination {
        test_utils::metric_of(phase, ranking_index)
    }

    /// The metric of a ranking that was knocked out in round `round`.
    fn out_in(round: u32) -> RoundOfElimination {
        RoundOfElimination(NonZeroU32::new(round))
    }

    /// Reports `result` for the match of `round` between the two single-ranking teams.
    fn report(phase: &mut Phase, round: u32, result: MatchResult) {
        let match_id = match_id(phase, round, &[0], &[1]);
        phase.report_match_result(match_id, result);
    }

    #[test]
    fn initialize_metrics_leaves_every_ranking_still_in() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), STILL_IN);
        }
    }

    #[test]
    fn the_loser_of_a_match_that_eliminates_is_out_in_the_round_of_that_match() {
        let mut phase = phase_with(2, vec![eliminating_the_loser((3, vec![0], vec![1]))]);

        report(&mut phase, 3, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), STILL_IN);
        assert_eq!(metric_of(&phase, 1), out_in(3));
    }

    /// A phase whose format does not knock anybody out — a round robin, or the winner bracket of a
    /// double elimination — leaves every ranking in.
    #[test]
    fn a_match_without_the_annotation_knocks_nobody_out() {
        let mut phase = phase_with(2, vec![((1, vec![0], vec![1]), TypeMap::new())]);

        report(&mut phase, 1, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), STILL_IN);
        assert_eq!(metric_of(&phase, 1), STILL_IN);
    }

    #[test]
    fn a_draw_knocks_nobody_out() {
        let mut phase = phase_with(2, vec![eliminating_the_loser((1, vec![0], vec![1]))]);

        report(&mut phase, 1, MatchResult::Draw);

        assert_eq!(metric_of(&phase, 0), STILL_IN);
        assert_eq!(metric_of(&phase, 1), STILL_IN);
    }

    /// The grand final of a double elimination with a reset to come: side B is out if it loses,
    /// side A plays the reset instead.
    #[test]
    fn a_match_that_eliminates_one_side_leaves_the_other_in_when_it_loses() {
        for (result, expected) in [
            (MatchResult::AWins, out_in(1)),
            (MatchResult::BWins, STILL_IN),
        ] {
            let mut phase = phase_with(
                2,
                vec![eliminating(
                    (1, vec![0], vec![1]),
                    Eliminates::Only(Team::B),
                )],
            );

            report(&mut phase, 1, result);

            assert_eq!(metric_of(&phase, 0), STILL_IN, "side A after {result:?}");
            assert_eq!(metric_of(&phase, 1), expected, "side B after {result:?}");
        }
    }

    #[test]
    fn every_ranking_of_a_losing_team_is_knocked_out() {
        let mut phase = phase_with(4, vec![eliminating_the_loser((1, vec![0, 1], vec![2, 3]))]);
        let match_id = match_id(&phase, 1, &[0, 1], &[2, 3]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), out_in(1));
        assert_eq!(metric_of(&phase, 1), out_in(1));
        assert_eq!(metric_of(&phase, 2), STILL_IN);
        assert_eq!(metric_of(&phase, 3), STILL_IN);
    }

    #[test]
    fn a_ranking_that_is_already_out_keeps_the_round_it_was_knocked_out_in() {
        let mut phase = phase_with(
            2,
            vec![
                eliminating_the_loser((1, vec![0], vec![1])),
                eliminating_the_loser((2, vec![0], vec![1])),
            ],
        );

        report(&mut phase, 1, MatchResult::AWins);
        report(&mut phase, 2, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 1), out_in(1));
    }

    #[test]
    fn a_bye_knocks_nobody_out() {
        let phase = test_utils::phase_with_byes(
            2,
            vec![],
            vec![(1, 0)],
            vec![Box::new(RoundOfEliminationCalculator)],
        );

        assert_eq!(phase.data().byes().len(), 1);
        assert_eq!(metric_of(&phase, 0), STILL_IN);
    }

    #[test]
    fn a_later_elimination_is_better_and_still_being_in_is_best() {
        assert_eq!(STILL_IN.compare(&out_in(3)), Comparison::Better);
        assert_eq!(out_in(3).compare(&STILL_IN), Comparison::Worse);
        assert_eq!(out_in(3).compare(&out_in(2)), Comparison::Better);
        assert_eq!(out_in(2).compare(&out_in(3)), Comparison::Worse);
        assert_eq!(out_in(2).compare(&out_in(2)), Comparison::Equal);
        assert_eq!(STILL_IN.compare(&STILL_IN), Comparison::Equal);
    }

    #[test]
    fn the_criterion_ranks_by_the_round_of_elimination() {
        assert_eq!(
            LaterElimination.compare(&STILL_IN, &out_in(1)),
            Comparison::Better
        );
    }
}
