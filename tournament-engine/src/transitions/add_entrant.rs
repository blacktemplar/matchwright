use thiserror::Error;
use undoable_state_machine::Transition;

use crate::entities::competition::{Competition, Entrant};

pub struct AddEntrant {
    entrant: Entrant,
}

impl AddEntrant {
    pub fn new(entrant: Entrant) -> Self {
        Self { entrant }
    }
}

#[derive(Error, Debug)]
enum AddEntrantError {
    #[error("competition already started")]
    CompetitionAlreadyStarted,
    #[error("entrant is already attending")]
    EntrantIsAlreadyAttending,
}

impl Transition<Competition> for AddEntrant {
    fn check_applicable(
        &self,
        competition: &Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        if competition.is_started() {
            return Err(Box::new(AddEntrantError::CompetitionAlreadyStarted));
        }
        if competition.entrants().contains_key(&self.entrant.id()) {
            return Err(Box::new(AddEntrantError::EntrantIsAlreadyAttending));
        }
        Ok(())
    }

    fn apply(
        &self,
        competition: &mut Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        competition.add_entrant(self.entrant.clone());
        Ok(())
    }
}
