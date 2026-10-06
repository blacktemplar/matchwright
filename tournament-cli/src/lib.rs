//! An interactive command line interface for running a tournament with [`tournament_engine`].
//!
//! Everything but reading lines from the terminal lives here, so that it can be tested on its
//! own: a line typed at the prompt is parsed into a [`Command`](command::Command), which a
//! [`Session`](session::Session) executes against the competition it runs. The one thing that
//! does ask the operator back is [`wizard`], which configures a [`PhaseConfig`](phase::PhaseConfig)
//! the session then adds as a phase.
//!
//! The engine deliberately knows nothing about display, so everything a tournament needs in order
//! to be *shown* to an operator is defined here and reaches the engine as an annotation on an
//! entity, written by the transition that creates it (see the "Consumer data on entities" section
//! of `DESIGN.md`).

pub mod annotations;
pub mod command;
pub mod phase;
pub mod session;
pub mod wizard;
