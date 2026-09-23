use crate::{
    entities::competition::{Match, Phase},
    plugins::Plugin,
};

pub trait PhaseFormat: Plugin {
    // the ranking ids of all produced matches must be part of this phase and match ids must be
    // unique
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;
}

impl Clone for Box<dyn PhaseFormat> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

pub mod single_elimination;
