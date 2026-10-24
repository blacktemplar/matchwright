//! What a GUI for running a tournament with [`tournament_engine`] does, without any UI.
//!
//! A [`Session`](session::Session) runs one competition for an operator: it turns what they do
//! (add an entrant, add the phase, start, report a result, undo) into transitions of the engine,
//! refuses what cannot be done with a typed [`error`], and hands out plain data for screens, the
//! [`view`]s. The names of the entrants are [`people`], kept apart from the competition's log. A
//! session is saved to and loaded from a [`Storage`](storage::Storage), whose methods are
//! asynchronous, and exported to and imported from a [`file`](mod@file). What the operator is told
//! is translated by [`i18n`].
//!
//! The crate builds for WebAssembly, where the GUI runs it.

pub mod error;
pub mod file;
pub mod i18n;
pub mod people;
pub mod session;
pub mod storage;
pub mod view;
