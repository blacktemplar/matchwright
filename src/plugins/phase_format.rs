use std::fmt::Debug;

use dyn_clone::DynClone;

use crate::entities::competition::{Match, Phase};

pub trait PhaseFormat: Debug + DynClone {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;
}

impl Clone for Box<dyn PhaseFormat> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

pub mod single_elimination;
