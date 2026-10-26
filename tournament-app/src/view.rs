//! What a [`Session`](crate::session::Session) hands a screen to show: plain data, laid out by the
//! GUI.

use std::num::NonZeroU32;

use tournament_engine::{EntrantId, MatchId, MatchResult, plugins::phase_format::Stage};
use unicode_normalization::{UnicodeNormalization as _, char::is_combining_mark};

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
