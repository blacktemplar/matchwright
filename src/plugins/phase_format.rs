use crate::{
    entities::competition::{Match, Phase},
    plugins::Plugin,
};

pub trait PhaseFormat: Plugin {
    // The ranking ids of all produced matches must be part of this phase and match ids must be
    // unique. Furthermore, it is not allowed to have two matchesf
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;
}

impl Clone for Box<dyn PhaseFormat> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

pub mod round_robin;
pub mod single_elimination;
