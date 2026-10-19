use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::Transition;

use crate::{
    entities::competition::Competition,
    transitions::{CompetitionTransition, ensure_not_started},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct StartCompetition;

/// Why a [`StartCompetition`] does not apply, besides
/// [`CompetitionAlreadyStarted`](crate::transitions::CompetitionAlreadyStarted).
#[derive(Error, Debug)]
pub enum StartCompetitionError {
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
        ensure_not_started(competition)?;
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
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::start_competition")]
impl CompetitionTransition for StartCompetition {}
