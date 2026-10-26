//! Why a [`Session`](crate::session::Session) refused to do something. An action that fails leaves
//! the session unchanged.
//!
//! Each variant is something the operator is told: its `Display` in English, and as a
//! [`Message`] in the operator's language.

use std::fmt::Display;

use plugin_factory::{config::ConfigError, draft::Issue};
use thiserror::Error;
use tournament_engine::undoable_state_machine::{StateMachineError, TransitionError};

use crate::i18n::{Message, Text};

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
    /// implied plugin provides. What is wrong is what the [`FormView`](crate::form::FormView) of
    /// the draft shows at its fields, in the operator's language.
    #[error("the phase cannot be played this way; the form shows what to change")]
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
    #[error("the phase cannot be played this way; the form shows what to change")]
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

/// Why a result entered as text was not reported, see
/// [`Session::report_entered`](crate::session::Session::report_entered).
#[derive(Debug, Error)]
pub enum ReportEnteredError {
    #[error("enter the number of a match and its result, 1, X or 2, e.g. \"3 1\"")]
    Unreadable,
    #[error("there is no match {0}")]
    NoMatch(usize),
    #[error(transparent)]
    Report(#[from] ReportError),
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

/// Why a file was not imported: it is no competition's file, or holds a log that does not replay,
/// e.g. one of a plugin this version does not have.
#[derive(Debug, Error)]
#[error("the file is no competition this version can open: {0}")]
pub struct ImportError(#[source] pub serde_json::Error);

impl Message for AddEntrantError {
    fn text(&self) -> Text {
        match self {
            Self::EmptyName => Text::new("error-empty-name"),
            Self::DuplicateName(name) => {
                Text::new("error-duplicate-name").with("name", name.as_str())
            }
            Self::Started => Text::new("error-entrant-added-after-start"),
            Self::Rejected(error) => rejected(error),
        }
    }
}

impl Message for RemoveEntrantError {
    fn text(&self) -> Text {
        match self {
            Self::UnknownEntrant => Text::new("error-unknown-entrant"),
            Self::Started => Text::new("error-entrant-removed-after-start"),
            Self::Rejected(error) => rejected(error),
        }
    }
}

impl Message for PhaseDraftError {
    fn text(&self) -> Text {
        match self {
            Self::UnknownFormat(format) => {
                Text::new("error-unknown-format").with("format", format.as_str())
            }
            Self::Configuration(error) => {
                Text::new("error-format-misconfigured").with("reason", error.to_string())
            }
        }
    }
}

impl Message for AddPhaseError {
    fn text(&self) -> Text {
        match self {
            Self::Invalid(_) => Text::new("error-invalid-phase"),
            Self::PhaseAlreadyAdded => Text::new("error-phase-already-added"),
            Self::Started => Text::new("error-phase-added-after-start"),
            Self::Rejected(error) => rejected(error),
        }
    }
}

impl Message for NoPhase {
    fn text(&self) -> Text {
        Text::new("error-no-phase")
    }
}

impl Message for EditPhaseError {
    fn text(&self) -> Text {
        match self {
            Self::NoPhase(no_phase) => no_phase.text(),
            Self::Invalid(_) => Text::new("error-invalid-phase"),
            Self::DoesNotFit(error) => {
                Text::new("error-phase-change-does-not-fit").with("reason", error.to_string())
            }
        }
    }
}

impl Message for StartError {
    fn text(&self) -> Text {
        match self {
            Self::Started => Text::new("error-started-already"),
            Self::NoPhase(no_phase) => no_phase.text(),
            Self::TooFewEntrants => Text::new("error-too-few-entrants"),
            Self::Rejected(error) => rejected(error),
        }
    }
}

impl Message for ReportError {
    fn text(&self) -> Text {
        match self {
            Self::NotStarted => Text::new("error-not-started"),
            Self::UnknownMatch => Text::new("error-unknown-match"),
            Self::AlreadyFinished => Text::new("error-match-finished-already"),
            Self::DrawNotAllowed => Text::new("error-draw-not-allowed"),
            Self::Rejected(error) => rejected(error),
        }
    }
}

impl Message for ReportEnteredError {
    fn text(&self) -> Text {
        match self {
            Self::Unreadable => Text::new("error-result-unreadable"),
            Self::NoMatch(number) => Text::new("error-no-match-numbered").with("number", *number),
            Self::Report(error) => error.text(),
        }
    }
}

impl Message for UndoReportError {
    fn text(&self) -> Text {
        match self {
            Self::NotStarted => Text::new("error-not-started"),
            Self::UnknownMatch => Text::new("error-unknown-match"),
            Self::NotReported => Text::new("error-match-not-reported"),
            Self::DependedOn => Text::new("error-result-depended-on"),
        }
    }
}

impl Message for NothingToUndo {
    fn text(&self) -> Text {
        Text::new("error-nothing-to-undo")
    }
}

impl Message for NothingToRedo {
    fn text(&self) -> Text {
        Text::new("error-nothing-to-redo")
    }
}

impl<E: Display> Message for LoadError<E> {
    fn text(&self) -> Text {
        match self {
            Self::NotFound => Text::new("error-competition-not-found"),
            Self::Storage(error) => {
                Text::new("error-competition-unreadable").with("reason", error.to_string())
            }
            Self::Entry { position, source } => Text::new("error-log-entry-unreadable")
                .with("position", *position)
                .with("reason", source.to_string()),
            Self::Person(error) => {
                Text::new("error-person-unreadable").with("reason", error.to_string())
            }
            Self::Replay(error) => {
                Text::new("error-log-does-not-replay").with("reason", error.to_string())
            }
        }
    }
}

impl Message for ImportError {
    fn text(&self) -> Text {
        Text::new("error-file-unreadable").with("reason", self.0.to_string())
    }
}

// The engine's own errors are passed on in English, since what they say is open-ended.

/// The engine refused an action for a reason this crate does not know.
fn rejected(error: &TransitionError) -> Text {
    Text::new("error-rejected").with("reason", error.to_string())
}
