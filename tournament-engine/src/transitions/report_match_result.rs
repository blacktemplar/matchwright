use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, MatchId, MatchResult, PhaseId},
    transitions::CompetitionTransition,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct ReportMatchResult {
    phase_id: PhaseId,
    match_id: MatchId,
    match_result: MatchResult,
}

impl ReportMatchResult {
    pub fn new(phase_id: PhaseId, match_id: MatchId, match_result: MatchResult) -> Self {
        Self {
            phase_id,
            match_id,
            match_result,
        }
    }

    pub fn phase_id(&self) -> PhaseId {
        self.phase_id
    }

    pub fn match_id(&self) -> MatchId {
        self.match_id
    }

    pub fn match_result(&self) -> MatchResult {
        self.match_result
    }
}

#[derive(Error, Debug)]
pub enum ReportMatchResultError {
    #[error("phase does not exist")]
    PhaseDoesNotExist,
    #[error("match does not exist")]
    MatchDoesNotExist,
    #[error("match already finished")]
    MatchAlreadyFinished,
    #[error("the phase format does not allow draws")]
    DrawNotAllowed,
}

impl Transition<Competition> for ReportMatchResult {
    fn check_applicable(&self, competition: &Competition) -> Result<(), TransitionError> {
        let phase = competition
            .phases()
            .get(&self.phase_id)
            .ok_or(ReportMatchResultError::PhaseDoesNotExist)?;
        let r#match = phase
            .data()
            .matches()
            .get(&self.match_id)
            .ok_or(ReportMatchResultError::MatchDoesNotExist)?;
        if r#match.is_finished() {
            return Err(Box::new(ReportMatchResultError::MatchAlreadyFinished));
        }
        if self.match_result == MatchResult::Draw && !phase.format().allows_draws() {
            return Err(Box::new(ReportMatchResultError::DrawNotAllowed));
        }
        Ok(())
    }

    fn apply(
        &self,
        competition: &mut Competition,
    ) -> Result<(), undoable_state_machine::TransitionError> {
        let phase = competition.phases_mut().get_mut(&self.phase_id).unwrap();
        phase.report_match_result(self.match_id, self.match_result);
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::report_match_result")]
impl CompetitionTransition for ReportMatchResult {}
