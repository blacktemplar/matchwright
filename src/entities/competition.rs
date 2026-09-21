use std::{collections::HashMap, hash::Hash, num::NonZeroU32, rc::Rc};

use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use rand_seeder::Seeder;
use uuid::Uuid;
use vec1::{Vec1, vec1};

#[derive(Default)]
pub struct Competition {
    entrants: Vec<Rc<Entrant>>,
    phases: Vec<Phase>,
    started: bool,
    seed: [u8; 12],
    players_to_entrants: HashMap<Player, Rc<Entrant>>,
}

impl Competition {
    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn entrants(&self) -> &[Rc<Entrant>] {
        &self.entrants
    }

    pub(crate) fn add_entrant(&mut self, entrant: Entrant) {
        let entrant = Rc::new(entrant);
        for player in &entrant.players {
            self.players_to_entrants
                .insert(player.clone(), entrant.clone());
        }
        self.entrants.push(entrant);
    }

    pub fn entrant_of_player(&self, player: &Player) -> Option<&Rc<Entrant>> {
        self.players_to_entrants.get(player)
    }

    pub(crate) fn remove_entrant(&mut self, entrant: &Entrant) {
        for player in &entrant.players {
            self.players_to_entrants.remove(player);
        }
        self.entrants.retain(|t| &**t != entrant);
    }

    pub(crate) fn set_seed(&mut self, seed: [u8; 12]) {
        self.seed = seed;
    }

    pub fn seed(&self) -> [u8; 12] {
        self.seed
    }

    pub(crate) fn add_phase(&mut self, id: Uuid) {
        let phase = Phase {
            id,
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
            rankings: vec![],
        };
        self.phases.push(phase);
    }

    pub fn phases(&self) -> &[Phase] {
        &self.phases
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
        let mut ranking_entrants: Vec<_> =
            self.entrants().iter().map(|e| vec1![e.clone()]).collect();
        // 0 represents the start of the competition
        let mut rng = self.new_seeded_std_rng(0);
        ranking_entrants.shuffle(&mut rng);
        self.phases[0].rankings = ranking_entrants
            .into_iter()
            .enumerate()
            .map(|(rank, entrants)| Ranking {
                entrants,
                rank: NonZeroU32::new(1).unwrap(),
                unique_rank: NonZeroU32::new(rank as u32 + 1).unwrap(),
            })
            .collect();
        self.started = true;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    id: Uuid,
    players: Vec1<Player>,
}

impl Entrant {
    pub fn new(id: Uuid, players: Vec1<Player>) -> Self {
        Self { id, players }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn players(&self) -> &Vec1<Player> {
        &self.players
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Player(Uuid);

impl Player {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
}

#[derive(Debug)]
#[expect(dead_code)]
pub struct Phase {
    id: Uuid,
    outgoing_phase_transitions: Vec<Rc<PhaseTransition>>,
    incoming_phase_transitions: Vec<Rc<PhaseTransition>>,
    rankings: Vec<Ranking>,
}

impl Phase {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn rankings(&self) -> &[Ranking] {
        &self.rankings
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
    entrants: Vec1<Rc<Entrant>>,
    unique_rank: NonZeroU32,
    rank: NonZeroU32,
}

impl Ranking {
    pub fn unique_rank(&self) -> NonZeroU32 {
        self.unique_rank
    }

    pub fn rank(&self) -> NonZeroU32 {
        self.rank
    }

    pub fn entrants(&self) -> &[Rc<Entrant>] {
        &self.entrants
    }
}
