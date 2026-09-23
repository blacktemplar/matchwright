use std::fmt::Debug;

use dyn_clone::DynClone;

pub mod metrics_calculator;
pub mod phase_format;
pub mod ranker;

pub trait Plugin: Debug + DynClone {}

impl<T: Debug + DynClone> Plugin for T {}
