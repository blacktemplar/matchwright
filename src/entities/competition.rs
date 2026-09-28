use std::hash::Hash;

use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};
use rand_seeder::Seeder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vec1::vec1;

use crate::{
    entities::phase::{Phase, PhaseId, PhaseTransition},
    map::{IdAble, IdMap},
    plugins::{metrics_calculator::MetricsCalculator, phase_format::PhaseFormat, ranker::Ranker},
};

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
        self.phases.insert(Phase::new(
            id,
            phase_format,
            ranker,
            metrics_calculators,
            seed,
        ));
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

    pub fn uuid(self) -> Uuid {
        self.0
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

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum EntropyTypes {
    CompetitionStart,
    PhaseFormat,
}
