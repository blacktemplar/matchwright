use std::{
    any::{Any, TypeId},
    hash::Hash,
    num::NonZeroU32,
    rc::Rc,
};

use itertools::{Either, Itertools};
use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};
use rand_seeder::Seeder;
use uuid::{Uuid, uuid};
use vec1::{Vec1, vec1};

use crate::{
    hash_map::HashMap,
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator},
        phase_format::PhaseFormat,
        ranker::Ranker,
    },
    type_map::TypeMap,
};

/// Namespaces for various deterministic ids
const NAMESPACE_MATCH: Uuid = uuid!("d4098937-85f4-49ce-945a-be8ae74da780");
const NAMESPACE_RANKING: Uuid = uuid!("eb5e6d16-15af-42e1-b0f4-f5a643fd1e9a");

#[derive(Default)]
pub struct Competition {
    entrants: HashMap<EntrantId, Entrant>,
    phases: HashMap<PhaseId, Phase>,
    started: bool,
    seed: [u8; 12],
}

impl Competition {
    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn entrants(&self) -> &HashMap<EntrantId, Entrant> {
        &self.entrants
    }

    pub(crate) fn add_entrant(&mut self, entrant: Entrant) {
        self.entrants.insert(entrant.id(), entrant);
    }

    pub(crate) fn remove_entrant(&mut self, entrant: &Entrant) {
        self.entrants.remove(&entrant.id());
    }

    pub(crate) fn set_seed(&mut self, seed: [u8; 12]) {
        self.seed = seed;
    }

    pub fn seed(&self) -> [u8; 12] {
        self.seed
    }

    pub(crate) fn add_phase(
        &mut self,
        id: PhaseId,
        phase_format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) {
        let seed = self.new_seeded_std_rng(id).random();
        let phase = Phase {
            id,
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings: HashMap::default(),
            ranking_ids_ordered_by_unique_rank: vec![],
            matches: HashMap::new(),
            phase_format,
            ranker,
            seed,
            metrics_calculators,
        };
        self.phases.insert(id, phase);
    }

    pub fn phases(&self) -> &HashMap<PhaseId, Phase> {
        &self.phases
    }

    pub(crate) fn phases_mut(&mut self) -> &mut HashMap<PhaseId, Phase> {
        &mut self.phases
    }

    pub fn phase(&mut self, phase_id: &PhaseId) -> Option<&Phase> {
        self.phases.get(phase_id)
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

    pub(crate) fn start(&mut self) {
        let mut ranking_entrants: Vec<_> = self.entrants().keys().map(|e| vec1![*e]).collect();
        // Note that keys() is not guaranteed to give a stable ordering. To make sure ordering is
        // stable we sort before shuffling
        ranking_entrants.sort_by_key(|e| e[0].0);
        let mut rng = self.new_seeded_std_rng(EntropyTypes::CompetitionStart);
        ranking_entrants.shuffle(&mut rng);
        if self.phases().len() != 1 {
            unimplemented!("Multi-phase competition not yet supported");
        }
        self.started = true;
        let phase = self.phases.values_mut().next().unwrap();
        phase.start(ranking_entrants);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntrantId(Uuid);

impl EntrantId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    id: EntrantId,
}

impl Entrant {
    pub fn new(id: EntrantId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> EntrantId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhaseId(Uuid);

impl PhaseId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug)]
#[expect(dead_code)]
pub struct Phase {
    id: PhaseId,
    outgoing_phase_transitions: Vec<Rc<PhaseTransition>>,
    incoming_phase_transitions: Vec<Rc<PhaseTransition>>,
    rankings: HashMap<RankingId, Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
    matches: HashMap<MatchId, Match>,
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    seed: [u8; 12],
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

impl Phase {
    pub fn id(&self) -> PhaseId {
        self.id
    }

    pub fn rankings(&self) -> &HashMap<RankingId, Ranking> {
        &self.rankings
    }

    pub fn ranking_ids_ordered_by_unique_rank(&self) -> &[RankingId] {
        &self.ranking_ids_ordered_by_unique_rank
    }

    pub fn matches(&self) -> &HashMap<MatchId, Match> {
        &self.matches
    }

    pub(crate) fn start(&mut self, ranking_entrants: Vec<Vec1<EntrantId>>) {
        let (ordered_ranking_ids, rankings) = ranking_entrants
            .into_iter()
            .enumerate()
            .map(|(index, entrants)| {
                let ranking = Ranking::new(entrants, NonZeroU32::new(index as u32 + 1).unwrap());
                (ranking.id(), (ranking.id(), ranking))
            })
            .unzip();
        self.rankings = rankings;
        self.ranking_ids_ordered_by_unique_rank = ordered_ranking_ids;
        self.initialize_metrics();
        self.create_new_matches();
    }

    /// Gives every ranking of this phase the default value of every metric calculated for this
    /// phase.
    fn initialize_metrics(&mut self) {
        let ranking_ids: Vec<_> = self.rankings.keys().copied().collect();
        let metrics: Vec<_> = self
            .metrics_calculators
            .iter()
            .flat_map(|metrics_calculator| {
                ranking_ids
                    .iter()
                    .map(move |ranking_id| (*ranking_id, metrics_calculator.default_metric_value()))
            })
            .collect();
        for (ranking_id, metric) in metrics {
            self.rankings
                .get_mut(&ranking_id)
                .expect("Ranking must exist")
                .metrics
                .insert_boxed(metric);
        }
    }

    pub(crate) fn create_new_matches(&mut self) {
        let new_matches = self.phase_format.create_new_matches(self);
        // check requirements of create_new_matches
        debug_assert!(
            new_matches
                .iter()
                .all(|m| { m.rankings().all(|r| self.rankings.contains_key(r)) })
        );
        let num_matches_before = self.matches.len();
        let num_new_matches = new_matches.len();
        self.matches
            .extend(new_matches.into_iter().map(|m| (m.id, m)));
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
        let mut old_metrics: HashMap<TypeId, Vec<(RankingId, Metric)>> = HashMap::new();
        for metrics_calculator in &self.metrics_calculators {
            let updates = metrics_calculator.update_after_match_result_changed(
                self,
                r#match,
                old_result,
                &old_metrics,
            );
            // check requirements of update_after_match_result_changed
            debug_assert!(updates.iter().all(|(ranking_id, metric)| {
                let metric: &dyn Any = &**metric;
                self.rankings.contains_key(ranking_id)
                    && (&*metrics_calculator.default_metric_value() as &dyn Any).type_id()
                        == metric.type_id()
            }));

            for (ranking_id, metric) in updates {
                let old_metric = self
                    .rankings
                    .get_mut(&ranking_id)
                    .expect("Ranking must exist")
                    .metrics
                    .insert_boxed(metric)
                    .unwrap();
                old_metrics
                    .entry((&*old_metric as &dyn Any).type_id())
                    .or_default()
                    .push((ranking_id, old_metric))
            }
        }
        // update rankings
        let new_ranks = self.ranker.update_ranks(self);
        for (rank_id, new_rank) in new_ranks {
            self.rankings.get_mut(&rank_id).unwrap().rank = new_rank;
        }
        self.update_ranking_ordering_and_unique_ranks();
        self.create_new_matches();
    }

    fn update_ranking_ordering_and_unique_ranks(&mut self) {
        // `sort_by_key` would look every ranking up twice per comparison, whereas the cached
        // variant looks every ranking up exactly once
        self.ranking_ids_ordered_by_unique_rank
            .sort_by_cached_key(|id| {
                let ranking = &self.rankings[id];
                (ranking.rank(), ranking.initial_unique_rank())
            });
        // set unique ranks
        for (index, ranking_id) in self.ranking_ids_ordered_by_unique_rank.iter().enumerate() {
            self.rankings.get_mut(ranking_id).unwrap().unique_rank =
                NonZeroU32::new(index as u32 + 1).unwrap()
        }
        // check requirements of [`Ranker::update_ranks`]
        debug_assert!(
            self.rankings[&self.ranking_ids_ordered_by_unique_rank[0]].rank.get() == 1 &&
            self.ranking_ids_ordered_by_unique_rank
                .iter()
                .map(|id| &self.rankings[id])
                .enumerate()
                .array_windows()
                .all(|[(_, r1), (i2, r2)]| r1.rank == r2.rank || r2.rank.get() == i2 as u32 + 1)
        );
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
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum EntropyTypes {
    CompetitionStart,
    PhaseFormat,
}

#[derive(Debug)]
#[expect(dead_code)]
pub struct PhaseTransition {
    source_phase_ids: Vec1<Uuid>,
    target_phase_ids: Vec1<Uuid>,
}

#[derive(Debug)]
pub struct Ranking {
    id: RankingId,
    entrants: Vec1<EntrantId>,
    initial_unique_rank: NonZeroU32,
    unique_rank: NonZeroU32,
    rank: NonZeroU32,
    metrics: TypeMap,
}

pub fn bytes_from_uuids(uuids: impl Iterator<Item = Uuid>) -> Vec<u8> {
    uuids.flat_map(|uuid| uuid.into_bytes()).collect()
}

impl Ranking {
    pub fn new(entrants: Vec1<EntrantId>, unique_rank: NonZeroU32) -> Self {
        let id = RankingId(Uuid::new_v5(
            &NAMESPACE_RANKING,
            &bytes_from_uuids(entrants.iter().map(|e| e.0)),
        ));
        Self {
            id,
            entrants,
            initial_unique_rank: unique_rank,
            unique_rank,
            rank: NonZeroU32::new(1).unwrap(),
            metrics: TypeMap::new(),
        }
    }

    pub fn id(&self) -> RankingId {
        self.id
    }

    pub fn entrants(&self) -> &Vec1<EntrantId> {
        &self.entrants
    }

    /// The unique rank of the ranking, which is guaranteed to be [`Ranking::rank`] with tie
    /// breaking equal values by their [`Ranking::initial_unique_rank`].
    pub fn unique_rank(&self) -> NonZeroU32 {
        self.unique_rank
    }

    pub fn rank(&self) -> NonZeroU32 {
        self.rank
    }

    pub fn initial_unique_rank(&self) -> NonZeroU32 {
        self.initial_unique_rank
    }

    pub fn metric<T: 'static>(&self) -> Option<&T> {
        self.metrics.get::<T>()
    }

    pub fn metrics(&self) -> &TypeMap {
        &self.metrics
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchResult {
    AWins,
    BWins,
    Draw,
    NotFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RankingId(Uuid);

impl RankingId {
    pub fn nil() -> Self {
        Self(Uuid::nil())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchId(Uuid);

impl MatchId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug)]
pub struct Match {
    id: MatchId,
    team_a: Vec1<RankingId>,
    team_b: Vec1<RankingId>,
    round: NonZeroU32,
    result: MatchResult,
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
                .flat_map(|uuid| uuid.0.into_bytes())
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

/// Builds a standalone [`Phase`] with prepopulated rankings, without needing a [`Competition`]
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
                .push(Ranking::new(vec1![entrant_id], unique_rank));
        }
        self
    }

    pub(crate) fn build(self) -> Phase {
        let ranking_ids_ordered_by_unique_rank = self.rankings.iter().map(Ranking::id).collect();
        let rankings = self.rankings.into_iter().map(|r| (r.id(), r)).collect();
        let mut phase = Phase {
            id: PhaseId::new(Uuid::nil()),
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings,
            ranking_ids_ordered_by_unique_rank,
            matches: HashMap::new(),
            phase_format: self.phase_format,
            seed: [0; 12],
            ranker: self.ranker,
            metrics_calculators: self.metrics_calculators,
        };
        phase.initialize_metrics();
        phase
    }
}
