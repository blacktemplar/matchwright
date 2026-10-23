//! Why a [`Session`](crate::session::Session) refused to do something. An action that fails leaves
//! the session unchanged.
//!
//! Each variant is something the operator is told; the messages are English, and a GUI may show
//! its own text for a variant instead.

use std::fmt::{self, Display};

use plugin_factory::{config::ConfigError, draft::Issue};
use thiserror::Error;
use tournament_engine::undoable_state_machine::{StateMachineError, TransitionError};

/// Why an entrant was not added.
#[derive(Debug, Error)]
pub enum AddEntrantError {
    #[error("an entrant name must not be empty")]
    EmptyName,
    #[error("there already is an entrant named {0:?}")]
    DuplicateName(String),
    #[error("the competition has started, so no entrant can be added anymore")]
    Started,
    /// The engine refused the entrant for a reason this crate does not know.
    #[error("{0}")]
    Rejected(TransitionError),
}

/// Why an entrant was not removed.
#[derive(Debug, Error)]
pub enum RemoveEntrantError {
    #[error("there is no such entrant")]
    UnknownEntrant,
    #[error("the competition has started, so no entrant can be removed anymore")]
    Started,
    /// The engine refused the removal for a reason this crate does not know.
    #[error("{0}")]
    Rejected(TransitionError),
}

/// Why there is no draft of a phase played in a format.
#[derive(Debug, Error)]
pub enum PhaseDraftError {
    /// No registered phase format has this id.
    #[error("there is no format {0:?}")]
    UnknownFormat(String),
    /// The format offers an answer its own schema does not take.
    #[error(transparent)]
    Configuration(#[from] ConfigError),
}

/// Why a phase was not added.
#[derive(Debug, Error)]
pub enum AddPhaseError {
    /// No phase can be built from the draft: its plugins do not fit together, or miss something no
    /// implied plugin provides.
    #[error("the phase cannot be played this way: {}", Issues(.0))]
    Invalid(Vec<Issue>),
    #[error("the competition already has a phase, and only one is supported")]
    PhaseAlreadyAdded,
    #[error("the competition has started, so no phase can be added anymore")]
    Started,
    /// The engine refused the phase for a reason this crate does not know.
    #[error("{0}")]
    Rejected(TransitionError),
}

/// The competition has no phase yet.
#[derive(Debug, Error)]
#[error("the competition has no phase yet")]
pub struct NoPhase;

/// Why the phase was not changed.
#[derive(Debug, Error)]
pub enum EditPhaseError {
    #[error(transparent)]
    NoPhase(#[from] NoPhase),
    /// No phase can be built from the draft, see [`AddPhaseError::Invalid`].
    #[error("the phase cannot be played this way: {}", Issues(.0))]
    Invalid(Vec<Issue>),
    /// The edited phase does not fit a change made after the phase was added, e.g. a result
    /// reported for a match the edited format does not schedule.
    #[error("the phase cannot be changed this way, since a later change no longer fits it: {0}")]
    DoesNotFit(TransitionError),
}

/// Why the competition did not start.
#[derive(Debug, Error)]
pub enum StartError {
    #[error("the competition has started already")]
    Started,
    #[error(transparent)]
    NoPhase(#[from] NoPhase),
    #[error("a competition needs at least two entrants")]
    TooFewEntrants,
    /// The engine refused the start for a reason this crate does not know.
    #[error("{0}")]
    Rejected(TransitionError),
}

/// Why a result was not reported.
#[derive(Debug, Error)]
pub enum ReportError {
    #[error("the competition has not started yet")]
    NotStarted,
    #[error("there is no such match")]
    UnknownMatch,
    #[error("the match has a result already")]
    AlreadyFinished,
    #[error("a match of this format cannot end in a draw")]
    DrawNotAllowed,
    /// The engine refused the result for a reason this crate does not know.
    #[error("{0}")]
    Rejected(TransitionError),
}

/// Why a result was not undone.
#[derive(Debug, Error)]
pub enum UndoReportError {
    #[error("the competition has not started yet")]
    NotStarted,
    #[error("there is no such match")]
    UnknownMatch,
    #[error("the match has no result that could be undone")]
    NotReported,
    /// A later result depends on this one, e.g. that of the final the undone semifinal decided the
    /// participants of; it has to be undone first.
    #[error("a later result depends on this one; undo that one first")]
    DependedOn,
}

/// The session has done nothing it has not undone.
#[derive(Debug, Error)]
#[error("there is nothing to undo")]
pub struct NothingToUndo;

/// The session has undone nothing since its latest action.
#[derive(Debug, Error)]
#[error("there is nothing to redo")]
pub struct NothingToRedo;

/// Why a stored competition was not loaded. `E` is the error of the storage.
#[derive(Debug, Error)]
pub enum LoadError<E> {
    #[error("there is no such competition")]
    NotFound,
    #[error("the competition cannot be read: {0}")]
    Storage(#[source] E),
    /// An entry of the log is not one this version knows, e.g. one of a plugin it does not have.
    #[error("entry {position} of the log cannot be read: {source}")]
    Entry {
        position: usize,
        source: serde_json::Error,
    },
    #[error("an entrant's name cannot be read: {0}")]
    Person(#[source] serde_json::Error),
    /// The log does not replay into a competition.
    #[error("the log does not replay: {0}")]
    Replay(#[source] StateMachineError),
}

/// Lists issues of a draft in one line.
struct Issues<'a>(&'a [Issue]);

impl Display for Issues<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, issue) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            issue.fmt(f)?;
        }
        Ok(())
    }
}
