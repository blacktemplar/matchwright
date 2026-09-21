use thiserror::Error;
use undoable_state_machine::Transition;

use crate::entities::competition::{Competition, Entrant, Player};

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
    #[error("{0:?} is part of multiple entrants")]
    PlayerIsPartOfMultipleEntrants(Player),
}

impl Transition<Competition> for AddEntrant {
    fn check_applicable(
        &self,
        competition: &Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        if competition.is_started() {
            return Err(Box::new(AddEntrantError::CompetitionAlreadyStarted));
        }
        for player in self.entrant.players() {
            if let Some(entrant) = competition.entrant_of_player(player) {
                if **entrant == self.entrant {
                    return Err(Box::new(AddEntrantError::EntrantIsAlreadyAttending));
                } else {
                    return Err(Box::new(AddEntrantError::PlayerIsPartOfMultipleEntrants(
                        player.clone(),
                    )));
                }
            }
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
