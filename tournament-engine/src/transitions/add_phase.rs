use plugin_factory::{compatibility, schema::Configurable};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, PhaseId},
    plugins::PhasePlugins,
    transitions::CompetitionTransition,
};

/// Adds a phase to a competition that has not started yet.
///
/// Applying it fails if the plugins of the phase do not fit together in the competition, see
/// [`compatibility::check`] with [`Competition::declare_phase_context`] as the context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddPhase {
    id: PhaseId,
    plugins: PhasePlugins,
}

impl AddPhase {
    /// Adds the phase `id` with `plugins`.
    ///
    /// Nothing is added to `plugins`: a fact they require that none of them provides is settled
    /// beforehand, by finishing a draft of them or by [`PhasePlugins::settle`].
    pub fn new(id: PhaseId, plugins: PhasePlugins) -> Self {
        Self { id, plugins }
    }

    pub fn plugins(&self) -> &PhasePlugins {
        &self.plugins
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
        compatibility::check(
            |declaration| competition.declare_phase_context(declaration),
            &self.plugins.plugins(),
        )?;
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.add_phase(self.id, self.plugins.clone());
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::add_phase")]
impl CompetitionTransition for AddPhase {}
