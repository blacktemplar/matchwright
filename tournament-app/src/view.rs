//! What a [`Session`](crate::session::Session) hands a screen to show: plain data, laid out by the
//! GUI.

use std::num::NonZeroU32;

use tournament_engine::{EntrantId, MatchId, MatchResult};

/// An entrant as a screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntrantRow {
    pub id: EntrantId,
    /// The name the entrant is shown by, `None` for an entrant of a log written elsewhere that came
    /// without one.
    pub name: Option<String>,
}

/// A format a phase can be played in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Format {
    /// The id [`Session::phase_draft`](crate::session::Session::phase_draft) takes.
    pub id: &'static str,
    pub label: &'static str,
    pub description: Option<&'static str>,
}

/// A round of the phase as a list of rounds shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoundSummary {
    pub round: NonZeroU32,
    /// The matches scheduled in the round.
    pub matches: usize,
    /// Those of [`Self::matches`] without a result.
    pub open: usize,
}

/// The matches and byes of a round.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Round {
    pub round: NonZeroU32,
    /// In the order they were scheduled.
    pub matches: Vec<MatchRow>,
    pub byes: Vec<ByeRow>,
}

/// A match as a screen lists it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchRow {
    pub id: MatchId,
    /// The position of the match among all matches of the phase, from 1, which never changes.
    pub number: usize,
    pub round: NonZeroU32,
    /// The stage the format names the match by, e.g. "Final".
    pub stage: Option<String>,
    /// The entrants of the side a [`MatchResult::AWins`] is a win of.
    pub a: Vec<EntrantRow>,
    pub b: Vec<EntrantRow>,
    pub result: Option<MatchResult>,
}

/// A team that sits out a round.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ByeRow {
    pub entrants: Vec<EntrantRow>,
    /// The stage the format names the bye by, only if the matches and byes of its round belong to
    /// more than one stage, e.g. a bye of the loser bracket in a round of the winner bracket.
    pub stage: Option<String>,
}

/// The standings of the phase.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Standings {
    /// The metric columns, one per calculator of the phase that offers one, in running order.
    pub columns: Vec<Column>,
    /// Best first.
    pub rows: Vec<StandingRow>,
}

/// A metric column of the standings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Column {
    /// The id of the calculator whose metric the column shows, which its translated header is
    /// found by, see [`PluginText::Column`](crate::i18n::PluginText::Column).
    pub calculator: &'static str,
    /// The header in English.
    pub header: &'static str,
}

/// One line of the standings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StandingRow {
    /// Shared by tied teams.
    pub rank: NonZeroU32,
    pub entrants: Vec<EntrantRow>,
    /// One cell per column of [`Standings::columns`], `None` where the metric is not calculated.
    pub cells: Vec<Option<String>>,
}

/// What the operator can do now. Each is true exactly when doing it would not be refused for the
/// state the competition is in; it can still fail for what is done, e.g. a duplicate name.
///
/// The default is that nothing can be done, as for a competition shown read-only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Actions {
    pub add_entrant: bool,
    pub remove_entrant: bool,
    pub add_phase: bool,
    pub edit_phase: bool,
    pub start: bool,
    /// Some match has no result yet.
    pub report: bool,
    pub undo: bool,
    pub redo: bool,
}

/// What reporting a result led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reported {
    /// The matches the result made the format schedule.
    pub new_matches: usize,
    /// Every match of the phase has a result now.
    pub all_finished: bool,
}

/// What undoing a result led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reopened {
    /// The matches that were only scheduled because of the result, and are gone with it.
    pub unscheduled: usize,
}
