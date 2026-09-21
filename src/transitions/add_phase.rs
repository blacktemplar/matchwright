use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};
use uuid::Uuid;

use crate::entities::competition::Competition;

pub struct AddPhase {
    uuid: Uuid,
}

impl AddPhase {
    pub fn new(uuid: Uuid) -> Self {
        Self { uuid }
    }
}

#[derive(Error, Debug)]
enum AddPhaseError {
    #[error("tournament already started")]
    TournamentAlreadyStarted,
}

impl Transition<Competition> for AddPhase {
    fn check_applicable(&self, competition: &Competition) -> Result<(), TransitionError> {
        if competition.is_started() {
            return Err(Box::new(AddPhaseError::TournamentAlreadyStarted));
        }
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.add_phase(self.uuid);
        Ok(())
    }
}
