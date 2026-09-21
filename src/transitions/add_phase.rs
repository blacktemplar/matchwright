use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};
use uuid::Uuid;

use crate::entities::competition::Competition;

pub struct AddPhase {
    phase_group_index: usize,
    uuid: Uuid,
}

impl AddPhase {
    pub fn new(phase_group_index: usize, uuid: Uuid) -> Self {
        Self {
            phase_group_index,
            uuid,
        }
    }
}

#[derive(Error, Debug)]
enum AddPhaseError {
    #[error("phase group index > 0 and the previous phase group doesn't contain any phases")]
    NoPhaseInPreviousPhaseGroup,
    #[error("tournament already started")]
    TournamentAlreadyStarted,
}

impl Transition<Competition> for AddPhase {
    fn check_applicable(&self, competition: &Competition) -> Result<(), TransitionError> {
        if competition.phase_groups().len() < self.phase_group_index {
            return Err(Box::new(AddPhaseError::NoPhaseInPreviousPhaseGroup));
        }
        if competition.is_started() {
            return Err(Box::new(AddPhaseError::TournamentAlreadyStarted));
        }
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.add_phase(self.phase_group_index, self.uuid);
        Ok(())
    }
}
