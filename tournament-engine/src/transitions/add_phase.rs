use plugin_factory::{
    compatibility::{self, Incompatibility},
    schema::Configurable,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, PhaseId},
    plugins::{OrderError, PhasePlugins},
    transitions::CompetitionTransition,
};

/// Adds a phase to a competition that has not started yet.
///
/// Applying it fails if the plugins of the phase do not fit together in the competition, see
/// [`compatibility::check`] with [`Competition::declare_phase_context`] as the context.
///
/// Deserializing one fails if its metrics calculators are not ordered as [`AddPhase::new`] orders
/// them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "SerializedAddPhase", into = "SerializedAddPhase")]
pub struct AddPhase {
    id: PhaseId,
    plugins: PhasePlugins,
}

/// The form an [`AddPhase`] is serialized in.
#[derive(Serialize, Deserialize)]
struct SerializedAddPhase {
    id: PhaseId,
    plugins: PhasePlugins,
}

impl TryFrom<SerializedAddPhase> for AddPhase {
    type Error = OrderError;

    fn try_from(serialized: SerializedAddPhase) -> Result<Self, Self::Error> {
        serialized.plugins.check_order()?;
        Ok(Self {
            id: serialized.id,
            plugins: serialized.plugins,
        })
    }
}

impl From<AddPhase> for SerializedAddPhase {
    fn from(add_phase: AddPhase) -> Self {
        Self {
            id: add_phase.id,
            plugins: add_phase.plugins,
        }
    }
}

impl AddPhase {
    /// Adds the phase `id` with `plugins`, whose metrics calculators are ordered so that each one
    /// runs after the calculators providing the facts it requires, keeping the given order
    /// otherwise; [`AddPhase::plugins`] returns the result.
    ///
    /// Nothing is added to `plugins`: a fact they require that none of them provides is settled
    /// beforehand, by finishing a draft of them or by [`PhasePlugins::settle`].
    ///
    /// # Errors
    /// Returns an error if metrics calculators require facts of each other in a cycle.
    pub fn new(id: PhaseId, mut plugins: PhasePlugins) -> Result<Self, Incompatibility> {
        plugins.order_metrics()?;
        Ok(Self { id, plugins })
    }

    /// The plugins of the phase, ordered by [`AddPhase::new`].
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
