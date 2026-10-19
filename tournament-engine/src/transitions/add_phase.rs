use plugin_factory::{compatibility, schema::Configurable};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, PhaseId},
    plugins::PhasePlugins,
    transitions::{CompetitionTransition, ensure_not_started},
};

/// Adds the phase to a competition that has not started yet. A competition has a single phase.
///
/// Applying it fails if the competition has a phase already, or if the plugins of the phase do not
/// fit together in the competition, see [`compatibility::check`] with
/// [`Competition::declare_phase_context`] as the context.
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

/// Why an [`AddPhase`] does not apply, besides
/// [`CompetitionAlreadyStarted`](crate::transitions::CompetitionAlreadyStarted) and an
/// [`Incompatibility`](compatibility::Incompatibility) of its plugins.
#[derive(Error, Debug)]
pub enum AddPhaseError {
    #[error("the competition already has a phase, and only one is supported")]
    PhaseAlreadyAdded,
}

impl Transition<Competition> for AddPhase {
    fn check_applicable(&self, competition: &Competition) -> Result<(), TransitionError> {
        ensure_not_started(competition)?;
        if !competition.phases().is_empty() {
            return Err(Box::new(AddPhaseError::PhaseAlreadyAdded));
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
