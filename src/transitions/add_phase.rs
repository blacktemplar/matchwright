use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};
use uuid::Uuid;

use crate::{entities::competition::Competition, plugins::phase_format::PhaseFormat};

pub struct AddPhase {
    uuid: Uuid,
    phase_format: Box<dyn PhaseFormat>,
}

impl AddPhase {
    pub fn new(uuid: Uuid, phase_format: Box<dyn PhaseFormat>) -> Self {
        Self { uuid, phase_format }
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
        state.add_phase(self.uuid, dyn_clone::clone_box(&*self.phase_format));
        Ok(())
    }
}
