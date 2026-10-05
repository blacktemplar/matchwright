//! The parts of the command line interface that are worth testing on their own, starting with the
//! data this consumer attaches to the entities of [`tournament_engine`].
//!
//! The engine deliberately knows nothing about display, so everything a tournament needs in order
//! to be *shown* to an operator is defined here and reaches the engine as an annotation on an
//! entity, written by the transition that creates it (see the "Consumer data on entities" section
//! of `DESIGN.md`).

pub mod annotations;
