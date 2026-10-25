use std::fmt::{self, Display, Formatter};

use dyn_clone::DynClone;
use plugin_factory::{Plugin, axis};

use crate::{
    entities::competition::{Bye, Match, PhaseData},
    type_map::TypeMap,
};

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
    /// a bracket it belongs to, is attached to it with [`Annotated::with_annotation`]. Every
    /// annotation a format writes has to be provided as a
    /// [`MatchAnnotation`](crate::plugins::facts::MatchAnnotation) in its declaration, so that the
    /// plugins reading it can require it.
    ///
    /// [`RoundOfFirstLoss`]: super::metrics_calculator::round_of_first_loss::RoundOfFirstLoss
    /// [`Annotated::with_annotation`]: crate::entities::competition::Annotated::with_annotation
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

    /// The stage of the phase a match or a bye with `annotations` belongs to, e.g. the semifinals,
    /// or `None` if this format names no stages. Read from the annotations the format wrote on the
    /// match or the bye, see [`Annotated::annotations`].
    ///
    /// [`Annotated::annotations`]: crate::entities::competition::Annotated::annotations
    fn stage(&self, annotations: &TypeMap) -> Option<Stage> {
        let _ = annotations;
        None
    }
}

dyn_clone::clone_trait_object!(PhaseFormat);

/// The name of a stage of a phase, as a [`PhaseFormat`] names a match or a bye by, see
/// [`PhaseFormat::stage`]. Shows as its name in English.
///
/// A translation finds it by the format's id, its [key](Self::key), and the numbers it has.
///
/// # Examples
/// ```
/// # use tournament_engine::plugins::phase_format::Stage;
/// let stage = Stage::new("winners-round", "Winners round 2").with("round", 2);
/// assert_eq!(stage.to_string(), "Winners round 2");
/// assert_eq!(stage.numbers(), [("round", 2)]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stage {
    key: &'static str,
    numbers: Vec<(&'static str, u32)>,
    english: String,
}

impl Stage {
    /// The stage `key`, which names it among the stages of its format whatever its numbers, e.g.
    /// `"final"`, and is named `english` in English.
    pub fn new(key: &'static str, english: impl Into<String>) -> Self {
        Self {
            key,
            numbers: Vec::new(),
            english: english.into(),
        }
    }

    /// This stage with the number `name`, which its name says, set to `value`.
    #[must_use]
    pub fn with(mut self, name: &'static str, value: u32) -> Self {
        self.numbers.push((name, value));
        self
    }

    /// What names it among the stages of its format, whatever its numbers.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The numbers its name says, each by its name, in the order they were set.
    pub fn numbers(&self) -> &[(&'static str, u32)] {
        &self.numbers
    }
}

impl Display for Stage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.english)
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
pub mod round_robin;
pub mod single_elimination;

pub use double_elimination::{DoubleElimination, DoubleEliminationRound};
pub use monster_dyp::MonsterDyp;
pub use round_robin::{RoundRobin, random_rounds::RandomRounds};
pub use single_elimination::{KnockoutRound, SingleElimination};
