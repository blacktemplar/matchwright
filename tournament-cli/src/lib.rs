//! An interactive command line interface for running a tournament with [`tournament_engine`].
//!
//! Everything but reading lines from the terminal lives here: a line typed at the prompt is parsed
//! into a [`Command`](command::Command), which a [`Session`](session::Session) executes against the
//! competition it runs. The one part that asks the operator back is [`wizard`], which configures a
//! [`PhaseConfig`](phase::PhaseConfig) the session then adds as a phase.
//!
//! Everything a tournament needs in order to be *shown* to an operator is defined in
//! [`annotations`] and reaches the engine as an annotation on an entity.

pub mod annotations;
pub mod command;
pub mod phase;
pub mod session;
pub mod wizard;
