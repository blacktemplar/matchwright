use thiserror::Error;
use undoable_state_machine::Transition;

use crate::entities::competition::{Competition, Entrant};

pub struct RemoveEntrant {
    entrant: Entrant,
}

impl RemoveEntrant {
    pub fn new(entrant: Entrant) -> Self {
        Self { entrant }
    }
}

#[derive(Error, Debug)]
enum RemoveEntrantError {
    #[error("competition already started")]
    CompetitionAlreadyStarted,
    #[error("entrant is not attending")]
    EntrantIsNotAttending,
}

impl Transition<Competition> for RemoveEntrant {
    fn check_applicable(
        &self,
        competition: &Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        if competition.is_started() {
            return Err(Box::new(RemoveEntrantError::CompetitionAlreadyStarted));
        }

        if competition.entrants().contains_key(&self.entrant.id()) {
            Ok(())
        } else {
            Err(Box::new(RemoveEntrantError::EntrantIsNotAttending))
        }
    }

    fn apply(
        &self,
        competition: &mut Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        competition.remove_entrant(&self.entrant);
        Ok(())
    }
}
