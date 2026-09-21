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
    #[error("no phases")]
    NoPhases,
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
        if competition.phases().is_empty() {
            return Err(Box::new(StartCompetitionError::NoPhases));
        }
        Ok(())
    }

    fn apply(
        &self,
        competition: &mut Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        competition.start();
        if competition.phases().len() > 1 {
            unimplemented!("Multi-phase competition not yet supported");
        }
        competition.phases_mut()[0].start();
        Ok(())
    }
}
