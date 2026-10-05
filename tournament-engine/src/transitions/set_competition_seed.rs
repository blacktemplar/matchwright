use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::entities::competition::Competition;

pub struct SetCompetitionSeed {
    seed: [u8; 12],
}

impl SetCompetitionSeed {
    pub fn new(seed: [u8; 12]) -> Self {
        Self { seed }
    }
}

#[derive(Error, Debug)]
pub enum SetCompetitionSeedError {
    #[error("competition already started")]
    CompetitionAlreadyStarted,
}

impl Transition<Competition> for SetCompetitionSeed {
    fn check_applicable(&self, state: &Competition) -> Result<(), TransitionError> {
        if state.is_started() {
            return Err(Box::new(SetCompetitionSeedError::CompetitionAlreadyStarted));
        }
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.set_seed(self.seed);
        Ok(())
    }
}
