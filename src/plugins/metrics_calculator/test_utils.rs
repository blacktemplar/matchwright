//! Shared test-only helpers for unit testing [`MetricsCalculator`] implementations against a
//! standalone [`Phase`] with a fixed, hand written schedule of matches.

use std::num::NonZeroU32;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use vec1::Vec1;

use crate::{
    entities::phase::{Match, MatchId, Phase, PhaseBuilder, RankingId},
    plugins::{metrics_calculator::MetricsCalculator, phase_format::PhaseFormat},
};

/// A match scheduled by [`FixedMatches`], given as its round and its two teams, where a team is
/// described by the indices of its rankings ordered by initial unique rank.
pub(crate) type MatchSpec = (u32, Vec<usize>, Vec<usize>);

/// Test-only [`PhaseFormat`] that schedules a fixed list of matches, all at once.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixedMatches(Vec<MatchSpec>);

#[typetag::serde]
impl PhaseFormat for FixedMatches {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match> {
        if !phase.matches().is_empty() {
            // we generate all matches at once, so as soon as one match exists nothing is
            // to be done
            return vec![];
        }
        self.0
            .iter()
            .map(|(round, team_a, team_b)| {
                Match::new(
                    team(phase, team_a),
                    team(phase, team_b),
                    NonZeroU32::new(*round).unwrap(),
                )
            })
            .collect()
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
    let mut builder = PhaseBuilder::new(Box::new(FixedMatches(matches))).with_players(num_players);
    for metrics_calculator in metrics_calculators {
        builder = builder.metrics_calculator(metrics_calculator);
    }
    let mut phase = builder.build();
    phase.create_new_matches();
    phase
}

fn ranking_ids(phase: &Phase) -> Vec<RankingId> {
    phase
        .rankings()
        .values()
        .sorted_by_key(|r| r.initial_unique_rank())
        .map(|r| r.id())
        .collect()
}

fn team(phase: &Phase, ranking_indices: &[usize]) -> Vec1<RankingId> {
    let ranking_ids = ranking_ids(phase);
    Vec1::try_from_vec(ranking_indices.iter().map(|i| ranking_ids[*i]).collect())
        .expect("a team must consist of at least one ranking")
}

/// The id of the scheduled match between the two given teams, recomputed from the fact that
/// [`Match::new`] derives ids deterministically from the teams and the round.
pub(crate) fn match_id(phase: &Phase, round: u32, team_a: &[usize], team_b: &[usize]) -> MatchId {
    let match_id = Match::new(
        team(phase, team_a),
        team(phase, team_b),
        NonZeroU32::new(round).unwrap(),
    )
    .id();
    assert!(phase.matches().contains_key(&match_id));
    match_id
}

/// The metric of type `M` of the ranking with the given index in the ordering by initial unique
/// rank.
pub(crate) fn metric_of<M: Copy + 'static>(phase: &Phase, ranking_index: usize) -> M {
    *metric_ref_of(phase, ranking_index)
}

/// Like [`metric_of`], but for metrics that are not [`Copy`].
pub(crate) fn metric_ref_of<M: 'static>(phase: &Phase, ranking_index: usize) -> &M {
    phase
        .metric_of::<M>(&ranking_ids(phase)[ranking_index])
        .expect("the metric must be initialized")
}
