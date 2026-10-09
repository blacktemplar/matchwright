use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, PhaseId},
    plugins::{
        PhasePlugins,
        compatibility::{Declaration, Incompatibility, check_facts},
        metrics_calculator::MetricsCalculator,
        phase_format::PhaseFormat,
        ranker::Ranker,
    },
    transitions::CompetitionTransition,
};

/// Adds a phase to a competition that has not started yet.
///
/// Applying it fails if a fact one of its plugins requires is neither provided by another one nor
/// by the competition, or does not meet the condition it is required with, see
/// [`Plugin::declare`](crate::plugins::Plugin::declare).
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
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

impl TryFrom<SerializedAddPhase> for AddPhase {
    type Error = Incompatibility;

    fn try_from(serialized: SerializedAddPhase) -> Result<Self, Self::Error> {
        let plugins = PhasePlugins::new(
            serialized.phase_format,
            serialized.ranker,
            serialized.metrics_calculators,
        );
        plugins.check_order()?;
        Ok(Self {
            id: serialized.id,
            plugins,
        })
    }
}

impl From<AddPhase> for SerializedAddPhase {
    fn from(add_phase: AddPhase) -> Self {
        let PhasePlugins {
            phase_format,
            ranker,
            metrics_calculators,
        } = add_phase.plugins;
        Self {
            id: add_phase.id,
            phase_format,
            ranker,
            metrics_calculators,
        }
    }
}

impl AddPhase {
    /// Adds the phase `id`, played in `phase_format`, ranked by `ranker` and calculating the
    /// metrics of `metrics_calculators`.
    ///
    /// For every fact a plugin requires that nothing provides, the
    /// [default provider](crate::plugins::compatibility::DefaultProvider) of the fact is added, if
    /// it has one. The metrics calculators are ordered so that each one runs after the calculators
    /// providing the facts it requires, keeping the given order otherwise; [`AddPhase::plugins`]
    /// returns the result.
    ///
    /// # Errors
    /// Returns an error if metrics calculators require facts of each other in a cycle.
    pub fn new(
        id: PhaseId,
        phase_format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Result<Self, Incompatibility> {
        let mut plugins = PhasePlugins::new(phase_format, ranker, metrics_calculators);
        plugins.resolve()?;
        Ok(Self { id, plugins })
    }

    /// The plugins of the phase, including the ones added by [`AddPhase::new`].
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
        let context = Declaration::of(|declaration| competition.declare_phase_context(declaration));
        check_facts(context, &self.plugins.declarations())?;
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.add_phase(self.id, self.plugins.clone());
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::add_phase")]
impl CompetitionTransition for AddPhase {}
