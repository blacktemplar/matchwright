use std::{
    any::{Any, TypeId},
    hash::Hash,
    num::NonZeroU32,
};

use itertools::{Either, Itertools};
use rand::{SeedableRng, rngs::StdRng};
use rand_seeder::Seeder;
use serde::{Deserialize, Serialize};
use uuid::{Uuid, uuid};
use vec1::Vec1;

#[cfg(test)]
use crate::entities::rankings::Ranking;
use crate::{
    entities::{
        competition::EntrantId,
        rankings::{RankingId, RankingView, Rankings},
    },
    map::{IdAble, IdMap, Map},
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator},
        phase_format::PhaseFormat,
        ranker::Ranker,
    },
};

/// Namespaces for various deterministic ids
const NAMESPACE_MATCH: Uuid = uuid!("d4098937-85f4-49ce-945a-be8ae74da780");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhaseId(Uuid);

impl PhaseId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// One stage of a competition: the [`Rankings`] it is played between, the [matches](Match) they
/// play, and the plugins that drive both. Its [`PhaseFormat`] schedules the matches, its [`Ranker`]
/// turns the metrics of its rankings into ranks, and the metrics calculators of those rankings are
/// what keeps the metrics up to date.
///
/// A phase is created empty and gets its rankings and its first matches once the competition it
/// belongs to starts. From then on every reported match result updates the metrics of its rankings,
/// reranks them and schedules whatever matches the format wants next.
#[derive(Debug, Serialize, Deserialize)]
pub struct Phase {
    id: PhaseId,
    outgoing_phase_transitions: Vec<PhaseTransitionId>,
    incoming_phase_transitions: Vec<PhaseTransitionId>,
    rankings: Rankings,
    matches: IdMap<Match>,
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    seed: [u8; 12],
}

impl IdAble for Phase {
    type Id = PhaseId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl Phase {
    /// Creates an empty phase, which gets its rankings and matches once the competition it belongs
    /// to [starts](crate::entities::competition::Competition::start).
    pub(crate) fn new(
        id: PhaseId,
        phase_format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
        seed: [u8; 12],
    ) -> Self {
        Self {
            id,
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings: Rankings::new(metrics_calculators),
            matches: IdMap::new(),
            phase_format,
            ranker,
            seed,
        }
    }

    pub fn id(&self) -> PhaseId {
        self.id
    }

    pub fn ranking_view(&self, ranking_id: &RankingId) -> RankingView<'_> {
        self.rankings.ranking_view(ranking_id)
    }

    pub fn ranking_views(&self) -> impl ExactSizeIterator<Item = RankingView<'_>> {
        self.rankings.ranking_views()
    }

    pub fn ranking_ids_ordered_by_unique_rank(&self) -> &[RankingId] {
        self.rankings.ranking_ids_ordered_by_unique_rank()
    }

    pub fn matches(&self) -> &IdMap<Match> {
        &self.matches
    }

    pub(crate) fn start(&mut self, ranking_entrants: Vec<Vec1<EntrantId>>) {
        self.rankings.initialize(ranking_entrants);
        self.create_new_matches();
    }

    pub(crate) fn create_new_matches(&mut self) {
        let new_matches = self.phase_format.create_new_matches(self);
        // check requirements of create_new_matches
        debug_assert!(
            new_matches
                .iter()
                .all(|m| { m.rankings().all(|r| self.rankings.contains(r)) })
        );
        let num_matches_before = self.matches.len();
        let num_new_matches = new_matches.len();
        self.matches.extend(new_matches);
        // This guarantees that all new match ids were unique (at least relative to the existing
        // match ids in this phase).
        debug_assert_eq!(self.matches.len(), num_matches_before + num_new_matches);
        // check that ranking ids are unique within a round
        debug_assert!(
            self.matches()
                .values()
                .flat_map(|m| m.rankings().map(|r| (m.round, r)))
                .all_unique()
        );
    }

    pub(crate) fn report_match_result(&mut self, match_id: MatchId, match_result: MatchResult) {
        let r#match = self.matches.get_mut(&match_id).unwrap();
        let old_result = r#match.result();
        assert!(old_result == MatchResult::NotFinished);
        r#match.set_result(match_result);
        let r#match = &self.matches[&match_id];
        // update ranking metrics
        let mut old_metrics: Map<TypeId, Vec<(RankingId, Metric)>> = Map::default();
        // Iterated by index rather than by iterator: an iterator would keep the calculators
        // borrowed for the whole loop, which rules out the `set_metric` below, while indexing
        // borrows them only for the call, which returns owned updates. That the indices stay
        // meaningful across the loop is what `set_metric` guarantees by not touching the
        // calculators.
        for index in 0..self.rankings.metrics_calculators().len() {
            let updates = self.rankings.metrics_calculators()[index]
                .update_after_match_result_changed(self, r#match, old_result, &old_metrics);

            for (ranking_id, metric) in updates {
                let old_metric = self.rankings.set_metric(ranking_id, metric);
                old_metrics
                    .entry((&*old_metric as &dyn Any).type_id())
                    .or_default()
                    .push((ranking_id, old_metric))
            }
        }
        let new_ranks = self.ranker.update_ranks(self);
        self.rankings.apply_new_ranks(new_ranks);
        self.create_new_matches();
    }

    /// Constructs a seeded [`SeedableRng`] by mixing the competition seed with `entropy`.
    ///
    /// Pass context-specific data (e.g. a phase index or a tuple of state fields) as
    /// `entropy` so that different operations produce independent random streams while
    /// remaining fully deterministic given the same update log.
    pub fn new_seeded_rng<H: Hash, R: SeedableRng>(&self, entropy: H) -> R {
        Seeder::from((self.seed, entropy)).into_rng()
    }

    /// Convenience wrapper around [`new_seeded_rng`](Self::new_seeded_rng) that
    /// returns a [`StdRng`].
    pub fn new_seeded_std_rng<H: Hash>(&self, entropy: H) -> StdRng {
        self.new_seeded_rng(entropy)
    }

    pub fn metric_of<T: 'static>(&self, ranking_id: &RankingId) -> &T {
        self.ranking_view(ranking_id).metric()
    }

    pub fn rankings(&self) -> &Rankings {
        &self.rankings
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhaseTransitionId(Uuid);

impl PhaseTransitionId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PhaseTransition {
    id: PhaseTransitionId,
    source_phase_ids: Vec1<PhaseId>,
    target_phase_ids: Vec1<PhaseId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchResult {
    AWins,
    BWins,
    Draw,
    NotFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MatchId(Uuid);

impl MatchId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Match {
    id: MatchId,
    team_a: Vec1<RankingId>,
    team_b: Vec1<RankingId>,
    round: NonZeroU32,
    result: MatchResult,
}

impl IdAble for Match {
    type Id = MatchId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Team {
    A,
    B,
}

impl Team {
    pub fn opponent(self) -> Self {
        match self {
            Team::A => Team::B,
            Team::B => Team::A,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ResultOfTeam {
    Win,
    Draw,
    Loss,
    NotFinished,
}

impl Match {
    pub fn new(team_a: Vec1<RankingId>, team_b: Vec1<RankingId>, round: NonZeroU32) -> Self {
        assert!(team_a.iter().all(|id| !team_b.contains(id)));
        let id = MatchId(Uuid::new_v5(
            &NAMESPACE_MATCH,
            &team_a
                .iter()
                .chain(team_b.iter())
                .copied()
                .flat_map(|uuid| uuid.into_bytes())
                .chain(round.get().to_le_bytes())
                .collect::<Vec<_>>(),
        ));
        Self {
            id,
            team_a,
            team_b,
            round,
            result: MatchResult::NotFinished,
        }
    }

    pub fn team_of(&self, ranking_id: &RankingId) -> Option<Team> {
        if self.team_a().contains(ranking_id) {
            Some(Team::A)
        } else if self.team_b().contains(ranking_id) {
            Some(Team::B)
        } else {
            None
        }
    }

    pub fn id(&self) -> MatchId {
        self.id
    }

    pub fn team_a(&self) -> &Vec1<RankingId> {
        &self.team_a
    }

    pub fn team_b(&self) -> &Vec1<RankingId> {
        &self.team_b
    }

    pub fn rankings_of(&self, team: Team) -> &Vec1<RankingId> {
        match team {
            Team::A => self.team_a(),
            Team::B => self.team_b(),
        }
    }

    pub fn result_of_team(&self, team: Team) -> ResultOfTeam {
        match (team, self.result) {
            (Team::A, MatchResult::AWins) | (Team::B, MatchResult::BWins) => ResultOfTeam::Win,
            (_, MatchResult::Draw) => ResultOfTeam::Draw,
            (Team::A, MatchResult::BWins) | (Team::B, MatchResult::AWins) => ResultOfTeam::Loss,
            (_, MatchResult::NotFinished) => ResultOfTeam::NotFinished,
        }
    }

    pub fn rankings(&self) -> impl Iterator<Item = &RankingId> {
        self.team_a.iter().chain(self.team_b.iter())
    }

    pub fn non_losing_rankings(&self) -> impl Iterator<Item = &RankingId> {
        match self.result {
            MatchResult::AWins => Either::Left(self.team_a.iter()),
            MatchResult::BWins => Either::Left(self.team_b.iter()),
            MatchResult::Draw => Either::Right(self.rankings()),
            // technically no team is losing yet if result is not finished
            MatchResult::NotFinished => Either::Right(self.rankings()),
        }
    }

    pub fn round(&self) -> NonZeroU32 {
        self.round
    }

    pub fn result(&self) -> MatchResult {
        self.result
    }

    pub fn is_finished(&self) -> bool {
        !matches!(self.result, MatchResult::NotFinished)
    }

    pub(crate) fn set_result(&mut self, result: MatchResult) {
        self.result = result;
    }
}

/// Builds a standalone [`Phase`] with prepopulated rankings, without needing a
/// [`Competition`](crate::entities::competition::Competition)
/// around it. Intended for unit tests of [`PhaseFormat`](crate::plugins::phase_format::PhaseFormat)
/// and [`Ranker`] implementations that only care about a single phase in isolation.
#[cfg(test)]
pub(crate) struct PhaseBuilder {
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    rankings: Vec<Ranking>,
}

#[cfg(test)]
impl PhaseBuilder {
    /// Creates a builder with no rankings yet and a ranker that leaves all ranks at 1 (use
    /// [`PhaseBuilder::ranker`] to override it if the test cares about ranks).
    pub(crate) fn new(phase_format: Box<dyn PhaseFormat>) -> Self {
        Self {
            phase_format,
            ranker: Box::new(crate::plugins::ranker::HierarchicalRanker::new(Vec::new())),
            metrics_calculators: Vec::new(),
            rankings: Vec::new(),
        }
    }

    #[expect(dead_code)]
    pub(crate) fn ranker(mut self, ranker: Box<dyn Ranker>) -> Self {
        self.ranker = ranker;
        self
    }

    /// Adds a metrics calculator whose metrics get initialized in [`PhaseBuilder::build`].
    pub(crate) fn metrics_calculator(
        mut self,
        metrics_calculator: Box<dyn MetricsCalculator>,
    ) -> Self {
        self.metrics_calculators.push(metrics_calculator);
        self
    }

    /// Adds `num_players` more rankings, each a single entrant, ranked after the rankings
    /// already added.
    pub(crate) fn with_players(mut self, num_players: u32) -> Self {
        for _ in 0..num_players {
            let entrant_id = EntrantId::new(Uuid::from_u128(self.rankings.len() as u128));
            let unique_rank = NonZeroU32::new(self.rankings.len() as u32 + 1).unwrap();
            self.rankings
                .push(Ranking::new(vec1::vec1![entrant_id], unique_rank));
        }
        self
    }

    pub(crate) fn build(self) -> Phase {
        let rankings = Rankings::new_with(self.metrics_calculators, self.rankings);
        Phase {
            id: PhaseId::new(Uuid::nil()),
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings,
            matches: IdMap::new(),
            phase_format: self.phase_format,
            seed: [0; 12],
            ranker: self.ranker,
        }
    }
}
