use std::fmt::Debug;

use dyn_clone::DynClone;

use crate::entities::competition::{Match, Phase};

pub trait PhaseFormat: Debug + DynClone {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;
}

pub mod single_elimination;
