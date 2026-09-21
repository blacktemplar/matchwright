use dyn_clone::DynClone;

use crate::entities::competition::{Match, Phase};
use std::fmt::Debug;

pub trait PhaseFormat: Debug + DynClone {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;
}

pub mod single_elimination;
