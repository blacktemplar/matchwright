use std::{collections::HashMap, hash::Hash, num::NonZeroU32, rc::Rc};

use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use rand_seeder::Seeder;
use uuid::{Uuid, uuid};
use vec1::{Vec1, vec1};

use crate::{
    plugins::{metrics_calculator::MetricsCalculator, phase_format::PhaseFormat},
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
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) {
        let phase = Phase {
            id,
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings: HashMap::new(),
            ranking_ids_ordered_by_unique_rank: vec![],
            matches: HashMap::new(),
            phase_format,
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
        // 0 represents the start of the competition
        let mut rng = self.new_seeded_std_rng(0);
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
        for metrics_calculator in &self.metrics_calculators {
            let metrics = metrics_calculator.initialize_metrics(self);
            for (ranking_id, metric) in metrics {
                self.rankings
                    .get_mut(&ranking_id)
                    .expect("Ranking must exist")
                    .metrics
                    .insert_boxed(metric);
            }
        }
        self.create_new_matches();
    }

    pub(crate) fn create_new_matches(&mut self) {
        let new_matches = self.phase_format.create_new_matches(self);
        self.matches
            .extend(new_matches.into_iter().map(|m| (m.id, m)))
    }

    pub(crate) fn report_match_result(&mut self, match_id: MatchId, match_result: MatchResult) {
        let r#match = self.matches.get_mut(&match_id).unwrap();
        let old_result = r#match.result();
        assert!(old_result == MatchResult::NotFinished);
        r#match.set_result(match_result);
        for metrics_calculator in &self.metrics_calculators {
            let updates =
                metrics_calculator.update_after_match_result_changed(self, match_id, old_result);
            for (ranking_id, metric) in updates {
                self.rankings
                    .get_mut(&ranking_id)
                    .expect("Ranking must exist")
                    .metrics
                    .insert_boxed(metric);
            }
        }
        self.create_new_matches();
    }

    #[expect(unused)]
    fn update_ranking_ids_ordered_by_unique_rank(&mut self) {
        self.ranking_ids_ordered_by_unique_rank
            .sort_by_key(|id| self.rankings[id].unique_rank());
    }
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
    team_a: Vec<RankingId>,
    team_b: Vec<RankingId>,
    round: NonZeroU32,
    result: MatchResult,
}

impl Match {
    pub fn new(team_a: Vec<RankingId>, team_b: Vec<RankingId>, round: NonZeroU32) -> Self {
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

    pub fn id(&self) -> MatchId {
        self.id
    }

    pub fn team_a(&self) -> &[RankingId] {
        &self.team_a
    }

    pub fn team_b(&self) -> &[RankingId] {
        &self.team_b
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
