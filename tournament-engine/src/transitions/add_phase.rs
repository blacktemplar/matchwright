use serde::{Deserialize, Serialize};
use thiserror::Error;
use undoable_state_machine::{Transition, TransitionError};

use crate::{
    entities::competition::{Competition, PhaseId},
    plugins::{
        compatibility::{Declaration, Incompatibility, check_facts, resolve_metrics_calculators},
        metrics_calculator::MetricsCalculator,
        phase_format::PhaseFormat,
        ranker::Ranker,
    },
    transitions::CompetitionTransition,
};

/// Adds a phase to a competition that has not started yet.
///
/// Deserializing one fails if its metrics calculators are not complete and ordered as
/// [`AddPhase::new`] makes them.
#[derive(Debug, Serialize, Deserialize)]
#[serde(try_from = "UncheckedAddPhase")]
pub struct AddPhase {
    id: PhaseId,
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

/// An [`AddPhase`] as it is serialized, before its metrics calculators are checked.
#[derive(Deserialize)]
struct UncheckedAddPhase {
    id: PhaseId,
    phase_format: Box<dyn PhaseFormat>,
    ranker: Box<dyn Ranker>,
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

impl TryFrom<UncheckedAddPhase> for AddPhase {
    type Error = Incompatibility;

    fn try_from(unchecked: UncheckedAddPhase) -> Result<Self, Self::Error> {
        let metrics_calculators = resolve_metrics_calculators(
            &*unchecked.phase_format,
            &*unchecked.ranker,
            unchecked.metrics_calculators,
            false,
        )?;
        Ok(Self {
            id: unchecked.id,
            phase_format: unchecked.phase_format,
            ranker: unchecked.ranker,
            metrics_calculators,
        })
    }
}

impl AddPhase {
    /// Adds the phase `id`, played in `phase_format`, ranked by `ranker` and calculating the
    /// metrics of `metrics_calculators`.
    ///
    /// Every metric a plugin requires that none of `metrics_calculators` calculates is calculated
    /// by the [default calculator][default] of that metric. The calculators are ordered so that
    /// each one runs after the calculators of the metrics it requires, keeping the given order
    /// otherwise; [`AddPhase::metrics_calculators`] returns the result.
    ///
    /// # Errors
    /// Returns an error if two calculators calculate the same metric, if a required metric has
    /// neither a calculator nor a default one, or if calculators require each other's metrics in
    /// a cycle.
    ///
    /// [default]: crate::plugins::metrics_calculator::CalculatedMetric::default_calculator
    pub fn new(
        id: PhaseId,
        phase_format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Result<Self, Incompatibility> {
        let metrics_calculators =
            resolve_metrics_calculators(&*phase_format, &*ranker, metrics_calculators, true)?;
        Ok(Self {
            id,
            phase_format,
            ranker,
            metrics_calculators,
        })
    }

    /// The metrics calculators of the phase, including the ones added by [`AddPhase::new`], in the
    /// order they run in.
    pub fn metrics_calculators(&self) -> &[Box<dyn MetricsCalculator>] {
        &self.metrics_calculators
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
        let declarations: Vec<_> = [
            (
                format!("{:?}", self.phase_format),
                Declaration::of(|declaration| self.phase_format.declare(declaration)),
            ),
            (
                format!("{:?}", self.ranker),
                Declaration::of(|declaration| self.ranker.declare(declaration)),
            ),
        ]
        .into_iter()
        .chain(self.metrics_calculators.iter().map(|calculator| {
            (
                format!("{calculator:?}"),
                Declaration::of(|declaration| calculator.declare(declaration)),
            )
        }))
        .collect();
        let context = Declaration::of(|declaration| competition.declare_phase_context(declaration));
        check_facts(context, &declarations)?;
        Ok(())
    }

    fn apply(&self, state: &mut Competition) -> Result<(), TransitionError> {
        state.add_phase(
            self.id,
            self.phase_format.clone(),
            self.ranker.clone(),
            self.metrics_calculators.clone(),
        );
        Ok(())
    }
}

#[typetag::serde(name = "tournament_engine::add_phase")]
impl CompetitionTransition for AddPhase {}
