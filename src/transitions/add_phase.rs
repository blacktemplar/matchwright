use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, PhaseId},
    plugins::{metrics_calculator::MetricsCalculator, phase_format::PhaseFormat},
};

pub struct AddPhase {
    id: PhaseId,
    phase_format: Box<dyn PhaseFormat>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

impl AddPhase {
    pub fn new(
        id: PhaseId,
        phase_format: Box<dyn PhaseFormat>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Self {
        Self {
            id,
            phase_format,
            metrics_calculators,
        }
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
        state.add_phase(
            self.id,
            self.phase_format.clone(),
            self.metrics_calculators.clone(),
        );
        Ok(())
    }
}
