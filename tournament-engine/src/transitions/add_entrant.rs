use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::Transition;

use crate::{
    entities::competition::{Competition, Entrant},
    transitions::{CompetitionTransition, ensure_not_started},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct AddEntrant {
    entrant: Entrant,
}

impl AddEntrant {
    pub fn new(entrant: Entrant) -> Self {
        Self { entrant }
    }
}

/// Why an [`AddEntrant`] does not apply, besides
/// [`CompetitionAlreadyStarted`](crate::transitions::CompetitionAlreadyStarted).
#[derive(Error, Debug)]
pub enum AddEntrantError {
    #[error("entrant is already attending")]
    EntrantIsAlreadyAttending,
}

impl Transition<Competition> for AddEntrant {
    fn check_applicable(
        &self,
        competition: &Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        ensure_not_started(competition)?;
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

#[typetag::serde(name = "tournament_engine::add_entrant")]
impl CompetitionTransition for AddEntrant {}
