use std::{
    any::{Any, TypeId},
    hash::Hash,
    num::NonZeroU32,
};

use itertools::{Either, Itertools};
use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};
use rand_seeder::Seeder;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::{Uuid, uuid};
use vec1::{Vec1, vec1};

use crate::{
    map::{IdAble, IdMap, Map},
    plugins::{
        metrics_calculator::{Metric, MetricsCalculator, MetricsCalculators},
        phase_format::PhaseFormat,
        ranker::Ranker,
    },
    type_map::downcast_unchecked_ref,
};

/// Namespaces for various deterministic ids
const NAMESPACE_MATCH: Uuid = uuid!("d4098937-85f4-49ce-945a-be8ae74da780");
const NAMESPACE_RANKING: Uuid = uuid!("eb5e6d16-15af-42e1-b0f4-f5a643fd1e9a");

#[derive(Default, Serialize, Deserialize)]
pub struct Competition {
    entrants: IdMap<Entrant>,
    phases: IdMap<Phase>,
    phase_transitions: Vec<PhaseTransition>,
    started: bool,
    seed: [u8; 12],
}

impl Competition {
    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn entrants(&self) -> &IdMap<Entrant> {
        &self.entrants
    }

    pub(crate) fn add_entrant(&mut self, entrant: Entrant) {
        self.entrants.insert(entrant);
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
            rankings: IdMap::default(),
            ranking_ids_ordered_by_unique_rank: vec![],
            matches: IdMap::new(),
            phase_format,
            ranker,
            seed,
            metrics_calculators: metrics_calculators.into(),
        };
        self.phases.insert(phase);
    }

    pub fn phases(&self) -> &IdMap<Phase> {
        &self.phases
    }

    pub(crate) fn phases_mut(&mut self) -> &mut IdMap<Phase> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntrantId(Uuid);

impl EntrantId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entrant {
    id: EntrantId,
}

impl IdAble for Entrant {
    type Id = EntrantId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl Entrant {
    pub fn new(id: EntrantId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> EntrantId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhaseId(Uuid);

impl PhaseId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(try_from = "SerializedPhase")]
pub struct Phase {
    id: PhaseId,
    outgoing_phase_transitions: Vec<PhaseTransitionId>,
    incoming_phase_transitions: Vec<PhaseTransitionId>,
    rankings: IdMap<Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
    matches: IdMap<Match>,
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    seed: [u8; 12],
    metrics_calculators: MetricsCalculators,
}

/// The fields of a [`Phase`] as they were serialized, before anything about them is checked.
///
/// A phase deserializes through this rather than directly, so that [`TryFrom`] gets to reject one
/// whose metrics do not follow the layout of its calculators before it exists as a [`Phase`].
/// Nothing else establishes that invariant for a deserialized phase: it is
/// [`initialize_metrics`](Phase::initialize_metrics) that does so for a phase that is played rather
/// than read, and deserializing never goes through it.
///
/// # Invariant
/// The fields must stay those of [`Phase`], in name and in order: a format that is not self
/// describing, like [`postcard`](https://docs.rs/postcard), writes nothing but the values in
/// declaration order, so a phase would silently read back as something else. The round trip tests
/// through both formats are what keeps the two in sync.
#[derive(Deserialize)]
struct SerializedPhase {
    id: PhaseId,
    outgoing_phase_transitions: Vec<PhaseTransitionId>,
    incoming_phase_transitions: Vec<PhaseTransitionId>,
    rankings: IdMap<Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
    matches: IdMap<Match>,
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    seed: [u8; 12],
    metrics_calculators: MetricsCalculators,
}

/// Why a serialized [`Phase`] cannot be read back as one.
#[derive(Error, Debug)]
pub enum InvalidPhase {
    #[error(
        "the metrics of ranking {0:?} do not follow the layout of the metrics calculators of \
         the phase"
    )]
    MetricsDoNotFollowLayout(RankingId),
}

impl TryFrom<SerializedPhase> for Phase {
    type Error = InvalidPhase;

    fn try_from(phase: SerializedPhase) -> Result<Self, Self::Error> {
        let phase = Self {
            id: phase.id,
            outgoing_phase_transitions: phase.outgoing_phase_transitions,
            incoming_phase_transitions: phase.incoming_phase_transitions,
            rankings: phase.rankings,
            ranking_ids_ordered_by_unique_rank: phase.ranking_ids_ordered_by_unique_rank,
            matches: phase.matches,
            phase_format: phase.phase_format,
            ranker: phase.ranker,
            seed: phase.seed,
            metrics_calculators: phase.metrics_calculators,
        };
        // Reading a metric by its position casts it without checking, so a serialized phase whose
        // metrics are in another order than its calculators, or hold another type than they
        // produce, must not become a `Phase` at all. The metrics are part of the serialized form
        // and the calculators are the deserialized plugins, so nothing but this check ties the two
        // together.
        for ranking in phase.rankings.values() {
            if !phase.metrics_calculators.is_layout_of(&ranking.metrics) {
                return Err(InvalidPhase::MetricsDoNotFollowLayout(ranking.id()));
            }
        }
        Ok(phase)
    }
}

impl IdAble for Phase {
    type Id = PhaseId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl Phase {
    pub fn id(&self) -> PhaseId {
        self.id
    }

    pub fn rankings(&self) -> &IdMap<Ranking> {
        &self.rankings
    }

    pub fn ranking_ids_ordered_by_unique_rank(&self) -> &[RankingId] {
        &self.ranking_ids_ordered_by_unique_rank
    }

    pub fn matches(&self) -> &IdMap<Match> {
        &self.matches
    }

    pub(crate) fn start(&mut self, ranking_entrants: Vec<Vec1<EntrantId>>) {
        let (ordered_ranking_ids, rankings) = ranking_entrants
            .into_iter()
            .enumerate()
            .map(|(index, entrants)| {
                let ranking = Ranking::new(entrants, NonZeroU32::new(index as u32 + 1).unwrap());
                (ranking.id(), ranking)
            })
            .unzip();
        self.rankings = rankings;
        self.ranking_ids_ordered_by_unique_rank = ordered_ranking_ids;
        self.initialize_metrics();
        self.create_new_matches();
    }

    /// Gives every ranking of this phase the default value of every metric calculated for this
    /// phase.
    ///
    /// # Panics
    /// Panics if those metrics do not follow the layout of the calculators, see
    /// [`MetricsCalculators::is_layout_of`].
    fn initialize_metrics(&mut self) {
        for ranking in self.rankings.values_mut() {
            ranking.metrics = self
                .metrics_calculators
                .iter()
                .map(|c| c.default_metric_value())
                .collect();
            // The layout was built from one call of `default_metric_value` per calculator and the
            // metrics just now from another, so this is where the two are checked against each
            // other: reading a metric by its position casts it without checking, and the two
            // agreeing is what keeps that sound. Every later write puts a metric at the position
            // of its own type, which cannot break the agreement again, so this one check covers
            // the whole lifetime of the phase. It runs once per phase start, which is cheap enough
            // to be a real assertion rather than a debug one.
            assert!(
                self.metrics_calculators.is_layout_of(&ranking.metrics),
                "every metrics calculator of a phase must produce a metric of the same type on \
                 every call, and no two of them the same type"
            );
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
        let mut old_metrics: Map<TypeId, Vec<(RankingId, Metric)>> = Map::default();
        for metrics_calculator in &self.metrics_calculators {
            let updates = metrics_calculator.update_after_match_result_changed(
                self,
                r#match,
                old_result,
                &old_metrics,
            );

            for (ranking_id, metric) in updates {
                let new_type = (*metric).type_id();
                let old_metric = std::mem::replace(
                    &mut self
                        .rankings
                        .get_mut(&ranking_id)
                        .expect("Ranking must exist")
                        .metrics[self.metrics_calculators.index_of_box(&metric)],
                    metric,
                );
                // the type of metrics should never change which is important to uphold
                // the invariant that metrics of rankings adhere to the layout. Note the
                // dereference: `old_metric.type_id()` would be the type id of the box.
                assert!(new_type == (*old_metric).type_id());
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

    pub fn metric_of<T: 'static>(&self, ranking_id: &RankingId) -> Option<&T> {
        self.metrics_of(ranking_id).metric()
    }

    pub fn metrics_of(&self, ranking_id: &RankingId) -> Metrics<'_> {
        let ranking = &self.rankings[ranking_id];
        // SAFETY: We just got ranking from `self.rankings` above.
        unsafe { self.metrics_of_ranking(ranking) }
    }

    /// The metrics of `ranking`, without looking it up by its id.
    ///
    /// # Safety
    /// `ranking` must exist in `self.rankings`, since that is what makes its metrics follow
    /// this phase's [layout](MetricsCalculators) and hence what [`Metrics`] relies on.
    pub(crate) unsafe fn metrics_of_ranking<'a>(&'a self, ranking: &'a Ranking) -> Metrics<'a> {
        Metrics(self, &ranking.metrics)
    }

    /// The position the metric `T` has in the metrics of every ranking of this phase, or
    /// [`None`] if this phase does not calculate it.
    pub(crate) fn metric_index<T: 'static>(&self) -> Option<usize> {
        self.metrics_calculators.index_of::<T>()
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum EntropyTypes {
    CompetitionStart,
    PhaseFormat,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Ranking {
    id: RankingId,
    entrants: Vec1<EntrantId>,
    initial_unique_rank: NonZeroU32,
    unique_rank: NonZeroU32,
    rank: NonZeroU32,
    metrics: Vec<Metric>,
}

impl IdAble for Ranking {
    type Id = RankingId;

    fn id(&self) -> Self::Id {
        self.id
    }
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
            metrics: Vec::new(),
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
}

/// The metrics of one ranking of a phase, which is what makes them readable: the metrics themselves
/// are nothing but a list of type erased values, and only the phase they belong to knows which
/// position holds which metric.
///
/// # Invariant
/// The metrics must be those of a ranking of the phase, so that they follow the phase's
/// [layout](MetricsCalculators). Only this module constructs a [`Metrics`], which is why
/// [`Self::metric`] may cast without checking.
#[derive(Debug, Clone, Copy)]
pub struct Metrics<'a>(&'a Phase, &'a [Metric]);

impl<'a> Metrics<'a> {
    /// The value of the metric `T`, or [`None`] if the phase does not calculate it.
    pub fn metric<T: 'static>(&self) -> Option<&'a T> {
        let metric = &self.1[self.0.metric_index::<T>()?];
        // SAFETY: the metrics of every ranking of a phase hold one value per metrics calculator of
        // that phase, in the order of the calculators, which is the order `metric_index` indexes;
        // the value at the position of `T` is therefore a `T`.
        Some(unsafe { downcast_unchecked_ref(&**metric) })
    }

    /// The metrics as the bare list of type erased values they are stored as, positioned by the
    /// [layout](Phase::metric_index) of the phase.
    ///
    /// This is what lets a ranker read a metric by the position it resolved once up front
    /// instead of looking its type up per ranking, which [`Self::metric`] has to do.
    pub(crate) fn values(&self) -> &'a [Metric] {
        self.1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchResult {
    AWins,
    BWins,
    Draw,
    NotFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RankingId(Uuid);

impl RankingId {
    pub fn nil() -> Self {
        Self(Uuid::nil())
    }
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
        let rankings = self.rankings.into_iter().collect();
        let mut phase = Phase {
            id: PhaseId::new(Uuid::nil()),
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings,
            ranking_ids_ordered_by_unique_rank,
            matches: IdMap::new(),
            phase_format: self.phase_format,
            seed: [0; 12],
            ranker: self.ranker,
            metrics_calculators: self.metrics_calculators.into(),
        };
        phase.initialize_metrics();
        phase
    }
}
