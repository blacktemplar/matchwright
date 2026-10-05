//! The transitions that drive a [`Competition`] forward, and the trait that makes them
//! persistable.

use std::fmt::Debug;

use undoable_state_machine::{StateMachine, Transition};

use crate::entities::competition::Competition;

pub mod add_entrant;
pub mod add_phase;
pub mod remove_entrant;
pub mod report_match_result;
pub mod set_competition_seed;
pub mod start_competition;

/// A [`Transition`] of a [`Competition`] that can be written to and read back from a transition
/// log.
///
/// [`Transition`] is generic over its state, and `typetag` rejects generic traits in `de` mode
/// ("deserialization of generic traits is not supported yet"), so it cannot be tagged upstream.
/// This sub-trait pins the state to [`Competition`], which makes it non-generic and therefore
/// taggable, and that is what gives `Box<dyn CompetitionTransition>` a [`Serialize`] and a
/// [`Deserialize`] impl.
///
/// The trait adds no methods of its own: implementing it is purely the statement that this
/// transition is part of the persisted log. Every implementor therefore needs
/// [`Serialize`]/[`Deserialize`] and a short, explicitly chosen, namespaced tag — a tag is a
/// persistence contract, so renaming or moving a transition must not change it.
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
#[typetag::serde(tag = "type", content = "value")]
pub trait CompetitionTransition: Transition<Competition> + Debug {}

/// The state machine of a competition, driven by transitions that can be persisted.
///
/// `undoable-state-machine` anticipates a boxed transition as its `T` through its blanket
/// `impl<S, T: ?Sized + Transition<S>> Transition<S> for Box<T>`, so the box needs nothing from
/// this crate to be applied.
///
/// Because [`CompetitionTransition`] makes that box `Serialize` and `Deserialize`, the whole
/// machine is too: it serializes as its append-only log — undo entries included — and
/// deserializing replays that log, which recomputes [`StateMachine::current`] rather than trusting
/// a stored copy of it. A log that does not replay cleanly therefore fails to deserialize.
///
/// This is the log persistence path, and it is the one that keeps consumer annotations and undo.
/// The other path is a snapshot of [`StateMachine::current`] alone, which is smaller and cheaper
/// to load but drops the history.
pub type CompetitionStateMachine = StateMachine<Competition, Box<dyn CompetitionTransition>>;
