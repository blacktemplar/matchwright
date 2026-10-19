//! Shared test-only helpers for unit testing [`MetricsCalculator`] implementations against a
//! standalone [`Phase`] with a fixed, hand written schedule of matches.

use std::num::NonZeroU32;

use itertools::Itertools;
use plugin_factory::{Plugin, register};
use serde::{Deserialize, Serialize};
use vec1::Vec1;

use crate::{
    entities::competition::{
        Annotated, Bye, Match, MatchId, Phase, PhaseBuilder, PhaseData, RankingId,
    },
    plugins::{
        metrics_calculator::MetricsCalculator,
        phase_format::{PhaseFormat, Schedule},
    },
    type_map::TypeMap,
};

/// A match scheduled by [`FixedMatches`], given as its round and its two teams, where a team is
/// described by the indices of its rankings ordered by initial unique rank.
pub(crate) type MatchSpec = (u32, Vec<usize>, Vec<usize>);

/// A match scheduled by [`FixedMatches`] together with the annotations it carries, as a format
/// annotating the matches it schedules writes them, e.g. the [`Eliminates`] of a knockout.
///
/// [`Eliminates`]: super::round_of_elimination::Eliminates
pub(crate) type AnnotatedMatchSpec = (MatchSpec, TypeMap);

/// A bye scheduled by [`FixedMatches`], given as its round and the index of its ranking ordered by
/// initial unique rank.
pub(crate) type ByeSpec = (u32, usize);

/// Test-only [`PhaseFormat`] that schedules a fixed list of matches all at once, and a fixed list
/// of byes, each as soon as every match of the rounds before its own is finished.
///
/// Its schedule is not configurable, so it is left out of its schema.
#[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "tournament_engine::test_utils::fixed_matches",
    label = "fixed matches"
)]
struct FixedMatches {
    #[serde(default)]
    #[plugin(skip)]
    matches: Vec<AnnotatedMatchSpec>,
    #[serde(default)]
    #[plugin(skip)]
    byes: Vec<ByeSpec>,
}

#[register]
impl PhaseFormat for FixedMatches {
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule {
        let all_matches: Vec<_> = self
            .matches
            .iter()
            .map(|((round, team_a, team_b), annotations)| {
                Match::new(
                    team(phase_data, team_a),
                    team(phase_data, team_b),
                    NonZeroU32::new(*round).unwrap(),
                )
                .with_annotations(annotations.clone())
            })
            .collect();
        let ranking_ids = ranking_ids(phase_data);
        let byes = self
            .byes
            .iter()
            .map(|(round, ranking)| {
                Bye::new(ranking_ids[*ranking], NonZeroU32::new(*round).unwrap())
            })
            .filter(|bye| {
                !phase_data.byes().contains_key(&bye.id())
                    && all_matches
                        .iter()
                        .filter(|r#match| r#match.round() < bye.round())
                        .all(|r#match| {
                            phase_data
                                .matches()
                                .get(&r#match.id())
                                .is_some_and(Match::is_finished)
                        })
            })
            .collect();
        let matches = if phase_data.matches().is_empty() {
            all_matches
        } else {
            vec![]
        };
        Schedule { matches, byes }
    }
}

/// Builds a phase with `num_players` rankings whose metrics got initialized by
/// `metrics_calculators` and with all matches of `matches` already scheduled.
///
/// The calculators are run in the given order, so a calculator that reads the metrics of another
/// one has to be listed after it.
pub(crate) fn phase_with(
    num_players: u32,
    matches: Vec<MatchSpec>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
) -> Phase {
    phase_with_byes(num_players, matches, vec![], metrics_calculators)
}

/// Like [`phase_with`], with the byes of `byes` scheduled as well: those of a round once every
/// match of the rounds before it is reported, so the byes of the first round right away.
pub(crate) fn phase_with_byes(
    num_players: u32,
    matches: Vec<MatchSpec>,
    byes: Vec<ByeSpec>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
) -> Phase {
    let matches = matches
        .into_iter()
        .map(|r#match| (r#match, TypeMap::new()))
        .collect();
    phase_with_annotated_matches(num_players, matches, byes, metrics_calculators)
}

/// Like [`phase_with_byes`], where every match carries the annotations given next to its schedule,
/// which is what a calculator reading a match annotation needs.
pub(crate) fn phase_with_annotated_matches(
    num_players: u32,
    matches: Vec<AnnotatedMatchSpec>,
    byes: Vec<ByeSpec>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
) -> Phase {
    let mut builder =
        PhaseBuilder::new(Box::new(FixedMatches { matches, byes })).with_players(num_players);
    for metrics_calculator in metrics_calculators {
        builder = builder.metrics_calculator(metrics_calculator);
    }
    let mut phase = builder.build();
    phase.create_new_matches();
    phase
}

fn ranking_ids(phase_data: &PhaseData) -> Vec<RankingId> {
    phase_data
        .rankings()
        .values()
        .sorted_by_key(|r| r.initial_unique_rank())
        .map(|r| r.id())
        .collect()
}

fn team(phase_data: &PhaseData, ranking_indices: &[usize]) -> Vec1<RankingId> {
    let ranking_ids = ranking_ids(phase_data);
    Vec1::try_from_vec(ranking_indices.iter().map(|i| ranking_ids[*i]).collect())
        .expect("a team must consist of at least one ranking")
}

/// The id of the scheduled match between the two given teams, recomputed from the fact that
/// [`Match::new`] derives ids deterministically from the teams and the round.
pub(crate) fn match_id(phase: &Phase, round: u32, team_a: &[usize], team_b: &[usize]) -> MatchId {
    let match_id = Match::new(
        team(phase.data(), team_a),
        team(phase.data(), team_b),
        NonZeroU32::new(round).unwrap(),
    )
    .id();
    assert!(phase.data().matches().contains_key(&match_id));
    match_id
}

/// The metric of type `M` of the ranking with the given index in the ordering by initial unique
/// rank.
pub(crate) fn metric_of<M: Copy + 'static>(phase: &Phase, ranking_index: usize) -> M {
    *metric_ref_of(phase, ranking_index)
}

/// Like [`metric_of`], but for metrics that are not [`Copy`].
pub(crate) fn metric_ref_of<M: 'static>(phase: &Phase, ranking_index: usize) -> &M {
    phase.data().rankings()[&ranking_ids(phase.data())[ranking_index]]
        .metric::<M>()
        .expect("the metric must be initialized")
}
