use std::{any::Any, hash::Hash, num::NonZeroU32};

use itertools::{Either, Itertools};
use plugin_factory::compatibility::Declaration;
use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};
use rand_seeder::Seeder;
use serde::{Deserialize, Serialize};
use uuid::{Uuid, uuid};
use vec1::{Vec1, vec1};

use crate::{
    map::{IdAble, IdMap},
    plugins::{
        PhasePlugins,
        facts::EntrantsPerRanking,
        metrics_calculator::{Metric, MetricsCalculator, OldMetrics},
        phase_format::{PhaseFormat, Schedule},
        ranker::HierarchicalRanker,
    },
    type_map::{TypeMap, TypeMapValue},
};

/// Namespaces for various deterministic ids
const NAMESPACE_MATCH: Uuid = uuid!("d4098937-85f4-49ce-945a-be8ae74da780");
const NAMESPACE_RANKING: Uuid = uuid!("eb5e6d16-15af-42e1-b0f4-f5a643fd1e9a");
const NAMESPACE_BYE: Uuid = uuid!("5b0f3c1e-2d8a-4c67-9e41-7a3f6d92b8c5");

#[derive(Default, Serialize, Deserialize)]
pub struct Competition {
    entrants: IdMap<Entrant>,
    phases: IdMap<Phase>,
    phase_transitions: Vec<PhaseTransition>,
    started: bool,
    seed: Seed,
}

/// The source of all randomness of a competition, or of one of its phases.
///
/// Every random stream is derived from a seed and some entropy, so replaying the same transitions
/// gives the same results.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seed([u8; 12]);

impl Seed {
    pub const fn new(bytes: [u8; 12]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> [u8; 12] {
        self.0
    }

    /// Constructs a seeded [`SeedableRng`] by mixing this seed with `entropy`.
    ///
    /// Pass context-specific data (e.g. a phase index or a tuple of state fields) as `entropy`:
    /// different values give independent random streams, the same value always gives the same one.
    pub fn new_seeded_rng<H: Hash, R: SeedableRng>(self, entropy: H) -> R {
        Seeder::from((self.0, entropy)).into_rng()
    }

    /// Convenience wrapper around [`new_seeded_rng`](Self::new_seeded_rng) that
    /// returns a [`StdRng`].
    pub fn new_seeded_std_rng<H: Hash>(self, entropy: H) -> StdRng {
        self.new_seeded_rng(entropy)
    }
}

impl From<[u8; 12]> for Seed {
    fn from(bytes: [u8; 12]) -> Self {
        Self(bytes)
    }
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

    pub(crate) fn set_seed(&mut self, seed: Seed) {
        self.seed = seed;
    }

    pub fn seed(&self) -> Seed {
        self.seed
    }

    pub(crate) fn add_phase(&mut self, id: PhaseId, plugins: PhasePlugins) {
        let (phase_format, ranker, metrics_calculators) = plugins.into_parts();
        let seed = Seed(self.seed.new_seeded_std_rng(id).random());
        self.phases.insert(Phase::new(
            id,
            phase_format,
            ranker,
            metrics_calculators,
            seed,
        ));
    }

    /// Declares the facts that hold for a phase added to this competition before any of its
    /// plugins declare theirs, i.e. the context of its [`PhasePlugins`] for the functions of
    /// [`plugin_factory::draft`] and [`plugin_factory::compatibility`].
    pub fn declare_phase_context(&self, declaration: &mut Declaration) {
        // every entrant gets a ranking of its own when the competition starts, see `Self::start`
        declaration.provide(EntrantsPerRanking::SINGLE);
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

    pub(crate) fn start(&mut self) {
        let mut ranking_entrants: Vec<_> = self.entrants().keys().map(|e| vec1![*e]).collect();
        // Note that keys() is not guaranteed to give a stable ordering. To make sure ordering is
        // stable we sort before shuffling
        ranking_entrants.sort_by_key(|e| e[0].0);
        let mut rng = self.seed.new_seeded_std_rng(EntropyTypes::CompetitionStart);
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

/// Someone taking part in a competition.
///
/// Beyond its [`EntrantId`] an entrant holds only `annotations`: everything a consumer wants to
/// show or store about it — a display name, a club, a database id — keyed by the consumer's own
/// type. Annotations are given once, by the
/// [`AddEntrant`](crate::transitions::add_entrant::AddEntrant) that creates the entrant, and cannot
/// be changed afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entrant {
    id: EntrantId,
    annotations: TypeMap,
}

impl IdAble for Entrant {
    type Id = EntrantId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl Entrant {
    /// Creates an entrant without any annotations.
    pub fn new(id: EntrantId) -> Self {
        Self {
            id,
            annotations: TypeMap::new(),
        }
    }

    pub fn id(&self) -> EntrantId {
        self.id
    }
}

impl Annotated for Entrant {
    fn annotations(&self) -> &TypeMap {
        &self.annotations
    }

    fn annotations_mut(&mut self) -> &mut TypeMap {
        &mut self.annotations
    }
}

/// An entity carrying annotations: data about it keyed by type, such as the display name of an
/// [`Entrant`] or the knockout stage of a [`Match`].
///
/// Annotations are given while building the entity, before the transition that creates it. Once it
/// is part of a [`Competition`] they cannot be changed, and they never take part in its id.
///
/// # Examples
/// ```
/// # use serde::{Deserialize, Serialize};
/// # use tournament_engine::entities::competition::{Annotated, Entrant, EntrantId};
/// # use tournament_engine::type_map::TypeMapValue;
/// # use uuid::Uuid;
/// #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// struct DatabaseId(u64);
///
/// #[typetag::serde(name = "example::database_id")]
/// impl TypeMapValue for DatabaseId {}
///
/// let entrant = Entrant::new(EntrantId::new(Uuid::nil())).with_annotation(DatabaseId(17));
///
/// assert_eq!(entrant.annotation::<DatabaseId>(), Some(&DatabaseId(17)));
/// ```
pub trait Annotated: Sized {
    /// All annotations of this entity.
    fn annotations(&self) -> &TypeMap;

    /// All annotations of this entity, for building it.
    fn annotations_mut(&mut self) -> &mut TypeMap;

    /// The annotation of type `T`, if any.
    fn annotation<T: 'static>(&self) -> Option<&T> {
        self.annotations().get::<T>()
    }

    /// Returns this entity with `annotation` added, replacing an annotation of the same type.
    fn with_annotation<T: TypeMapValue>(mut self, annotation: T) -> Self {
        self.annotations_mut().insert(annotation);
        self
    }

    /// Returns this entity with `annotations` as its annotations, replacing any it had.
    fn with_annotations(mut self, annotations: TypeMap) -> Self {
        *self.annotations_mut() = annotations;
        self
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
pub struct Phase {
    id: PhaseId,
    outgoing_phase_transitions: Vec<PhaseTransitionId>,
    incoming_phase_transitions: Vec<PhaseTransitionId>,
    phase_format: Box<dyn PhaseFormat>,
    ranker: HierarchicalRanker,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    data: PhaseData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PhaseData {
    rankings: IdMap<Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
    matches: IdMap<Match>,
    byes: IdMap<Bye>,
    seed: Seed,
}

impl IdAble for Phase {
    type Id = PhaseId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl PhaseData {
    /// The rankings of this phase, ordered by their [`Ranking::initial_unique_rank`].
    pub fn rankings(&self) -> &IdMap<Ranking> {
        &self.rankings
    }

    pub fn ranking_ids_ordered_by_unique_rank(&self) -> &[RankingId] {
        &self.ranking_ids_ordered_by_unique_rank
    }

    pub fn matches(&self) -> &IdMap<Match> {
        &self.matches
    }

    /// The byes of this phase, in the order they were scheduled in.
    pub fn byes(&self) -> &IdMap<Bye> {
        &self.byes
    }

    /// The seed of this phase, derived from the competition seed and the phase id.
    pub fn seed(&self) -> Seed {
        self.seed
    }

    /// Whether any match or bye has been scheduled in this phase.
    pub fn has_schedule(&self) -> bool {
        !self.matches.is_empty() || !self.byes.is_empty()
    }

    /// The position of the match in [`PhaseData::matches`], which can be used as a stable match
    /// number: matches are only ever appended, so an index never changes.
    pub fn get_match_index(&self, match_id: &MatchId) -> usize {
        self.matches.get_index_of(match_id).unwrap()
    }
}

impl Phase {
    /// A phase without rankings, matches and byes yet.
    fn new(
        id: PhaseId,
        phase_format: Box<dyn PhaseFormat>,
        ranker: HierarchicalRanker,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
        seed: Seed,
    ) -> Self {
        Self {
            id,
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            data: PhaseData {
                rankings: IdMap::new(),
                ranking_ids_ordered_by_unique_rank: vec![],
                matches: IdMap::new(),
                byes: IdMap::new(),
                seed,
            },
            phase_format,
            ranker,
            metrics_calculators,
        }
    }

    pub fn id(&self) -> PhaseId {
        self.id
    }

    /// The rankings, matches and seed of this phase, i.e. everything but its plugins.
    pub fn data(&self) -> &PhaseData {
        &self.data
    }

    pub(crate) fn phase_format(&self) -> &dyn PhaseFormat {
        &*self.phase_format
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
        self.data.rankings = rankings;
        self.data.ranking_ids_ordered_by_unique_rank = ordered_ranking_ids;
        self.initialize_metrics();
        self.create_new_matches();
    }

    /// Gives every ranking of this phase the default value of every metric calculated for this
    /// phase.
    fn initialize_metrics(&mut self) {
        let ranking_ids: Vec<_> = self.data.rankings.keys().copied().collect();
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
            self.data
                .rankings
                .get_mut(&ranking_id)
                .expect("Ranking must exist")
                .metrics
                .insert_boxed(metric);
        }
    }

    /// Adds the matches and byes the phase format schedules next, and updates the metrics and the
    /// ranks for every new bye.
    pub(crate) fn create_new_matches(&mut self) {
        let Schedule { matches, byes } = self.phase_format.create_new_matches(&self.data);
        // check requirements of create_new_matches
        debug_assert!(
            matches
                .iter()
                .flat_map(Match::rankings)
                .copied()
                .chain(byes.iter().map(Bye::ranking))
                .all(|r| self.data.rankings.contains_key(&r))
        );
        let num_matches_before = self.data.matches.len();
        let num_new_matches = matches.len();
        self.data.matches.extend(matches);
        let new_bye_ids: Vec<_> = byes.iter().map(Bye::id).collect();
        let num_byes_before = self.data.byes.len();
        self.data.byes.extend(byes);
        // This guarantees that all new match and bye ids were unique (at least relative to the
        // existing ids in this phase).
        debug_assert_eq!(
            self.data.matches.len(),
            num_matches_before + num_new_matches
        );
        debug_assert_eq!(self.data.byes.len(), num_byes_before + new_bye_ids.len());
        // check that ranking ids are unique within a round
        debug_assert!(
            self.data
                .matches
                .values()
                .flat_map(|m| m.rankings().map(|r| (m.round, *r)))
                .chain(self.data.byes.values().map(|b| (b.round, b.ranking)))
                .all_unique()
        );

        // A bye is final when it is scheduled, so it is accounted for right away. The format is not
        // asked again afterwards, even though the ranks may have changed: only a reported result
        // makes it schedule.
        for bye_id in &new_bye_ids {
            self.update_metrics(|metrics_calculator, data, old_metrics| {
                metrics_calculator.update_after_bye(data, &data.byes[bye_id], old_metrics)
            });
        }
        if !new_bye_ids.is_empty() {
            self.update_ranks();
        }
    }

    pub(crate) fn report_match_result(&mut self, match_id: MatchId, match_result: MatchResult) {
        let r#match = self.data.matches.get_mut(&match_id).unwrap();
        let old_result = r#match.result();
        assert!(old_result == MatchResult::NotFinished);
        r#match.set_result(match_result);
        self.update_metrics(|metrics_calculator, data, old_metrics| {
            metrics_calculator.update_after_match_result_changed(
                data,
                &data.matches[&match_id],
                old_result,
                old_metrics,
            )
        });
        self.update_ranks();
        self.create_new_matches();
    }

    /// Runs every metrics calculator, in order, through `updates_of` and stores the metrics it
    /// returns, handing each calculator the metrics replaced by the ones before it.
    fn update_metrics(
        &mut self,
        mut updates_of: impl FnMut(
            &mut dyn MetricsCalculator,
            &PhaseData,
            &OldMetrics,
        ) -> Vec<(RankingId, Metric)>,
    ) {
        let mut old_metrics = OldMetrics::default();
        for metrics_calculator in &mut self.metrics_calculators {
            let updates = updates_of(&mut **metrics_calculator, &self.data, &old_metrics);
            // check requirements of the update methods of MetricsCalculator
            debug_assert!(updates.iter().all(|(ranking_id, metric)| {
                let metric: &dyn Any = &**metric;
                self.data.rankings.contains_key(ranking_id)
                    && (&*metrics_calculator.default_metric_value() as &dyn Any).type_id()
                        == metric.type_id()
            }));

            for (ranking_id, metric) in updates {
                let old_metric = self
                    .data
                    .rankings
                    .get_mut(&ranking_id)
                    .expect("Ranking must exist")
                    .metrics
                    .insert_boxed(metric)
                    .unwrap();
                old_metrics.push(ranking_id, old_metric);
            }
        }
    }

    fn update_ranks(&mut self) {
        let new_ranks = self.ranker.update_ranks(&self.data);
        for (rank_id, new_rank) in new_ranks {
            self.data.rankings.get_mut(&rank_id).unwrap().rank = new_rank;
        }
        self.update_ranking_ordering_and_unique_ranks();
    }

    fn update_ranking_ordering_and_unique_ranks(&mut self) {
        // `sort_by_key` would look every ranking up twice per comparison, whereas the cached
        // variant looks every ranking up exactly once
        self.data
            .ranking_ids_ordered_by_unique_rank
            .sort_by_cached_key(|id| {
                let ranking = &self.data.rankings[id];
                (ranking.rank(), ranking.initial_unique_rank())
            });
        // set unique ranks
        for (index, ranking_id) in self
            .data
            .ranking_ids_ordered_by_unique_rank
            .iter()
            .enumerate()
        {
            self.data.rankings.get_mut(ranking_id).unwrap().unique_rank =
                NonZeroU32::new(index as u32 + 1).unwrap()
        }
        // check requirements of [`HierarchicalRanker::update_ranks`]
        debug_assert!(
            self.data.rankings[&self.data.ranking_ids_ordered_by_unique_rank[0]].rank.get() == 1 &&
            self.data.ranking_ids_ordered_by_unique_rank
                .iter()
                .map(|id| &self.data.rankings[id])
                .enumerate()
                .array_windows()
                .all(|[(_, r1), (i2, r2)]| r1.rank == r2.rank || r2.rank.get() == i2 as u32 + 1)
        );
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
    metrics: TypeMap,
}

impl IdAble for Ranking {
    type Id = RankingId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

/// The UUIDv5 in `namespace` of the bytes of `uuids`, followed by those of `round` if given.
fn derive_id(
    namespace: &Uuid,
    uuids: impl IntoIterator<Item = Uuid>,
    round: Option<NonZeroU32>,
) -> Uuid {
    let bytes: Vec<u8> = uuids
        .into_iter()
        .flat_map(Uuid::into_bytes)
        .chain(
            round
                .into_iter()
                .flat_map(|round| round.get().to_le_bytes()),
        )
        .collect();
    Uuid::new_v5(namespace, &bytes)
}

impl Ranking {
    pub fn new(entrants: Vec1<EntrantId>, unique_rank: NonZeroU32) -> Self {
        let id = RankingId(derive_id(
            &NAMESPACE_RANKING,
            entrants.iter().map(|e| e.0),
            None,
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

    /// The ranking with `metric` set, for tests of rankers that do not need a whole phase.
    #[cfg(test)]
    pub(crate) fn with_metric(mut self, metric: impl TypeMapValue) -> Self {
        self.metrics.insert(metric);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchResult {
    AWins,
    BWins,
    Draw,
    NotFinished,
}

impl MatchResult {
    /// The team that won, or `None` for a draw or a match that is not finished.
    pub fn winner(self) -> Option<Team> {
        match self {
            MatchResult::AWins => Some(Team::A),
            MatchResult::BWins => Some(Team::B),
            MatchResult::Draw | MatchResult::NotFinished => None,
        }
    }

    /// The team that lost, or `None` for a draw or a match that is not finished.
    pub fn loser(self) -> Option<Team> {
        self.winner().map(Team::opponent)
    }
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

/// A match between two teams of rankings in a round of a phase.
///
/// Beyond who plays it, its round and its result, a match holds `annotations`: data that describes
/// it, keyed by type, such as which stage of a knockout it belongs to. The
/// [`PhaseFormat`] scheduling the match gives them when
/// creating it, with [`Annotated::with_annotation`].
#[derive(Debug, Serialize, Deserialize)]
pub struct Match {
    id: MatchId,
    team_a: Vec1<RankingId>,
    team_b: Vec1<RankingId>,
    round: NonZeroU32,
    result: MatchResult,
    annotations: TypeMap,
}

impl IdAble for Match {
    type Id = MatchId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
        let id = MatchId(derive_id(
            &NAMESPACE_MATCH,
            team_a.iter().chain(team_b.iter()).map(|ranking| ranking.0),
            Some(round),
        ));
        Self {
            id,
            team_a,
            team_b,
            round,
            result: MatchResult::NotFinished,
            annotations: TypeMap::new(),
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
        // technically no team is losing yet if result is not finished
        match self.result.winner() {
            Some(winner) => Either::Left(self.rankings_of(winner).iter()),
            None => Either::Right(self.rankings()),
        }
    }

    /// The round this match is played in. A ranking plays at most once per round, and its matches
    /// in later rounds are played after those in earlier ones.
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

impl Annotated for Match {
    fn annotations(&self) -> &TypeMap {
        &self.annotations
    }

    fn annotations_mut(&mut self) -> &mut TypeMap {
        &mut self.annotations
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ByeId(Uuid);

/// A round of a phase in which a ranking has no match to play, as decided by the
/// [`PhaseFormat`] that schedules it.
///
/// A bye is final as soon as it is scheduled: it has no result to report, and the metrics of its
/// ranking are updated right away, see [`MetricsCalculator::update_after_bye`]. Like a match it
/// holds `annotations`, given by the format when creating it with [`Annotated::with_annotation`].
#[derive(Debug, Serialize, Deserialize)]
pub struct Bye {
    id: ByeId,
    ranking: RankingId,
    round: NonZeroU32,
    annotations: TypeMap,
}

impl IdAble for Bye {
    type Id = ByeId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl Bye {
    pub fn new(ranking: RankingId, round: NonZeroU32) -> Self {
        let id = ByeId(derive_id(&NAMESPACE_BYE, [ranking.0], Some(round)));
        Self {
            id,
            ranking,
            round,
            annotations: TypeMap::new(),
        }
    }

    pub fn id(&self) -> ByeId {
        self.id
    }

    /// The ranking that has no match to play.
    pub fn ranking(&self) -> RankingId {
        self.ranking
    }

    /// The round the ranking has no match in. Like a match, a bye takes up the one slot its ranking
    /// has in that round, see [`Match::round`].
    pub fn round(&self) -> NonZeroU32 {
        self.round
    }
}

impl Annotated for Bye {
    fn annotations(&self) -> &TypeMap {
        &self.annotations
    }

    fn annotations_mut(&mut self) -> &mut TypeMap {
        &mut self.annotations
    }
}

/// Builds a standalone [`Phase`] with prepopulated rankings, without a [`Competition`] around it,
/// for unit tests of [`PhaseFormat`](crate::plugins::phase_format::PhaseFormat) and
/// [`SliceRanker`](crate::plugins::ranker::SliceRanker) implementations.
#[cfg(test)]
pub(crate) struct PhaseBuilder {
    phase_format: Box<dyn PhaseFormat>,
    ranker: HierarchicalRanker,
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
            ranker: HierarchicalRanker::default(),
            metrics_calculators: Vec::new(),
            rankings: Vec::new(),
        }
    }

    pub(crate) fn ranker(mut self, ranker: HierarchicalRanker) -> Self {
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
        let mut phase = Phase::new(
            PhaseId::new(Uuid::nil()),
            self.phase_format,
            self.ranker,
            self.metrics_calculators,
            Seed::default(),
        );
        phase.data.ranking_ids_ordered_by_unique_rank =
            self.rankings.iter().map(Ranking::id).collect();
        phase.data.rankings = self.rankings.into_iter().collect();
        phase.initialize_metrics();
        phase
    }
}
