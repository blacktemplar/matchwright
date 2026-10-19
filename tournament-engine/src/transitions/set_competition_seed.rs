use serde::{Deserialize, Serialize};
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, Seed},
    transitions::{CompetitionTransition, ensure_not_started},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct SetCompetitionSeed {
    seed: Seed,
}

impl SetCompetitionSeed {
    pub fn new(seed: Seed) -> Self {
        Self { seed }
    }
}

impl Transition<Competition> for SetCompetitionSeed {
    fn check_applicable(&self, state: &Competition) -> Result<(), TransitionError> {
        ensure_not_started(state)?;
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.set_seed(self.seed);
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::set_competition_seed")]
impl CompetitionTransition for SetCompetitionSeed {}
