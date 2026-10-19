use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::Transition;

use crate::{
    entities::competition::{Competition, EntrantId},
    transitions::{CompetitionTransition, ensure_not_started},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveEntrant {
    entrant_id: EntrantId,
}

impl RemoveEntrant {
    pub fn new(entrant_id: EntrantId) -> Self {
        Self { entrant_id }
    }
}

/// Why a [`RemoveEntrant`] does not apply, besides
/// [`CompetitionAlreadyStarted`](crate::transitions::CompetitionAlreadyStarted).
#[derive(Error, Debug)]
pub enum RemoveEntrantError {
    #[error("entrant is not attending")]
    EntrantIsNotAttending,
}

impl Transition<Competition> for RemoveEntrant {
    fn check_applicable(
        &self,
        competition: &Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        ensure_not_started(competition)?;

        if competition.entrants().contains_key(&self.entrant_id) {
            Ok(())
        } else {
            Err(Box::new(RemoveEntrantError::EntrantIsNotAttending))
        }
    }

    fn apply(
        &self,
        competition: &mut Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        competition.remove_entrant(&self.entrant_id);
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::remove_entrant")]
impl CompetitionTransition for RemoveEntrant {}
