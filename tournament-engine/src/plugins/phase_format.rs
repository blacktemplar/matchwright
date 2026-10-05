use crate::{
    entities::competition::{Match, Phase},
    plugins::Plugin,
};

#[typetag::serde(tag = "type", content = "value")]
pub trait PhaseFormat: Plugin {
    // The ranking ids of all produced matches must be part of this phase and match ids must be
    // unique. Furthermore, it is not allowed to have two matchesf
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;

    /// Whether a match of this format may end in a [`MatchResult::Draw`].
    ///
    /// A draw reported for a match of a format that returns `false` here is rejected before it
    /// reaches the phase, so [`PhaseFormat::create_new_matches`] never sees one.
    ///
    /// [`MatchResult::Draw`]: crate::entities::competition::MatchResult::Draw
    fn allows_draws(&self) -> bool {
        true
    }
}

impl Clone for Box<dyn PhaseFormat> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

pub mod random_rounds;
pub mod round_robin;
pub mod single_elimination;
