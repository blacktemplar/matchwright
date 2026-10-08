use std::fmt::Debug;

use dyn_clone::DynClone;

pub mod compatibility;
pub mod metrics_calculator;
pub mod phase_format;
pub mod ranker;

/// The supertrait of every plugin trait.
///
/// A plugin may keep state in `self` and update it in its `&mut self` methods. That state is
/// serialized together with the phase, and it must only depend on the [`PhaseData`] and the calls
/// the plugin has seen, so that replaying the transition log recomputes it.
///
/// [`PhaseData`]: crate::entities::competition::PhaseData
pub trait Plugin: Debug + DynClone {}

impl<T: Debug + DynClone> Plugin for T {}
