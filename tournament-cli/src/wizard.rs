//! Configuring a phase, by asking the [`questionnaire`] of [`PhasePlugins`].
//!
//! Every question starts out at the answer the plugins chosen before it recommend, so accepting
//! all of them with Enter configures the phase a preset of the chosen format adds. Cancelling a
//! question with Esc cancels the whole configuration.
//!
//! The configured phase is handed to [`Session::add_phase`](crate::session::Session::add_phase),
//! which is what changes the competition.

use plugin_factory::draft::Draft;
use tournament_engine::{entities::competition::Competition, plugins::PhasePlugins};

use crate::questionnaire::{self, Error, Prompter};

/// Asks for every part of a phase of `competition` through `prompter` and answers it in `draft`.
///
/// A question starts out at the answer `draft` holds already, so a draft the session could not add
/// is asked again with the answers given before.
///
/// # Errors
/// Returns what [`questionnaire::fill`] returns, e.g. [`Error::Prompt`] with
/// [`InquireError::OperationCanceled`] if the operator cancels a question with Esc,
/// [`InquireError::OperationInterrupted`] on Ctrl-C and [`InquireError::NotTTY`] if there is no
/// terminal to ask at, e.g. because the input is piped in.
///
/// [`InquireError::OperationCanceled`]: inquire::InquireError::OperationCanceled
/// [`InquireError::OperationInterrupted`]: inquire::InquireError::OperationInterrupted
/// [`InquireError::NotTTY`]: inquire::InquireError::NotTTY
pub fn configure_phase(
    competition: &Competition,
    draft: &mut Draft<PhasePlugins>,
    prompter: &mut dyn Prompter,
) -> Result<(), Error> {
    questionnaire::fill(
        draft,
        &|declaration| competition.declare_phase_context(declaration),
        prompter,
    )
}
