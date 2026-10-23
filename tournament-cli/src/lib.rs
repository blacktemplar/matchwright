//! An interactive command line interface for running a tournament with [`tournament_engine`].
//!
//! Everything but reading lines from the terminal lives here: a line typed at the prompt is parsed
//! into a [`Command`](command::Command), which a [`Session`](session::Session) executes against the
//! competition it runs. The one part that asks the operator back is the [`questionnaire`], which
//! configures the plugins of a phase through their schemas, and which the session then adds as a
//! phase.
//!
//! The names the entrants are shown by are [`people`] (those of [`tournament_app`]), kept apart
//! from the competition. A session saves both to a [`Storage`](storage::Storage), e.g. to files.
//!
//! The library also builds for WebAssembly, without the default feature `terminal`, which adds
//! what only works at a terminal: the binary and asking at the terminal
//! (`questionnaire::Terminal`).

pub mod command;
pub mod questionnaire;
pub mod session;
pub mod storage;

pub use tournament_app::people;
