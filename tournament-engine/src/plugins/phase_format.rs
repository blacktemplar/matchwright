use crate::{
    entities::competition::{Match, PhaseData},
    plugins::Plugin,
};

#[typetag::serde(tag = "type", content = "value")]
pub trait PhaseFormat: Plugin {
    /// The matches to schedule next, given everything the phase has played so far. Empty if there
    /// is nothing to schedule, which is how a finished phase is reported.
    ///
    /// Called when the phase starts and again after every reported match result.
    ///
    /// An implementation must only pair rankings of `phase_data`, must not return a match whose id
    /// already exists in the phase, and must not put the same ranking into two matches of the same
    /// round. Rounds have to order the matches of each ranking by when they are played, since
    /// metrics such as [`RoundOfFirstLoss`] read a later round as a later match.
    ///
    /// Anything a consumer needs to know about a match beyond its round, e.g. the stage of a
    /// bracket it belongs to, is attached with [`Match::with_annotation`].
    ///
    /// [`RoundOfFirstLoss`]: super::metrics_calculator::round_of_first_loss::RoundOfFirstLoss
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Vec<Match>;

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

mod bracket;
pub mod double_elimination;
pub mod random_rounds;
pub mod round_robin;
pub mod single_elimination;
