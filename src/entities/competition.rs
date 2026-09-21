use std::{
    cmp::Ordering::{Equal, Greater, Less},
    collections::HashMap,
    rc::Rc,
};

use uuid::Uuid;
use vec1::{Vec1, vec1};

#[derive(Default)]
pub struct Competition {
    entrants: Vec<Rc<Entrant>>,
    phase_groups: Vec<PhaseGroup>,
    started: bool,
    seed: [u8; 12],
    players_to_entrants: HashMap<Player, Rc<Entrant>>,
}

impl Competition {
    pub fn is_started(&self) -> bool {
        self.started
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

    pub(crate) fn add_phase(&mut self, phase_group_index: usize, id: Uuid) {
        let phase = Rc::new(Phase {
            id,
            outgoing_phase_transitions: vec![],
            incoming_phase_transitions: vec![],
        });
        match self.phase_groups.len().cmp(&phase_group_index) {
            Greater => self.phase_groups[phase_group_index].phases.push(phase),
            Equal => self.phase_groups.push(PhaseGroup {
                phases: vec1![phase],
            }),
            Less => {
                panic!(
                    "Phase group index can be at most one more than the highest phase group index \
                    existing before the call. Make sure to add phases in order of increasing index"
                )
            }
        }
    }

    pub fn phase_groups(&self) -> &[PhaseGroup] {
        &self.phase_groups
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    players: Vec1<Player>,
}

impl Entrant {
    pub fn new(players: Vec1<Player>) -> Self {
        Self { players }
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
pub struct PhaseGroup {
    phases: Vec1<Rc<Phase>>,
}

#[derive(Debug)]
#[expect(dead_code)]
pub struct Phase {
    id: Uuid,
    outgoing_phase_transitions: Vec<Rc<PhaseTransition>>,
    incoming_phase_transitions: Vec<Rc<PhaseTransition>>,
}

#[derive(Debug)]
pub struct PhaseTransition {
    pub source_phases: Vec1<Rc<Phase>>,
    pub target_phases: Vec1<Rc<Phase>>,
}
