//! The transitions that drive a [`Competition`] forward, and the trait that makes them
//! persistable.

use std::{any::Any, collections::HashSet, fmt::Debug};

use thiserror::Error;
use undoable_state_machine::{
    LogEntry, LogEntryKind, StateMachine, StateMachineError, Transition, TransitionId,
};

use crate::{
    entities::competition::{Competition, MatchId, PhaseId},
    transitions::report_match_result::ReportMatchResult,
};

pub mod add_entrant;
pub mod add_phase;
pub mod remove_entrant;
pub mod report_match_result;
pub mod set_competition_seed;
pub mod start_competition;

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
/// use tournament_engine::{
///     entities::competition::Competition, transitions::CompetitionTransition,
/// };
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

/// Undoing a change by what it did rather than by the [`TransitionId`] of the log entry it was
/// applied as, which is all [`CompetitionStateMachine`] itself takes.
pub trait CompetitionStateMachineExt {
    /// Undoes the result reported for the match `match_id` of the phase `phase_id` by undoing the
    /// latest log entry that is still in effect and reported it, and returns the id of the undo
    /// entry appended for that.
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
    /// inapplicable. In either case the log stays unchanged.
    ///
    /// # Examples
    /// ```
    /// # use tournament_engine::{
    /// #     entities::competition::{Entrant, EntrantId, MatchResult, PhaseId},
    /// #     plugins::{phase_format::round_robin::RoundRobin, ranker::HierarchicalRanker},
    /// #     transitions::{
    /// #         CompetitionStateMachine, CompetitionStateMachineExt, CompetitionTransition,
    /// #         add_entrant::AddEntrant, add_phase::AddPhase,
    /// #         report_match_result::ReportMatchResult, set_competition_seed::SetCompetitionSeed,
    /// #         start_competition::StartCompetition,
    /// #     },
    /// # };
    /// # use undoable_state_machine::{LogEntry, LogEntryKind, TransitionId};
    /// # use uuid::Uuid;
    /// # fn apply(
    /// #     machine: &mut CompetitionStateMachine,
    /// #     transition: impl CompetitionTransition + 'static,
    /// # ) {
    /// #     machine
    /// #         .apply(LogEntry::new(
    /// #             TransitionId::new(),
    /// #             LogEntryKind::Apply(Box::new(transition)),
    /// #         ))
    /// #         .unwrap();
    /// # }
    /// # let phase_id = PhaseId::new(Uuid::from_u128(1));
    /// # let mut machine = CompetitionStateMachine::default();
    /// # apply(&mut machine, SetCompetitionSeed::new(*b"doc-test!!!!"));
    /// # for entrant in 0..4 {
    /// #     apply(
    /// #         &mut machine,
    /// #         AddEntrant::new(Entrant::new(EntrantId::new(Uuid::from_u128(entrant)))),
    /// #     );
    /// # }
    /// # apply(
    /// #     &mut machine,
    /// #     AddPhase::new(
    /// #         phase_id,
    /// #         Box::new(RoundRobin),
    /// #         HierarchicalRanker::default(),
    /// #         Vec::new(),
    /// #     )
    /// #     .unwrap(),
    /// # );
    /// # apply(&mut machine, StartCompetition);
    /// // a round robin between four entrants, with the first and the last match reported
    /// let matches: Vec<_> = machine.current().phases()[&phase_id]
    ///     .matches()
    ///     .keys()
    ///     .copied()
    ///     .collect();
    /// for index in [0, 5] {
    ///     apply(
    ///         &mut machine,
    ///         ReportMatchResult::new(phase_id, matches[index], MatchResult::AWins),
    ///     );
    /// }
    ///
    /// machine.undo_match_result(phase_id, matches[0])?;
    ///
    /// let phase = &machine.current().phases()[&phase_id];
    /// assert_eq!(phase.matches()[&matches[0]].result(), MatchResult::NotFinished);
    /// assert_eq!(phase.matches()[&matches[5]].result(), MatchResult::AWins);
    /// # Ok::<(), tournament_engine::transitions::UndoMatchResultError>(())
    /// ```
    fn undo_match_result(
        &mut self,
        phase_id: PhaseId,
        match_id: MatchId,
    ) -> Result<TransitionId, UndoMatchResultError>;
}

impl CompetitionStateMachineExt for CompetitionStateMachine {
    fn undo_match_result(
        &mut self,
        phase_id: PhaseId,
        match_id: MatchId,
    ) -> Result<TransitionId, UndoMatchResultError> {
        let target = last_report_in_effect(self.log(), phase_id, match_id)
            .ok_or(UndoMatchResultError::NotReported)?;
        let id = TransitionId::new();
        self.apply(LogEntry::new(id, LogEntryKind::Undo(target)))?;
        Ok(id)
    }
}

/// The id of the latest log entry that is still in effect and reported a result for the match
/// `match_id` of the phase `phase_id`.
fn last_report_in_effect(
    log: &[LogEntry<Box<dyn CompetitionTransition>>],
    phase_id: PhaseId,
    match_id: MatchId,
) -> Option<TransitionId> {
    // an entry is in effect unless a later entry that is in effect itself undoes it, so a backward
    // scan has seen every undo that could cancel the entry it is looking at
    let mut undone = HashSet::new();
    for entry in log.iter().rev() {
        if undone.contains(&entry.id()) {
            continue;
        }
        match entry.kind() {
            LogEntryKind::Undo(target) => {
                undone.insert(*target);
            }
            LogEntryKind::Apply(transition) => {
                let transition: &dyn Any = &**transition;
                if let Some(report) = transition.downcast_ref::<ReportMatchResult>()
                    && (report.phase_id(), report.match_id()) == (phase_id, match_id)
                {
                    return Some(entry.id());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::{
        entities::competition::{Entrant, EntrantId, MatchResult},
        plugins::{
            phase_format::single_elimination::SingleElimination, ranker::HierarchicalRanker,
        },
        transitions::{
            add_entrant::AddEntrant, add_phase::AddPhase, set_competition_seed::SetCompetitionSeed,
            start_competition::StartCompetition,
        },
    };

    fn phase_id() -> PhaseId {
        PhaseId::new(Uuid::from_u128(0xfa5e))
    }

    #[track_caller]
    fn apply(
        machine: &mut CompetitionStateMachine,
        transition: impl CompetitionTransition + 'static,
    ) -> TransitionId {
        let id = TransitionId::new();
        machine
            .apply(LogEntry::new(id, LogEntryKind::Apply(Box::new(transition))))
            .expect("transition must be applicable");
        id
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
        apply(&mut machine, SetCompetitionSeed::new(*b"undo-test!!!"));
        for entrant in 0..4 {
            let entrant = Entrant::new(EntrantId::new(Uuid::from_u128(entrant)));
            apply(&mut machine, AddEntrant::new(entrant));
        }
        apply(
            &mut machine,
            AddPhase::new(
                phase_id(),
                Box::new(SingleElimination::new()),
                HierarchicalRanker::default(),
                Vec::new(),
            )
            .unwrap(),
        );
        apply(&mut machine, StartCompetition);
        machine
    }

    /// The matches of the phase, in the order they were scheduled in.
    fn matches(machine: &CompetitionStateMachine) -> Vec<MatchId> {
        machine.current().phases()[&phase_id()]
            .matches()
            .keys()
            .copied()
            .collect()
    }

    fn result_of(machine: &CompetitionStateMachine, match_id: MatchId) -> MatchResult {
        machine.current().phases()[&phase_id()].matches()[&match_id].result()
    }

    #[track_caller]
    fn report(machine: &mut CompetitionStateMachine, match_id: MatchId) -> TransitionId {
        apply(
            machine,
            ReportMatchResult::new(phase_id(), match_id, MatchResult::AWins),
        )
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
            .undo_match_result(phase_id(), first)
            .expect("the report of the first semifinal must be undoable");

        assert_eq!(result_of(&machine, first), MatchResult::NotFinished);
        assert_eq!(result_of(&machine, second), MatchResult::AWins);
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
            .undo_match_result(phase_id(), open)
            .expect_err("an open match must not be undoable");

        assert!(matches!(error, UndoMatchResultError::NotReported));
    }

    #[test]
    fn a_report_that_is_already_undone_is_not_undone_twice() {
        let mut machine = knockout();
        let first = matches(&machine)[0];
        report(&mut machine, first);
        machine.undo_match_result(phase_id(), first).unwrap();
        let log_len = machine.log().len();

        let error = machine
            .undo_match_result(phase_id(), first)
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
        assert_eq!(result_of(&machine, first), MatchResult::AWins);

        let id = machine.undo_match_result(phase_id(), first).unwrap();

        assert_eq!(result_of(&machine, first), MatchResult::NotFinished);
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
            .undo_match_result(phase_id(), semifinals[0])
            .expect_err("the final depends on the semifinal, so undoing it must be rejected");

        assert!(matches!(error, UndoMatchResultError::Rejected(_)));
        assert_eq!(result_of(&machine, semifinals[0]), MatchResult::AWins);
        assert_eq!(result_of(&machine, r#final), MatchResult::AWins);
        assert_eq!(machine.log().len(), log_len, "the log must stay unchanged");
    }
}
