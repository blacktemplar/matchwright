use thiserror::Error;
use undoable_state_machine::Transition;

use crate::entities::competition::Competition;

pub struct StartCompetition;

#[derive(Error, Debug)]
pub enum StartCompetitionError {
    #[error("competition already started")]
    CompetitionAlreadyStarted,
    #[error("too few entrants")]
    TooFewEntrants,
}

impl Transition<Competition> for StartCompetition {
    fn check_applicable(
        &self,
        competition: &Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        if competition.is_started() {
            return Err(Box::new(StartCompetitionError::CompetitionAlreadyStarted));
        }
        if competition.entrants().len() < 2 {
            return Err(Box::new(StartCompetitionError::TooFewEntrants));
        }
        Ok(())
    }

    fn apply(
        &self,
        competition: &mut Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        competition.start();
        Ok(())
    }
}
