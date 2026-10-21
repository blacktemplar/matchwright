//! The transitions that drive a [`Competition`] forward, and the trait that makes them
//! persistable.

use std::{any::Any, fmt::Debug};

use thiserror::Error;
use undoable_state_machine::{
    LogEntry, LogEntryKind, SessionHistory, StateMachine, StateMachineError, Transition,
    TransitionId,
};

use crate::{
    entities::competition::{Competition, MatchId, PhaseId},
    plugins::PhasePlugins,
};

pub mod add_entrant;
pub mod add_phase;
pub mod remove_entrant;
pub mod report_match_result;
pub mod set_competition_seed;
pub mod start_competition;

pub use add_entrant::{AddEntrant, AddEntrantError};
pub use add_phase::{AddPhase, AddPhaseError};
pub use remove_entrant::{RemoveEntrant, RemoveEntrantError};
pub use report_match_result::{ReportMatchResult, ReportMatchResultError};
pub use set_competition_seed::SetCompetitionSeed;
pub use start_competition::{StartCompetition, StartCompetitionError};

/// A [`Transition`] of a [`Competition`] that can be written to and read back from a transition
/// log, which is what gives `Box<dyn CompetitionTransition>` a [`Serialize`] and a [`Deserialize`]
/// impl.
///
/// The trait adds no methods of its own: implementing it is purely the statement that this
/// transition is part of the persisted log. An implementor therefore needs
/// [`Serialize`]/[`Deserialize`] and an explicit, namespaced `typetag` name. That name is a
/// persistence contract: renaming or moving a transition must not change it.
///
/// [`Any`] is a supertrait, so a transition in a log can be downcast back to its concrete type -
/// `&dyn Any` upcasting is how [`CompetitionStateMachineExt`] finds the entry to undo.
///
/// [`Serialize`]: serde::Serialize
/// [`Deserialize`]: serde::Deserialize
///
/// # Examples
/// ```
/// use serde::{Deserialize, Serialize};
/// use tournament_engine::{Competition, CompetitionTransition};
/// use undoable_state_machine::{Transition, TransitionError};
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct DoNothing;
///
/// impl Transition<Competition> for DoNothing {
///     fn check_applicable(&self, _competition: &Competition) -> Result<(), TransitionError> {
///         Ok(())
///     }
///
///     fn apply(&self, _competition: &mut Competition) -> Result<(), TransitionError> {
///         Ok(())
///     }
/// }
///
/// #[typetag::serde(name = "example::do_nothing")]
/// impl CompetitionTransition for DoNothing {}
///
/// let transition: Box<dyn CompetitionTransition> = Box::new(DoNothing);
/// let json = serde_json::to_string(&transition).unwrap();
///
/// assert_eq!(json, r#"{"type":"example::do_nothing","value":null}"#);
/// assert!(serde_json::from_str::<Box<dyn CompetitionTransition>>(&json).is_ok());
/// ```
// `Transition` is generic over its state and `typetag` rejects generic traits in `de` mode
// ("deserialization of generic traits is not supported yet"), so it cannot be tagged upstream.
// Pinning the state to `Competition` here is what makes the trait non-generic and taggable.
#[typetag::serde(tag = "type", content = "value")]
pub trait CompetitionTransition: Transition<Competition> + Debug + Any {}

/// The state machine of a competition, driven by transitions that can be persisted.
///
/// Serializes as its append-only log, undo entries included. Deserializing replays that log and
/// recomputes [`StateMachine::current`] from it rather than reading a stored copy, so a log that
/// does not replay cleanly fails to deserialize.
///
/// This is the persistence path that keeps the history and therefore undo and redo. The other one
/// is a snapshot of [`StateMachine::current`] alone, which is smaller and cheaper to load.
pub type CompetitionStateMachine = StateMachine<Competition, Box<dyn CompetitionTransition>>;

/// A transition that only applies before the competition starts was applied after it started.
#[derive(Debug, Error)]
#[error("competition already started")]
pub struct CompetitionAlreadyStarted;

/// Fails with [`CompetitionAlreadyStarted`] if `competition` has started.
pub(crate) fn ensure_not_started(
    competition: &Competition,
) -> Result<(), CompetitionAlreadyStarted> {
    if competition.is_started() {
        Err(CompetitionAlreadyStarted)
    } else {
        Ok(())
    }
}

/// Why the result reported for a match could not be undone.
#[derive(Debug, Error)]
pub enum UndoMatchResultError {
    #[error("no result that is in effect was reported for this match")]
    NotReported,
    /// The undo was rejected by the state machine, which for an undo means that a later change in
    /// the log depends on the undone one — e.g. the reported result of a match that is only
    /// scheduled because of it. The error names the change that would no longer apply.
    #[error(transparent)]
    Rejected(#[from] StateMachineError),
}

/// Why a phase could not be reconfigured.
#[derive(Debug, Error)]
pub enum ReconfigurePhaseError {
    #[error("no phase with this id was added")]
    NotAdded,
    /// The reconfiguration was rejected by the state machine, because the phase cannot be built
    /// from the config, or a later change in the log does not apply to the reconfigured phase, e.g.
    /// the result reported for a match its new format does not schedule. The error names the
    /// change that would no longer apply.
    #[error(transparent)]
    Rejected(#[from] StateMachineError),
}

/// Applying a transition without building its log entry, undoing a change by what it did, and
/// reconfiguring a phase by its id rather than by the [`TransitionId`] of the log entry that added
/// it, which is all [`CompetitionStateMachine`] itself takes.
///
/// What a session appends as one of its actions goes through its [`SessionHistory`], so that
/// [`SessionHistory::undo_last`] and [`SessionHistory::redo_last`] undo and redo it. A
/// [`SetCompetitionSeed`] is better applied without one, since everything random that follows
/// depends on it.
pub trait CompetitionStateMachineExt {
    /// Applies `transition` as a new [`LogEntryKind::Apply`] entry with a fresh [`TransitionId`],
    /// and returns that id, which a later undo can target. The entry is no action of any
    /// [`SessionHistory`].
    ///
    /// # Errors
    /// Returns the error of [`StateMachine::apply`] if `transition` does not apply to the current
    /// state, in which case the log stays unchanged.
    fn apply_transition(
        &mut self,
        transition: impl CompetitionTransition,
    ) -> Result<TransitionId, StateMachineError>;

    /// Applies `transition` like [`Self::apply_transition`], as an action of the session of
    /// `history`, see [`SessionHistory::apply`].
    ///
    /// # Errors
    /// Returns what [`Self::apply_transition`] returns, in which case neither the log nor
    /// `history` changes.
    fn apply_in(
        &mut self,
        history: &mut SessionHistory,
        transition: impl CompetitionTransition,
    ) -> Result<TransitionId, StateMachineError>;

    /// Undoes the result reported for the match `match_id` of the phase `phase_id` by undoing the
    /// latest log entry that is still in effect and reported it, as an action of the session of
    /// `history`, and returns the id of the undo entry appended for that. Undoing that action, see
    /// [`SessionHistory::undo_last`], brings the result back.
    ///
    /// This reaches back arbitrarily far: changes made after the report stay in effect, as long as
    /// none of them depends on it.
    ///
    /// Only a [`ReportMatchResult`] is recognized as having reported a result. A third-party
    /// transition that sets one cannot be found in the log and is therefore reported as
    /// [`UndoMatchResultError::NotReported`].
    ///
    /// # Errors
    /// Returns [`UndoMatchResultError::NotReported`] if no report for that match is in effect, and
    /// [`UndoMatchResultError::Rejected`] if undoing it would leave a later change in the log
    /// inapplicable. In either case neither the log nor `history` changes.
    ///
    /// # Examples
    /// ```
    /// # use tournament_engine::{
    /// #     AddEntrant, AddPhase, CompetitionStateMachine, CompetitionStateMachineExt, Entrant,
    /// #     EntrantId, MatchResult, PhaseId, PhasePluginsBuilder, ReportMatchResult, Seed,
    /// #     SetCompetitionSeed, StartCompetition,
    /// #     undoable_state_machine::SessionHistory, uuid::Uuid,
    /// # };
    /// # let phase_id = PhaseId::new(Uuid::from_u128(1));
    /// # let mut machine = CompetitionStateMachine::default();
    /// # machine.apply_transition(SetCompetitionSeed::new(Seed::new(*b"doc-test!!!!")))?;
    /// # for entrant in 0..4 {
    /// #     let entrant = Entrant::new(EntrantId::new(Uuid::from_u128(entrant)));
    /// #     machine.apply_transition(AddEntrant::new(entrant))?;
    /// # }
    /// # let plugins = PhasePluginsBuilder::new().build()?;
    /// # machine.apply_transition(AddPhase::new(phase_id, plugins))?;
    /// # machine.apply_transition(StartCompetition)?;
    /// // a round robin between four entrants, with the first and the last match reported
    /// let matches: Vec<_> = machine.current().phases()[&phase_id]
    ///     .data()
    ///     .matches()
    ///     .keys()
    ///     .copied()
    ///     .collect();
    /// for index in [0, 5] {
    ///     machine.apply_transition(ReportMatchResult::new(
    ///         phase_id,
    ///         matches[index],
    ///         MatchResult::AWins,
    ///     ))?;
    /// }
    ///
    /// let mut history = SessionHistory::new();
    /// machine.undo_match_result(&mut history, phase_id, matches[0])?;
    ///
    /// let phase = machine.current().phases()[&phase_id].data();
    /// assert_eq!(phase.matches()[&matches[0]].result(), None);
    /// assert_eq!(
    ///     phase.matches()[&matches[5]].result(),
    ///     Some(MatchResult::AWins)
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    fn undo_match_result(
        &mut self,
        history: &mut SessionHistory,
        phase_id: PhaseId,
        match_id: MatchId,
    ) -> Result<TransitionId, UndoMatchResultError>;

    /// Reconfigures the phase `phase_id` as if it had been added with `plugins`, as an action of
    /// the session of `history`, and returns the id of the entry appended for that: a
    /// [`LogEntryKind::Replace`] of the [`AddPhase`] in effect that added it by one adding it with
    /// `plugins`. Every change made after it applies to the reconfigured phase, so the results
    /// reported so far stay as long as the reconfigured phase schedules their matches as well.
    ///
    /// The plugins are built from a draft holding the config of the phase, see
    /// [`Phase::config`](crate::Phase::config), changed as needed. Returns `None`, leaving the log
    /// unchanged, if they have the config of the phase already.
    ///
    /// # Errors
    /// Returns [`ReconfigurePhaseError::NotAdded`] if no [`AddPhase`] in effect added a phase with
    /// this id, and [`ReconfigurePhaseError::Rejected`] if `plugins` do not fit together in the
    /// competition or a change made after the phase was added does not apply to the
    /// reconfigured phase. In either case neither the log nor `history` changes.
    ///
    /// # Examples
    /// ```
    /// # use tournament_engine::{
    /// #     AddEntrant, AddPhase, CompetitionStateMachine, CompetitionStateMachineExt, Entrant,
    /// #     EntrantId, PhaseId, PhasePluginsBuilder, Seed, SetCompetitionSeed, StartCompetition,
    /// #     plugin_factory::config::PluginConfig, plugins::phase_format::RandomRounds,
    /// #     undoable_state_machine::SessionHistory, uuid::Uuid,
    /// # };
    /// # let phase_id = PhaseId::new(Uuid::from_u128(1));
    /// # let mut machine = CompetitionStateMachine::default();
    /// # machine.apply_transition(SetCompetitionSeed::new(Seed::new(*b"doc-test!!!!")))?;
    /// # for entrant in 0..4 {
    /// #     let entrant = Entrant::new(EntrantId::new(Uuid::from_u128(entrant)));
    /// #     machine.apply_transition(AddEntrant::new(entrant))?;
    /// # }
    /// # let plugins = PhasePluginsBuilder::new().build()?;
    /// # machine.apply_transition(AddPhase::new(phase_id, plugins))?;
    /// # machine.apply_transition(StartCompetition)?;
    /// // a started round robin, changed into two random rounds of it
    /// let competition = machine.current();
    /// let plugins = competition
    ///     .phase_draft()
    ///     .with_answers(&competition.phases()[&phase_id].config())?
    ///     .with(
    ///         "format",
    ///         PluginConfig::of::<RandomRounds>().with("rounds", 2),
    ///     )?
    ///     .build()
    ///     .expect("the plugins fit together");
    ///
    /// let mut history = SessionHistory::new();
    /// machine.reconfigure_phase(&mut history, phase_id, plugins)?;
    ///
    /// // the phase was started in its new format: two rounds of two matches
    /// let phase = &machine.current().phases()[&phase_id];
    /// assert_eq!(
    ///     format!("{:?}", phase.format()),
    ///     "RandomRounds { rounds: 2 }"
    /// );
    /// assert_eq!(phase.data().matches().len(), 4);
    ///
    /// // undoing the reconfiguration plays the phase as a round robin again
    /// history.undo_last(&mut machine)?;
    /// let phase = &machine.current().phases()[&phase_id];
    /// assert_eq!(format!("{:?}", phase.format()), "RoundRobin");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    fn reconfigure_phase(
        &mut self,
        history: &mut SessionHistory,
        phase_id: PhaseId,
        plugins: PhasePlugins,
    ) -> Result<Option<TransitionId>, ReconfigurePhaseError>;
}

impl CompetitionStateMachineExt for CompetitionStateMachine {
    fn apply_transition(
        &mut self,
        transition: impl CompetitionTransition,
    ) -> Result<TransitionId, StateMachineError> {
        let id = TransitionId::new();
        self.apply(LogEntry::new(id, LogEntryKind::Apply(Box::new(transition))))?;
        Ok(id)
    }

    fn apply_in(
        &mut self,
        history: &mut SessionHistory,
        transition: impl CompetitionTransition,
    ) -> Result<TransitionId, StateMachineError> {
        let id = TransitionId::new();
        history.apply(
            self,
            LogEntry::new(id, LogEntryKind::Apply(Box::new(transition))),
        )?;
        Ok(id)
    }

    fn undo_match_result(
        &mut self,
        history: &mut SessionHistory,
        phase_id: PhaseId,
        match_id: MatchId,
    ) -> Result<TransitionId, UndoMatchResultError> {
        let target = self
            .rev_in_effect()
            .find(|entry| {
                applied::<ReportMatchResult>(entry).is_some_and(|report| {
                    (report.phase_id(), report.match_id()) == (phase_id, match_id)
                })
            })
            .ok_or(UndoMatchResultError::NotReported)?
            .id();
        let id = TransitionId::new();
        history.apply(self, LogEntry::new(id, LogEntryKind::Undo(target)))?;
        Ok(id)
    }

    fn reconfigure_phase(
        &mut self,
        history: &mut SessionHistory,
        phase_id: PhaseId,
        plugins: PhasePlugins,
    ) -> Result<Option<TransitionId>, ReconfigurePhaseError> {
        let target = added_phase(self, phase_id).ok_or(ReconfigurePhaseError::NotAdded)?;
        if self
            .current()
            .phase(&phase_id)
            .is_some_and(|phase| phase.config() == plugins.config())
        {
            return Ok(None);
        }
        let id = TransitionId::new();
        let with: Box<dyn CompetitionTransition> = Box::new(AddPhase::new(phase_id, plugins));
        history.apply(
            self,
            LogEntry::new(id, LogEntryKind::Replace { target, with }),
        )?;
        Ok(Some(id))
    }
}

/// The id of the [`AddPhase`] entry in effect that added the phase `phase_id`.
///
/// A replacement of an [`AddPhase`] made by [`CompetitionStateMachineExt::reconfigure_phase`]
/// adds the same phase, so the entry is found by the id of the phase it adds itself.
fn added_phase(machine: &CompetitionStateMachine, phase_id: PhaseId) -> Option<TransitionId> {
    machine
        .rev_in_effect()
        .find(|entry| applied::<AddPhase>(entry).is_some_and(|added| added.id() == phase_id))
        .map(LogEntry::id)
}

/// The transition `entry` applies, if it applies one of type `T`.
fn applied<T: CompetitionTransition>(
    entry: &LogEntry<Box<dyn CompetitionTransition>>,
) -> Option<&T> {
    let LogEntryKind::Apply(transition) = entry.kind() else {
        return None;
    };
    let transition: &dyn Any = &**transition;
    transition.downcast_ref()
}

#[cfg(test)]
mod tests {
    use plugin_factory::config::Config;
    use uuid::Uuid;

    use super::*;
    use crate::{
        Entrant, EntrantId, MatchResult, PhasePluginsBuilder, Seed,
        plugins::{
            metrics_calculator::{WinsDrawsLosses, WinsDrawsLossesCalculator},
            phase_format::{MonsterDyp, SingleElimination},
        },
    };

    fn phase_id() -> PhaseId {
        PhaseId::new(Uuid::from_u128(0xfa5e))
    }

    #[track_caller]
    fn undo(machine: &mut CompetitionStateMachine, target: TransitionId) -> TransitionId {
        let id = TransitionId::new();
        machine
            .apply(LogEntry::new(id, LogEntryKind::Undo(target)))
            .expect("the undo must be accepted");
        id
    }

    /// A started knockout between four entrants, which schedules two semifinals and, once both of
    /// them are reported, the final.
    fn knockout() -> CompetitionStateMachine {
        let mut machine = CompetitionStateMachine::default();
        machine
            .apply_transition(SetCompetitionSeed::new(Seed::new(*b"undo-test!!!")))
            .unwrap();
        for entrant in 0..4 {
            let entrant = Entrant::new(EntrantId::new(Uuid::from_u128(entrant)));
            machine.apply_transition(AddEntrant::new(entrant)).unwrap();
        }
        let plugins = PhasePluginsBuilder::new()
            .format(SingleElimination::new())
            .build()
            .unwrap();
        machine
            .apply_transition(AddPhase::new(phase_id(), plugins))
            .unwrap();
        machine.apply_transition(StartCompetition).unwrap();
        machine
    }

    /// The config of the phase as it is played.
    fn config(machine: &CompetitionStateMachine) -> Config {
        machine.current().phases()[&phase_id()].config()
    }

    /// The matches of the phase, in the order they were scheduled in.
    fn matches(machine: &CompetitionStateMachine) -> Vec<MatchId> {
        machine.current().phases()[&phase_id()]
            .data()
            .matches()
            .keys()
            .copied()
            .collect()
    }

    fn result_of(machine: &CompetitionStateMachine, match_id: MatchId) -> Option<MatchResult> {
        machine.current().phases()[&phase_id()].data().matches()[&match_id].result()
    }

    #[track_caller]
    fn report(machine: &mut CompetitionStateMachine, match_id: MatchId) -> TransitionId {
        machine
            .apply_transition(ReportMatchResult::new(
                phase_id(),
                match_id,
                MatchResult::AWins,
            ))
            .expect("the match must be reportable")
    }

    #[test]
    fn a_competition_takes_a_single_phase() {
        let mut machine = CompetitionStateMachine::default();
        let plugins = PhasePluginsBuilder::new().build().unwrap();
        machine
            .apply_transition(AddPhase::new(phase_id(), plugins.clone()))
            .unwrap();

        let second = AddPhase::new(PhaseId::new(Uuid::from_u128(2)), plugins);

        assert!(machine.apply_transition(second).is_err());
        assert_eq!(machine.current().phases().len(), 1);
    }

    /// Undoing the result of a match is an action of the session, so undoing that action brings
    /// the result back.
    #[test]
    fn undoing_the_undo_of_a_match_result_brings_it_back() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        report(&mut machine, first);
        let mut history = SessionHistory::new();
        machine
            .undo_match_result(&mut history, phase_id(), first)
            .unwrap();

        history
            .undo_last(&mut machine)
            .expect("the undo of the result is an action of the session");

        assert_eq!(result_of(&machine, first), Some(MatchResult::AWins));
    }

    /// Undoing the earlier of two reports leaves the later one in effect, which is the whole point
    /// of undoing by match rather than by walking the log back from its end.
    #[test]
    fn a_report_is_undone_without_touching_the_reports_made_after_it() {
        let mut machine = knockout();
        let [first, second] = matches(&machine)[..] else {
            panic!("a knockout between four entrants starts with two semifinals");
        };
        report(&mut machine, first);
        report(&mut machine, second);

        machine
            .undo_match_result(&mut SessionHistory::new(), phase_id(), first)
            .expect("the report of the first semifinal must be undoable");

        assert_eq!(result_of(&machine, first), None);
        assert_eq!(result_of(&machine, second), Some(MatchResult::AWins));
        assert_eq!(
            matches(&machine).len(),
            2,
            "the final is only scheduled once both semifinals are reported"
        );
    }

    #[test]
    fn a_match_that_was_never_reported_has_nothing_to_undo() {
        let mut machine = knockout();
        let open = matches(&machine)[0];

        let error = machine
            .undo_match_result(&mut SessionHistory::new(), phase_id(), open)
            .expect_err("an open match must not be undoable");

        assert!(matches!(error, UndoMatchResultError::NotReported));
    }

    #[test]
    fn a_report_that_is_already_undone_is_not_undone_twice() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        report(&mut machine, first);
        let mut history = SessionHistory::new();
        machine
            .undo_match_result(&mut history, phase_id(), first)
            .unwrap();
        let log_len = machine.log().len();

        let error = machine
            .undo_match_result(&mut history, phase_id(), first)
            .expect_err("an undone report must not be undoable again");

        assert!(matches!(error, UndoMatchResultError::NotReported));
        assert_eq!(machine.log().len(), log_len, "the log must stay unchanged");
    }

    /// Only the report that is in effect is undone, which after an undo and a redo of the same
    /// match is an earlier log entry than the one that reported it last.
    #[test]
    fn the_report_that_was_put_back_in_effect_is_the_one_undone() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        let reported = report(&mut machine, first);
        let undone = undo(&mut machine, reported);
        // reporting the match again and undoing that report leaves the first report the only one
        // that any further undo could still cancel
        let reported_again = report(&mut machine, first);
        undo(&mut machine, reported_again);
        undo(&mut machine, undone);
        assert_eq!(result_of(&machine, first), Some(MatchResult::AWins));

        let id = machine
            .undo_match_result(&mut SessionHistory::new(), phase_id(), first)
            .unwrap();

        assert_eq!(result_of(&machine, first), None);
        assert!(matches!(
            machine.log().last().expect("the undo entry").kind(),
            LogEntryKind::Undo(target) if *target == reported,
        ));
        assert_ne!(id, reported);
    }

    /// A knockout schedules its final from the reported semifinals, so undoing a semifinal would
    /// leave the report of the final without a match to report, which the state machine rejects
    /// rather than dropping that report.
    #[test]
    fn a_report_a_later_report_depends_on_is_not_undone() {
        let mut machine = knockout();
        let semifinals = matches(&machine);
        report(&mut machine, semifinals[0]);
        report(&mut machine, semifinals[1]);
        let r#final = matches(&machine)[2];
        report(&mut machine, r#final);
        let log_len = machine.log().len();

        let error = machine
            .undo_match_result(&mut SessionHistory::new(), phase_id(), semifinals[0])
            .expect_err("the final depends on the semifinal, so undoing it must be rejected");

        assert!(matches!(error, UndoMatchResultError::Rejected(_)));
        assert_eq!(result_of(&machine, semifinals[0]), Some(MatchResult::AWins));
        assert_eq!(result_of(&machine, r#final), Some(MatchResult::AWins));
        assert_eq!(machine.log().len(), log_len, "the log must stay unchanged");
    }

    /// A reconfigured phase is played as if it had been added that way, so the results of the
    /// matches it schedules as well stay reported.
    #[test]
    fn a_reconfigured_phase_keeps_the_results_of_its_matches() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        report(&mut machine, first);
        let with_record = PhasePluginsBuilder::new()
            .format(SingleElimination::new())
            .add_metric(WinsDrawsLossesCalculator)
            .build()
            .unwrap();

        machine
            .reconfigure_phase(&mut SessionHistory::new(), phase_id(), with_record.clone())
            .unwrap()
            .expect("the plugins changed");

        assert_eq!(config(&machine), with_record.config());
        let phase = &machine.current().phases()[&phase_id()];
        assert_eq!(phase.metrics_calculators().len(), 1);
        assert_eq!(result_of(&machine, first), Some(MatchResult::AWins));
        let winner = phase.data().matches()[&first].team_a()[0];
        let record = phase.data().rankings()[&winner].metric::<WinsDrawsLosses>();
        assert_eq!(record.map(|record| record.wins), Some(1));
    }

    /// The config of a started phase leaves out the runtime state of its plugins, such as the
    /// bracket of a knockout, so the plugins a draft builds from it are the ones the phase was
    /// added with.
    #[test]
    fn reconfiguring_a_phase_with_its_own_config_changes_nothing() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        report(&mut machine, first);
        let unchanged = machine
            .current()
            .phase_draft()
            .with_answers(&config(&machine))
            .unwrap()
            .build()
            .unwrap();
        let log_len = machine.log().len();

        assert!(matches!(
            machine.reconfigure_phase(&mut SessionHistory::new(), phase_id(), unchanged),
            Ok(None)
        ));
        assert_eq!(machine.log().len(), log_len, "the log must stay unchanged");
    }

    #[test]
    fn a_phase_that_was_not_added_cannot_be_reconfigured() {
        let mut machine = knockout();
        let unknown = PhaseId::new(Uuid::from_u128(2));

        assert!(matches!(
            machine.reconfigure_phase(
                &mut SessionHistory::new(),
                unknown,
                PhasePluginsBuilder::new().build().unwrap()
            ),
            Err(ReconfigurePhaseError::NotAdded)
        ));
    }

    /// The teams of a Monster DYP are pairs, so it never schedules the match a knockout of single
    /// entrants reported a result for.
    #[test]
    fn a_reconfiguration_a_reported_result_does_not_fit_is_rejected() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        report(&mut machine, first);
        let log_len = machine.log().len();

        let monster_dyp = PhasePluginsBuilder::new()
            .format(MonsterDyp::new(1))
            .build()
            .unwrap();

        let result = machine.reconfigure_phase(&mut SessionHistory::new(), phase_id(), monster_dyp);

        assert!(matches!(result, Err(ReconfigurePhaseError::Rejected(_))));
        assert_eq!(machine.log().len(), log_len, "the log must stay unchanged");
    }

    #[test]
    fn a_reconfiguration_is_undone_and_redone_like_any_action() {
        let mut machine = knockout();
        let knockout = config(&machine);
        let monster_dyp = PhasePluginsBuilder::new()
            .format(MonsterDyp::new(1))
            .build()
            .unwrap();
        let mut history = SessionHistory::new();
        machine
            .reconfigure_phase(&mut history, phase_id(), monster_dyp.clone())
            .unwrap();

        history
            .undo_last(&mut machine)
            .expect("the reconfiguration is in effect");
        assert_eq!(config(&machine), knockout);
        history
            .redo_last(&mut machine)
            .expect("the reconfiguration was undone last");

        assert_eq!(config(&machine), monster_dyp.config());
    }

    /// Undoing a reconfiguration changes the phase every change after it applies to, so it is
    /// rejected if a change another session made since does not apply to the phase as it was
    /// configured before.
    #[test]
    fn undoing_a_reconfiguration_a_result_of_another_session_depends_on_is_rejected() {
        let mut machine = knockout();
        let mut history = SessionHistory::new();
        let monster_dyp = PhasePluginsBuilder::new()
            .format(MonsterDyp::new(1))
            .build()
            .unwrap();
        machine
            .reconfigure_phase(&mut history, phase_id(), monster_dyp)
            .unwrap();
        // the teams of a Monster DYP are pairs, so the knockout never schedules this match
        let first = matches(&machine)[0];
        report(&mut machine, first);
        let log_len = machine.log().len();

        let result = history.undo_last(&mut machine);

        assert!(matches!(
            result,
            Err(undoable_state_machine::UndoLastError::Rejected(_))
        ));
        assert_eq!(machine.log().len(), log_len, "the log must stay unchanged");
    }
}
