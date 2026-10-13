use dyn_clone::DynClone;
use plugin_factory::{Plugin, axis};

use crate::entities::competition::{Bye, Match, PhaseData};

/// How the matches of a phase are scheduled.
///
/// A format may keep state in `self` and update it in [`Self::create_new_matches`], in a field
/// left out of its schema with `#[plugin(skip)]`. That state is serialized together with the
/// phase, and it must only depend on the [`PhaseData`] and the calls the format has seen, so that
/// replaying the transition log recomputes it.
#[axis]
pub trait PhaseFormat: Plugin + DynClone {
    /// The matches and byes to schedule next, given everything the phase has played so far. Empty
    /// if there is nothing to schedule, which is how a finished phase is reported.
    ///
    /// Called when the phase starts and again after every reported match result. Scheduling a bye
    /// does not lead to another call.
    ///
    /// An implementation must only schedule rankings of `phase_data`, must not return a match or a
    /// bye whose id already exists in the phase, and must not put the same ranking into two matches
    /// or byes of the same round. Rounds have to order the matches of each ranking by when they are
    /// played, since metrics such as [`RoundOfFirstLoss`] read a later round as a later match.
    ///
    /// Anything a consumer needs to know about a match or a bye beyond its round, e.g. the stage of
    /// a bracket it belongs to, is attached with [`Match::with_annotation`] or
    /// [`Bye::with_annotation`]. Every annotation a format writes has to be provided as a
    /// [`MatchAnnotation`](crate::plugins::facts::MatchAnnotation) in its declaration, so that the
    /// plugins reading it can require it.
    ///
    /// [`RoundOfFirstLoss`]: super::metrics_calculator::round_of_first_loss::RoundOfFirstLoss
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule;

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

/// What a [`PhaseFormat`] schedules at once, see [`PhaseFormat::create_new_matches`].
#[derive(Debug, Default)]
pub struct Schedule {
    pub matches: Vec<Match>,
    pub byes: Vec<Bye>,
}

impl From<Vec<Match>> for Schedule {
    /// Schedules `matches` and no byes.
    fn from(matches: Vec<Match>) -> Self {
        Self {
            matches,
            byes: Vec::new(),
        }
    }
}

mod bracket;
pub mod double_elimination;
pub mod monster_dyp;
pub mod random_rounds;
pub mod round_robin;
pub mod single_elimination;
