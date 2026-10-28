//! What a [`Session`](crate::session::Session) hands a screen to show: plain data, laid out by the
//! GUI.

use std::num::NonZeroU32;

use tournament_engine::{
    EntrantId, MatchId, MatchResult,
    plugins::{metrics_calculator::Cell, phase_format::Stage},
};
use unicode_normalization::{UnicodeNormalization as _, char::is_combining_mark};

use crate::error::ReportEnteredError;

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
    /// Whether a match of the round may end in a [`MatchResult::Draw`].
    pub draws: bool,
}

/// A match as a screen lists it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchRow {
    pub id: MatchId,
    /// The position of the match among all matches of the phase, from 1, which never changes.
    pub number: usize,
    pub round: NonZeroU32,
    /// The stage the format names the match by, e.g. the final.
    pub stage: Option<StageName>,
    /// The entrants of the side a [`MatchResult::AWins`] is a win of.
    pub a: Vec<EntrantRow>,
    pub b: Vec<EntrantRow>,
    pub result: Option<MatchResult>,
}

/// What an operator typed to find entrants by their name: it finds those whose name contains it,
/// whatever the case and the accents of either. Nothing typed, or only spaces, finds every
/// entrant, and so does every match.
///
/// ```
/// use tournament_app::view::NameSearch;
///
/// assert!(NameSearch::new(" ali").finds_name("Alice"));
/// assert!(NameSearch::new("muller").finds_name("Müller"));
/// assert!(NameSearch::new("ZOË").finds_name("Zoe"));
/// assert!(!NameSearch::new("bob").finds_name("Alice"));
/// assert!(NameSearch::new("").finds_name("Alice"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct NameSearch {
    /// Trimmed and [folded](fold).
    text: String,
}

impl NameSearch {
    pub fn new(text: &str) -> Self {
        Self {
            text: fold(text.trim()),
        }
    }

    /// Whether nothing was typed but spaces, which finds everything.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn finds_name(&self, name: &str) -> bool {
        fold(name).contains(&self.text)
    }

    /// Whether it finds `entrant`; one without a name only when nothing was typed.
    pub fn finds(&self, entrant: &EntrantRow) -> bool {
        match &entrant.name {
            Some(name) => self.finds_name(name),
            None => self.is_empty(),
        }
    }

    /// Whether it finds an entrant of either side of the match `row`.
    pub fn finds_in(&self, row: &MatchRow) -> bool {
        row.a
            .iter()
            .chain(&row.b)
            .any(|entrant| self.finds(entrant))
    }
}

/// `text` in lower case without accents: each letter split from its accents (Unicode's canonical
/// decomposition), which are dropped, e.g. `Müller` as `muller`.
fn fold(text: &str) -> String {
    text.nfd()
        .filter(|&char| !is_combining_mark(char))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A team that sits out a round.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ByeRow {
    pub entrants: Vec<EntrantRow>,
    /// The stage the format names the bye by, only if the matches and byes of its round belong to
    /// more than one stage, e.g. a bye of the loser bracket in a round of the winner bracket.
    pub stage: Option<StageName>,
}

/// The stage of the phase a match or a bye belongs to, as its format names it, which is said in
/// the operator's language by [`Translator::stage`](crate::i18n::Translator::stage).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StageName {
    /// The id of the format, whose message translates the stage.
    pub format: &'static str,
    /// Shows as its name in English.
    pub stage: Stage,
}

/// The bracket of a knockout phase, every pairing in it from the start, in sections of columns.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bracket {
    /// In the order they are played.
    pub sections: Vec<BracketSection>,
}

/// A part of a [`Bracket`], whose columns are drawn left to right, joined by lines.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BracketSection {
    pub part: BracketPart,
    /// In the order they are played.
    pub columns: Vec<BracketColumn>,
}

/// Which part of its bracket a [`BracketSection`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BracketPart {
    /// The knockout of a single elimination, with its match for third place below the final.
    Knockout,
    /// The winner bracket of a double elimination.
    Winners,
    /// The loser bracket of a double elimination.
    Losers,
    /// The grand final of a double elimination, and its reset.
    GrandFinal,
}

/// The pairings of a stage of a [`BracketSection`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BracketColumn {
    pub stage: Option<StageName>,
    /// Top to bottom, so that the winners of two neighbours that meet next are drawn either side
    /// of the pairing they meet in.
    pub pairings: Vec<BracketPairing>,
    /// A stage drawn below the pairings, in the bottom quarter of the column, apart from the lines:
    /// the match for third place below the final.
    pub below: Option<Box<BracketColumn>>,
}

/// A pairing of a [`Bracket`]: a match, once both of its sides are known.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BracketPairing {
    /// The number its match has once scheduled (see [`MatchRow::number`]), from the start, which
    /// the sides waiting for it name it by, see [`BracketSource::number`]; `None` if it is never a
    /// match.
    pub number: Option<usize>,
    /// Its match, once scheduled.
    pub r#match: Option<MatchRow>,
    /// Sides A and B.
    pub sides: [BracketEntry; 2],
    /// The line from it to the pairing its winner plays next, if that is in the next column.
    pub line_out: Option<BracketLine>,
    /// Whether a line comes in from the previous column.
    pub line_in: bool,
    /// Whether it is only played if the pairing before it ends a certain way, e.g. a grand final
    /// reset, which is not decided yet.
    pub only_if_needed: bool,
    /// Why it is never played, if it is not: then it has no lines, and its sides say nothing.
    pub unplayed: Option<Unplayed>,
}

/// Why a [`BracketPairing`] is never played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unplayed {
    /// Both of its sides are byes, so nobody moves on from it.
    Byes,
    /// It was only played if needed, see [`BracketPairing::only_if_needed`], and was not.
    NotNeeded,
}

/// Who takes a side of a [`BracketPairing`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BracketEntry {
    Team(Vec<EntrantRow>),
    /// Nobody, so the other side moves on without a match.
    Bye,
    /// The winner of a pairing not decided yet.
    WinnerOf(BracketSource),
    /// The loser of a pairing not decided yet.
    LoserOf(BracketSource),
}

/// The pairing a side of a [`BracketPairing`] is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BracketSource {
    /// Its number, see [`BracketPairing::number`].
    pub number: Option<usize>,
    pub stage: Option<StageName>,
    /// Its place in the column of its stage, from 1 at the top, if the column has several
    /// pairings.
    pub position: Option<usize>,
}

/// Where the line from a [`BracketPairing`] to the pairing its winner plays next goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BracketLine {
    /// Straight on, the next pairing taking the winner of this one alone from this column.
    Straight,
    /// Down, to meet the winner of the pairing below.
    Down,
    /// Up, to meet the winner of the pairing above.
    Up,
}

/// The matches of every two teams of a phase, as a square table with a row and a column per team.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrossTable {
    /// In the order the teams' entrants were entered, which results do not change; the columns are
    /// in the same order.
    pub rows: Vec<CrossRow>,
    /// Whether a match may end in a [`MatchResult::Draw`].
    pub draws: bool,
}

/// A team's row of a [`CrossTable`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrossRow {
    pub entrants: Vec<EntrantRow>,
    /// Its rank in the standings, shared by tied teams.
    pub rank: NonZeroU32,
    /// One per column, i.e. per row of the table, in the same order: the matches of this team
    /// against that row's team, by their numbers; none against itself.
    pub cells: Vec<Vec<CrossMatch>>,
}

/// A match in a cell of a [`CrossTable`], seen from the team of the cell's row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrossMatch {
    pub row: MatchRow,
    /// How it ended for the team of the cell's row, once it has a result.
    pub outcome: Option<Outcome>,
}

/// How a match ended for one of its teams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Win,
    Draw,
    Loss,
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
    /// The calculator's label in English, the metric's whole name where the header abbreviates
    /// it, translated as its [`PluginText::Label`](crate::i18n::PluginText::Label).
    pub label: &'static str,
    /// What the calculator says about its metric in English, translated as its
    /// [`PluginText::Description`](crate::i18n::PluginText::Description).
    pub description: Option<&'static str>,
}

/// One line of the standings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StandingRow {
    /// Shared by tied teams.
    pub rank: NonZeroU32,
    pub entrants: Vec<EntrantRow>,
    /// How many matches it has a result of, which tells one that lost every match from one that
    /// has not played yet.
    pub played: usize,
    /// One cell per column of [`Standings::columns`], `None` where the metric is not calculated,
    /// said in the operator's language by [`Translator::cell`](crate::i18n::Translator::cell).
    pub cells: Vec<Option<Cell>>,
    /// The medal of its rank, once every match has a result; `None` before that.
    pub medal: Option<Medal>,
}

/// A medal of the first three ranks. Teams tied share the rank and its medal, and the next rank
/// is counted past them: two teams tied for first both win gold, and the next one bronze.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Medal {
    Gold,
    Silver,
    Bronze,
}

impl Medal {
    /// The medal of `rank`, if any.
    pub fn of(rank: NonZeroU32) -> Option<Self> {
        match rank.get() {
            1 => Some(Self::Gold),
            2 => Some(Self::Silver),
            3 => Some(Self::Bronze),
            _ => None,
        }
    }
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

/// What an entry of a result fits and says, see
/// [`Session::read_entered`](crate::session::Session::read_entered).
#[derive(Debug)]
pub struct Entered {
    /// The matches it fits, by their numbers, for a screen to show: the match of the number
    /// entered, or the open matches with an entrant of the name entered; none where it fits none.
    pub found: Vec<MatchRow>,
    /// The match whose result it says, one of [`Self::found`], with that result; or why it
    /// reports none.
    pub report: Result<MatchRow, ReportEnteredError>,
}

/// What reporting a result led to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reported {
    /// The match, with the result reported.
    pub row: MatchRow,
    /// The matches the result made the format schedule.
    pub new_matches: usize,
    /// Every match of the phase has a result now.
    pub all_finished: bool,
}

/// What undoing a result led to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reopened {
    /// The match, with the result it had.
    pub row: MatchRow,
    /// The matches that were only scheduled because of the result, and are gone with it.
    pub unscheduled: usize,
}

/// An action of the operator that undo and redo walk through, as it was when it was done, e.g. so
/// that a GUI can tell what an undo undid.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    AddEntrant(EntrantRow),
    RemoveEntrant(EntrantRow),
    AddPhase,
    EditPhase,
    Start,
    /// A result reported, see [`Reported::row`].
    Report(MatchRow),
    /// A result undone by its match, see [`Reopened::row`].
    Reopen(MatchRow),
}
