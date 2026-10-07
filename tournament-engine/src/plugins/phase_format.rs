use crate::{
    entities::competition::{Match, Phase},
    plugins::Plugin,
};

#[typetag::serde(tag = "type", content = "value")]
pub trait PhaseFormat: Plugin {
    /// The matches to schedule next, given everything `phase` has played so far. Empty if there is
    /// nothing to schedule, which is how a finished phase is reported.
    ///
    /// Called when the phase starts and again after every reported match result.
    ///
    /// An implementation must only pair rankings of `phase`, must not return a match whose id
    /// already exists in the phase, and must not put the same ranking into two matches of the same
    /// round.
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match>;

    /// Whether a match of this format may end in a [`MatchResult::Draw`].
    ///
    /// A draw reported for a format that returns `false` is rejected, so
    /// [`PhaseFormat::create_new_matches`] never sees one.
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
