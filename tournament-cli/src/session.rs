//! A competition being run at the prompt, from the first command typed to the last.
//!
//! A [`Session`] owns the [`CompetitionStateMachine`] of the competition and executes every
//! [`Command`] against it. It is saved as that machine's transition log rather than as a snapshot
//! of the competition, so undo and redo reach back past a save and a reload.

use std::{
    collections::HashSet,
    fmt::Write as _,
    fs, io,
    num::{NonZeroU32, NonZeroUsize},
    path::{Path, PathBuf},
};

use plugin_factory::{
    catalog::AnyAxis,
    config::{ConfigError, Path as ConfigPath, PluginConfig, Value},
    draft::{self, Draft, Issue, Problem},
};
use thiserror::Error;
use tournament_engine::{
    entities::competition::{
        Competition, Entrant, EntrantId, Match, MatchId, MatchResult, Phase, PhaseId, Ranking,
        RankingId,
    },
    plugins::{
        PhasePlugins,
        metrics_calculator::{
            MetricsCalculator,
            buchholz::{Buchholz, BuchholzOfBuchholz},
            points::Points,
            round_of_elimination::RoundOfElimination,
            round_of_first_loss::RoundOfFirstLoss,
            sonneborn_berger::SonnebornBerger,
            wins_draws_losses::WinsDrawsLosses,
        },
        phase_format::{
            PhaseFormat, double_elimination::DoubleEliminationRound,
            single_elimination::KnockoutRound,
        },
        ranker::SliceRanker,
    },
    transitions::{
        CompetitionStateMachine, CompetitionStateMachineExt as _, CompetitionTransition,
        UndoMatchResultError, add_entrant::AddEntrant, add_phase::AddPhase,
        remove_entrant::RemoveEntrant, report_match_result::ReportMatchResult,
        set_competition_seed::SetCompetitionSeed, start_competition::StartCompetition,
    },
};
use undoable_state_machine::{
    LogEntry, LogEntryKind, StateMachineError, TransitionError, TransitionId,
};
use uuid::Uuid;

use crate::{
    annotations::DisplayName,
    command::{Command, PhasePreset, ReportedResult},
    questionnaire::{self, AcceptDefaults, IssueList},
    wizard,
};

/// What the prompt does after a command was executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Show the text, which may be empty, and wait for the next command.
    Continue(String),
    /// End the session.
    Exit,
}

/// Why a command was not executed. A command that fails leaves the session unchanged.
#[derive(Debug, Error)]
pub enum Error {
    #[error("an entrant name must not be empty")]
    EmptyName,
    #[error("there already is an entrant named {0:?}")]
    DuplicateName(String),
    #[error("there is no entrant named {0:?}")]
    UnknownEntrant(String),
    #[error("the competition already has a phase, and only one is supported")]
    PhaseAlreadyAdded,
    /// A session never reads from the terminal, so a phase that is not taken from a preset has to
    /// be configured before it reaches one, see [`crate::wizard::configure_phase`].
    #[error(
        "configuring a phase needs a terminal to ask at; add a preset instead, see `help add-phase`"
    )]
    NotConfigured,
    #[error("the competition has not started yet")]
    NotStarted,
    #[error("there is no match number {0}")]
    UnknownMatch(NonZeroUsize),
    #[error("#{0} has no result that could be undone")]
    NotReported(NonZeroUsize),
    /// The match whose result depends on the undone one is scheduled because of it, e.g. the final
    /// a semifinal decides the participants of.
    #[error("#{0} cannot be undone, because a later result depends on it; undo that one first")]
    ReportDependedOn(NonZeroUsize),
    #[error("there is nothing to undo")]
    NothingToUndo,
    #[error("there is nothing to redo")]
    NothingToRedo,
    #[error("the competition has not been saved yet, so a path to save it to is needed")]
    NoPath,
    #[error("there are unsaved changes; save them, or exit with --force to discard them")]
    UnsavedChanges,
    /// The competition does not accept the change in its current state, e.g. an entrant added
    /// after the start.
    #[error("{0}")]
    Rejected(TransitionError),
    /// A preset does not answer everything about its phase, see
    /// [`AcceptDefaults`].
    #[error("the preset cannot be added: {0}")]
    Preset(#[from] questionnaire::Error),
    /// The plugins of a phase do not fit together, or miss something nothing could be added for.
    #[error("the phase cannot be played this way:{}", IssueList(.0))]
    Unsettled(Vec<Issue>),
    /// A plugin added to a phase does not fit its configuration.
    #[error(transparent)]
    Configuration(#[from] ConfigError),
    #[error(transparent)]
    StateMachine(StateMachineError),
    #[error("cannot read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{} is not a competition saved by this CLI: {source}", path.display())]
    Load {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("cannot serialize the competition: {0}")]
    Serialize(serde_json::Error),
    #[error("cannot write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
}

impl From<StateMachineError> for Error {
    fn from(error: StateMachineError) -> Self {
        match error {
            StateMachineError::Transition(_, error) => Self::Rejected(error),
            error => Self::StateMachine(error),
        }
    }
}

/// A competition being run at the prompt, together with the file it is saved to.
///
/// The first entry of the log of every session is the [`SetCompetitionSeed`] it was created with,
/// which is never undone.
pub struct Session {
    machine: CompetitionStateMachine,
    path: Option<PathBuf>,
    /// The length of the log when it was last saved or loaded. The log is append-only, so every
    /// change since makes it longer, undo and redo included.
    saved_log_len: usize,
}

impl Session {
    /// Creates a session for a new competition with a random seed, to be saved to `path` once it
    /// is saved without an explicit path.
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut machine = CompetitionStateMachine::default();
        machine
            .apply(LogEntry::new(
                TransitionId::new(),
                LogEntryKind::Apply(Box::new(SetCompetitionSeed::new(rand::random()))),
            ))
            .expect("a new competition must accept a seed");
        Self {
            saved_log_len: machine.log().len(),
            machine,
            path,
        }
    }

    /// Loads the competition saved to `path`, which also becomes the file it is saved to.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or does not hold a log that replays into a
    /// competition.
    pub fn load(path: PathBuf) -> Result<Self, Error> {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) => return Err(Error::Read { path, source }),
        };
        let machine: CompetitionStateMachine = match serde_json::from_slice(&bytes) {
            Ok(machine) => machine,
            Err(source) => return Err(Error::Load { path, source }),
        };
        Ok(Self {
            saved_log_len: machine.log().len(),
            machine,
            path: Some(path),
        })
    }

    pub fn competition(&self) -> &Competition {
        self.machine.current()
    }

    /// The file the competition is saved to unless a save is given another one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.machine.log().len() != self.saved_log_len
    }

    /// Executes `command`, returning what to show and whether to go on.
    ///
    /// # Errors
    /// Returns an error if the command cannot be executed, in which case the session is unchanged.
    pub fn execute(&mut self, command: Command) -> Result<Outcome, Error> {
        let output = match command {
            Command::Entrants => self.entrants(),
            Command::AddEntrant { names } => self.add_entrants(names)?,
            Command::RemoveEntrant { name } => self.remove_entrant(&name)?,
            Command::AddPhase { preset } => self.add_preset(preset.ok_or(Error::NotConfigured)?)?,
            Command::Start => self.start()?,
            Command::Matches { open, round } => self.matches(open, round)?,
            Command::Report { number, result } => self.report(number, result)?,
            Command::UndoReport { number } => self.undo_report(number)?,
            Command::Ranking => self.ranking()?,
            Command::Undo => self.undo()?,
            Command::Redo => self.redo()?,
            Command::Save { path } => self.save(path)?,
            Command::Exit { force } => {
                if self.has_unsaved_changes() && !force {
                    return Err(Error::UnsavedChanges);
                }
                return Ok(Outcome::Exit);
            }
        };
        Ok(Outcome::Continue(output))
    }

    fn apply(&mut self, transition: impl CompetitionTransition + 'static) -> Result<(), Error> {
        self.append(LogEntryKind::Apply(Box::new(transition)))
    }

    fn append(&mut self, kind: LogEntryKind<Box<dyn CompetitionTransition>>) -> Result<(), Error> {
        self.machine
            .apply(LogEntry::new(TransitionId::new(), kind))
            .map_err(Error::from)
    }

    fn entrants(&self) -> String {
        let entrants = self.competition().entrants();
        if entrants.is_empty() {
            return "there are no entrants yet".to_owned();
        }
        entrants
            .values()
            .map(name_of)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn entrant_named(&self, name: &str) -> Option<&Entrant> {
        self.competition()
            .entrants()
            .values()
            .find(|entrant| name_of(entrant) == name)
    }

    /// Adds all of `names` or, if any of them is not a valid new name, none of them.
    fn add_entrants(&mut self, names: Vec<String>) -> Result<String, Error> {
        let mut new_names = HashSet::new();
        for name in &names {
            if name.trim().is_empty() {
                return Err(Error::EmptyName);
            }
            if self.entrant_named(name).is_some() || !new_names.insert(name.as_str()) {
                return Err(Error::DuplicateName(name.clone()));
            }
        }
        // the first entrant is rejected exactly when every other one would be, so nothing is
        // added partially
        for name in &names {
            let entrant = Entrant::new(EntrantId::new(Uuid::new_v4()))
                .with_annotation(DisplayName::new(name.as_str()));
            self.apply(AddEntrant::new(entrant))?;
        }
        Ok(format!(
            "{} entrant(s) added, {} in total",
            names.len(),
            self.competition().entrants().len()
        ))
    }

    fn remove_entrant(&mut self, name: &str) -> Result<String, Error> {
        let entrant = self
            .entrant_named(name)
            .ok_or_else(|| Error::UnknownEntrant(name.to_owned()))?
            .clone();
        self.apply(RemoveEntrant::new(entrant))?;
        Ok(format!("{name} removed"))
    }

    /// Adds the phase `draft` configures, which is the only phase the competition is played in.
    ///
    /// Every metric a configured one is derived from is calculated as well: one that only a single
    /// calculator provides is added with the parameters that calculator offers.
    ///
    /// # Errors
    /// Returns an error if the competition already has a phase, does not accept one anymore
    /// because it has started, or if the plugins `draft` configures do not fit together, e.g.
    /// [`Error::Unsettled`].
    pub fn add_phase(&mut self, draft: &Draft<PhasePlugins>) -> Result<String, Error> {
        if !self.competition().phases().is_empty() {
            return Err(Error::PhaseAlreadyAdded);
        }
        let plugins = self.settled(draft.clone())?;
        let summary = summary(&plugins);
        self.apply(AddPhase::new(PhaseId::new(Uuid::new_v4()), plugins))?;
        Ok(format!("phase added: {summary}"))
    }

    /// Adds the phase of `preset`: its format, with every other part answered as offered, see
    /// [`AcceptDefaults`].
    fn add_preset(&mut self, preset: PhasePreset) -> Result<String, Error> {
        let mut draft = Draft::new();
        let format = serde_json::to_value(preset.format()).map_err(Error::Serialize)?;
        draft.set(&ConfigPath::ROOT.field("format"), format)?;
        wizard::configure_phase(self.competition(), &mut draft, &mut AcceptDefaults)?;
        self.add_phase(&draft)
    }

    /// `draft` finished, with every metric added that one of its plugins requires and nothing
    /// calculates, if a single calculator does: the engine settles the ones that need no
    /// configuration, and the others are configured with the answers they offer, as if the
    /// operator accepted those.
    ///
    /// # Errors
    /// Returns [`Error::Unsettled`] with every issue left once nothing more can be added.
    fn settled(&self, mut draft: Draft<PhasePlugins>) -> Result<PhasePlugins, Error> {
        let context = |declaration: &mut _| self.competition().declare_phase_context(declaration);
        let metrics = ConfigPath::ROOT.field("metrics");
        loop {
            let issues = match draft::finish(context, &draft) {
                Ok(plugins) => return Ok(plugins),
                Err(issues) => issues,
            };
            let single_provider = issues.iter().find_map(|issue| match issue.problem() {
                Problem::Unprovided { providers, .. }
                    if issue.path() == &metrics && providers.len() == 1 =>
                {
                    Some(providers[0])
                }
                _ => None,
            });
            let Some(provider) = single_provider else {
                return Err(Error::Unsettled(issues));
            };
            let entry = AnyAxis::of::<dyn MetricsCalculator>()
                .entry(provider)
                .expect("a provider of the metrics is a calculator");
            let mut calculator = PluginConfig::new(provider);
            for field in entry.schema().fields() {
                if let Some(default) = field.default_value() {
                    calculator = calculator.with(field.name(), default.clone());
                }
            }
            let calculators = draft
                .answers()
                .get("metrics")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            draft.set(&metrics.clone().index(calculators), calculator)?;
        }
    }

    fn start(&mut self) -> Result<String, Error> {
        self.apply(StartCompetition)?;
        Ok(format!(
            "competition started, {} match(es) scheduled",
            self.phase()?.matches().len()
        ))
    }

    /// The single phase of a started competition.
    fn phase(&self) -> Result<&Phase, Error> {
        let competition = self.competition();
        if !competition.is_started() {
            return Err(Error::NotStarted);
        }
        Ok(competition
            .phases()
            .values()
            .next()
            .expect("a started competition must have a phase"))
    }

    /// Lists the matches, numbered the way [`Self::report`] addresses them and independently of
    /// the filters applied, followed by the byes unless only open matches are asked for.
    fn matches(&self, only_open: bool, only_round: Option<NonZeroU32>) -> Result<String, Error> {
        let phase = self.phase()?;
        // only a format that names its stages gets a column for them
        let has_stages = phase
            .matches()
            .values()
            .any(|r#match| stage_of(r#match).is_some());
        let rows: Vec<_> = phase
            .matches()
            .values()
            .enumerate()
            .filter(|(_, r#match)| !(only_open && r#match.is_finished()))
            .filter(|(_, r#match)| only_round.is_none_or(|round| r#match.round() == round))
            .map(|(index, r#match)| {
                [(index + 1).to_string(), r#match.round().to_string()]
                    .into_iter()
                    .chain(has_stages.then(|| stage_of(r#match).unwrap_or_default()))
                    .chain([
                        self.team_name(phase, r#match.team_a()),
                        "vs".to_owned(),
                        self.team_name(phase, r#match.team_b()),
                        self.result_text(phase, r#match),
                    ])
                    .collect()
            })
            .collect();
        // a bye is never open, it is decided when it is scheduled
        let bye_rows: Vec<_> = phase
            .byes()
            .values()
            .filter(|_| !only_open)
            .filter(|bye| only_round.is_none_or(|round| bye.round() == round))
            .map(|bye| {
                vec![
                    bye.round().to_string(),
                    self.team_name(phase, &[bye.ranking()]),
                ]
            })
            .collect();
        if rows.is_empty() && bye_rows.is_empty() {
            let open = if only_open { " open" } else { "" };
            return Ok(match only_round {
                Some(round) => format!("there are no{open} matches in round {round}"),
                None => format!("there are no{open} matches"),
            });
        }
        // naming the two sides `A` and `B` is what `report <number> a|b` reports a win for
        let header = ["#", "round"]
            .into_iter()
            .chain(has_stages.then_some("stage"))
            .chain(["A", "vs", "B", "result"])
            .collect::<Vec<_>>();
        let tables = [
            (!rows.is_empty()).then(|| table(&header, &rows)),
            (!bye_rows.is_empty()).then(|| table(&["round", "bye"], &bye_rows)),
        ];
        Ok(tables
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n"))
    }

    /// The phase of the match `number` addresses and that match, where the number is the position
    /// the match is listed at by [`Self::matches`].
    ///
    /// # Errors
    /// Returns an error if the competition has not started or has no such match.
    fn match_numbered(&self, number: NonZeroUsize) -> Result<(PhaseId, MatchId), Error> {
        let phase = self.phase()?;
        let (match_id, _) = phase
            .matches()
            .get_index(number.get() - 1)
            .ok_or(Error::UnknownMatch(number))?;
        Ok((phase.id(), *match_id))
    }

    fn report(&mut self, number: NonZeroUsize, result: ReportedResult) -> Result<String, Error> {
        let (phase_id, match_id) = self.match_numbered(number)?;
        let matches_before = self.phase()?.matches().len();
        let result = match result {
            ReportedResult::A => MatchResult::AWins,
            ReportedResult::B => MatchResult::BWins,
            ReportedResult::Draw => MatchResult::Draw,
        };
        self.apply(ReportMatchResult::new(phase_id, match_id, result))?;

        let phase = self.phase()?;
        let mut output = format!(
            "#{number}: {}",
            self.result_text(phase, &phase.matches()[&match_id])
        );
        let new_matches = phase.matches().len() - matches_before;
        if new_matches > 0 {
            write!(output, "\n{new_matches} new match(es) scheduled").unwrap();
        } else if phase.matches().values().all(Match::is_finished) {
            output.push_str("\nall matches are finished");
        }
        Ok(output)
    }

    /// Undoes the result reported for the match `number`, which may be any reported match rather
    /// than only the one reported last. Every other change stays in effect.
    fn undo_report(&mut self, number: NonZeroUsize) -> Result<String, Error> {
        let (phase_id, match_id) = self.match_numbered(number)?;
        let matches_before = self.phase()?.matches().len();
        self.machine
            .undo_match_result(phase_id, match_id)
            .map_err(|error| match error {
                UndoMatchResultError::NotReported => Error::NotReported(number),
                UndoMatchResultError::Rejected(_) => Error::ReportDependedOn(number),
            })?;

        let phase = self.phase()?;
        let r#match = &phase.matches()[&match_id];
        let mut output = format!(
            "#{number} is open again: {} vs {}",
            self.team_name(phase, r#match.team_a()),
            self.team_name(phase, r#match.team_b())
        );
        // a match that was only scheduled because of the undone result is gone with it
        let unscheduled = matches_before.saturating_sub(phase.matches().len());
        if unscheduled > 0 {
            write!(output, "\n{unscheduled} match(es) no longer scheduled").unwrap();
        }
        Ok(output)
    }

    fn ranking(&self) -> Result<String, Error> {
        let phase = self.phase()?;
        let rankings: Vec<_> = phase
            .ranking_ids_ordered_by_unique_rank()
            .iter()
            .map(|id| &phase.rankings()[id])
            .collect();
        // every ranking of a phase has the same metrics, so the first one tells which to show
        let columns: Vec<_> = METRIC_COLUMNS
            .iter()
            .filter(|(_, cell)| cell(rankings[0]).is_some())
            .collect();
        let header: Vec<_> = ["#", "name"]
            .into_iter()
            .chain(columns.iter().map(|(header, _)| *header))
            .collect();
        let rows: Vec<_> = rankings
            .iter()
            .map(|ranking| {
                [
                    ranking.rank().to_string(),
                    self.team_name(phase, &[ranking.id()]),
                ]
                .into_iter()
                .chain(columns.iter().map(|(_, cell)| cell(ranking).unwrap()))
                .collect()
            })
            .collect();
        Ok(table(&header, &rows))
    }

    /// Undoes the change that is the latest one still in effect.
    fn undo(&mut self) -> Result<String, Error> {
        let log = self.machine.log();
        let in_effect = in_effect(log);
        // the first entry is the seed of the session, see [`Session`]
        let target = log
            .iter()
            .zip(&in_effect)
            .skip(1)
            .rev()
            .find(|(entry, in_effect)| **in_effect && is_change(entry))
            .map(|(entry, _)| entry.id())
            .ok_or(Error::NothingToUndo)?;
        self.append(LogEntryKind::Undo(target))?;
        Ok("undone".to_owned())
    }

    /// Redoes the latest undone change by undoing its undo, as long as no other change was made
    /// since that undo.
    fn redo(&mut self) -> Result<String, Error> {
        let log = self.machine.log();
        let in_effect = in_effect(log);
        let after_last_change = log.iter().rposition(is_change).map_or(0, |index| index + 1);
        let target = log
            .iter()
            .zip(&in_effect)
            .skip(after_last_change)
            .rev()
            .find(|(entry, in_effect)| {
                // an undo of an undo is a redo itself, which is not to be redone
                **in_effect
                    && matches!(entry.kind(), LogEntryKind::Undo(target)
                        if log.iter().any(|entry| entry.id() == *target && is_change(entry)))
            })
            .map(|(entry, _)| entry.id())
            .ok_or(Error::NothingToRedo)?;
        self.append(LogEntryKind::Undo(target))?;
        Ok("redone".to_owned())
    }

    fn save(&mut self, path: Option<PathBuf>) -> Result<String, Error> {
        let path = path.or_else(|| self.path.clone()).ok_or(Error::NoPath)?;
        let json = serde_json::to_vec_pretty(&self.machine).map_err(Error::Serialize)?;
        // writing to a sibling first and moving that over the file means a failed write never
        // destroys the previous save
        let mut temporary = path.clone().into_os_string();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        if let Err(source) = fs::write(&temporary, json) {
            return Err(Error::Write {
                path: temporary,
                source,
            });
        }
        if let Err(source) = fs::rename(&temporary, &path) {
            return Err(Error::Write { path, source });
        }
        let output = format!("saved to {}", path.display());
        self.path = Some(path);
        self.saved_log_len = self.machine.log().len();
        Ok(output)
    }

    fn team_name(&self, phase: &Phase, rankings: &[RankingId]) -> String {
        rankings
            .iter()
            .flat_map(|id| phase.rankings()[id].entrants())
            .map(|id| name_of(&self.competition().entrants()[id]))
            .collect::<Vec<_>>()
            .join(" & ")
    }

    fn result_text(&self, phase: &Phase, r#match: &Match) -> String {
        let a = self.team_name(phase, r#match.team_a());
        let b = self.team_name(phase, r#match.team_b());
        match r#match.result() {
            MatchResult::AWins => format!("{a} beat {b}"),
            MatchResult::BWins => format!("{b} beat {a}"),
            MatchResult::Draw => "draw".to_owned(),
            MatchResult::NotFinished => "open".to_owned(),
        }
    }
}

/// Names the format and the ranking of `plugins` in one line, the way a phase that was just added
/// is reported, e.g. "round robin, ranked by more points, then more Sonneborn-Berger".
fn summary(plugins: &PhasePlugins) -> String {
    let draft = Draft::from_built(plugins).expect("a built plugin is a configuration of itself");
    let answers = draft.answers();
    let format = answers
        .get("format")
        .and_then(|format| questionnaire::describe(AnyAxis::of::<dyn PhaseFormat>(), format))
        .expect("a phase has a format");
    let ranked_by: Vec<_> = answers
        .get("ranking")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|ranker| questionnaire::describe(AnyAxis::of::<dyn SliceRanker>(), ranker))
        .collect();
    if ranked_by.is_empty() {
        return format!("{format}, not ranked by anything");
    }
    format!("{format}, ranked by {}", ranked_by.join(", then "))
}

/// The name `entrant` is shown with. Every entrant this CLI adds has one, so the fallback is only
/// for a log written by someone else.
fn name_of(entrant: &Entrant) -> &str {
    entrant
        .annotation::<DisplayName>()
        .map_or("<unnamed>", DisplayName::as_str)
}

/// The name of the stage `r#match` belongs to, if its format names one.
fn stage_of(r#match: &Match) -> Option<String> {
    if let Some(stage) = r#match.annotation::<KnockoutRound>() {
        return Some(knockout_round_name(*stage));
    }
    r#match
        .annotation::<DoubleEliminationRound>()
        .copied()
        .map(double_elimination_round_name)
}

fn double_elimination_round_name(stage: DoubleEliminationRound) -> String {
    let bracket_round = |bracket, round, rounds| {
        if round == rounds {
            format!("{bracket} final")
        } else {
            format!("{bracket} round {round}")
        }
    };
    match stage {
        DoubleEliminationRound::Winners { round, rounds } => {
            bracket_round("Winners", round, rounds)
        }
        DoubleEliminationRound::Losers { round, rounds } => bracket_round("Losers", round, rounds),
        DoubleEliminationRound::GrandFinal => "Grand final".to_owned(),
        DoubleEliminationRound::GrandFinalReset => "Grand final reset".to_owned(),
    }
}

fn knockout_round_name(stage: KnockoutRound) -> String {
    match stage.fraction_of_final() {
        None => "Third place".to_owned(),
        Some(1) => "Final".to_owned(),
        Some(2) => "Semifinals".to_owned(),
        Some(4) => "Quarterfinals".to_owned(),
        Some(fraction) => format!("{fraction}{}-finals", ordinal_suffix(fraction)),
    }
}

/// The English ordinal suffix of `n`, e.g. "nd" for 32.
fn ordinal_suffix(n: u32) -> &'static str {
    match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

fn is_change(entry: &LogEntry<Box<dyn CompetitionTransition>>) -> bool {
    matches!(entry.kind(), LogEntryKind::Apply(_))
}

/// Whether each entry of `log` is still in effect, i.e. not undone by a later entry that is in
/// effect itself.
fn in_effect<T>(log: &[LogEntry<T>]) -> Vec<bool> {
    let mut undone = HashSet::new();
    let mut in_effect = vec![false; log.len()];
    for (index, entry) in log.iter().enumerate().rev() {
        if undone.contains(&entry.id()) {
            continue;
        }
        in_effect[index] = true;
        if let LogEntryKind::Undo(target) = entry.kind() {
            undone.insert(*target);
        }
    }
    in_effect
}

/// The metrics the ranking can show, each with its column header. Only those a phase calculates
/// are shown for it.
type MetricColumn = (&'static str, fn(&Ranking) -> Option<String>);

const METRIC_COLUMNS: &[MetricColumn] = &[
    ("lost in", |ranking| {
        ranking.metric::<RoundOfFirstLoss>().map(|lost_in| {
            lost_in
                .0
                .map_or("-".to_owned(), |round| format!("round {round}"))
        })
    }),
    ("out in", |ranking| {
        ranking.metric::<RoundOfElimination>().map(|out_in| {
            out_in
                .0
                .map_or("-".to_owned(), |round| format!("round {round}"))
        })
    }),
    ("W-D-L", |ranking| {
        ranking
            .metric::<WinsDrawsLosses>()
            .map(|wdl| format!("{}-{}-{}", wdl.wins, wdl.draws, wdl.losses))
    }),
    ("points", |ranking| {
        ranking
            .metric::<Points>()
            .map(|points| points.0.to_string())
    }),
    ("Buchholz", |ranking| {
        ranking
            .metric::<Buchholz>()
            .map(|value| value.0.to_string())
    }),
    ("Sonneborn-Berger", |ranking| {
        ranking
            .metric::<SonnebornBerger>()
            .map(|value| value.0.to_string())
    }),
    ("Buchholz of Buchholz", |ranking| {
        ranking
            .metric::<BuchholzOfBuchholz>()
            .map(|value| value.0.to_string())
    }),
];

/// Lays `header` and `rows` out as left-aligned columns, with the header underlined.
fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<_> = header.iter().map(|cell| cell.chars().count()).collect();
    for row in rows {
        widths.resize(widths.len().max(row.len()), 0);
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let underline: Vec<_> = widths.iter().map(|width| "-".repeat(*width)).collect();
    let body = rows
        .iter()
        .map(|row| row.iter().map(String::as_str).collect());
    [
        header.to_vec(),
        underline.iter().map(String::as_str).collect(),
    ]
    .into_iter()
    .chain(body)
    .map(|row: Vec<&str>| {
        let mut line = String::new();
        for (cell, width) in row.iter().zip(&widths) {
            write!(line, "{cell:width$}  ").unwrap();
        }
        line.trim_end().to_owned()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[cfg(test)]
mod tests {
    use tournament_engine::plugins::{
        metrics_calculator::{
            points::{MorePoints, PointsCalculator},
            sonneborn_berger::SonnebornBergerCalculator,
            wins_draws_losses::{FewerMatchesPlayed, MoreWins, WinsDrawsLossesCalculator},
        },
        phase_format::{round_robin::RoundRobin, single_elimination::SingleElimination},
    };

    use super::*;

    /// The header and its underline, which every table shown by a command starts with.
    const HEADER_LINES: usize = 2;

    /// Adds the phase played in `format`, ranked by `ranking` and calculating `metrics`.
    fn add_phase(
        session: &mut Session,
        format: impl PhaseFormat + 'static,
        ranking: Vec<Box<dyn SliceRanker>>,
        metrics: Vec<Box<dyn MetricsCalculator>>,
    ) -> Result<String, Error> {
        let plugins = PhasePlugins::new(Box::new(format), ranking, metrics).unwrap();
        session.add_phase(&Draft::from_built(&plugins).unwrap())
    }

    #[track_caller]
    fn execute(session: &mut Session, line: &str) -> Result<Outcome, Error> {
        let command = Command::parse_line(line)
            .expect("line must parse")
            .expect("line must hold a command");
        session.execute(command)
    }

    /// Executes `line`, which has to succeed, and returns what it shows.
    #[track_caller]
    fn output(session: &mut Session, line: &str) -> String {
        match execute(session, line) {
            Ok(Outcome::Continue(output)) => output,
            other => panic!("{line:?} must succeed and continue, got {other:?}"),
        }
    }

    fn names(session: &Session) -> Vec<&str> {
        session
            .competition()
            .entrants()
            .values()
            .map(name_of)
            .collect()
    }

    /// A session with a started competition between Alice, Bob, Carol and Dave in a phase of
    /// `preset`.
    fn started(preset: &str) -> Session {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol Dave");
        output(&mut session, &format!("add-phase {preset}"));
        output(&mut session, "start");
        session
    }

    #[test]
    fn entrants_are_listed_by_name() {
        let mut session = Session::new(None);

        output(&mut session, "add Alice 'Bob B.'");

        assert_eq!(output(&mut session, "entrants"), "Alice\nBob B.");
    }

    #[test]
    fn a_duplicate_name_adds_none_of_the_entrants() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice");

        let result = execute(&mut session, "add Bob Alice");

        assert!(matches!(result, Err(Error::DuplicateName(name)) if name == "Alice"));
        assert_eq!(names(&session), ["Alice"]);
    }

    #[test]
    fn a_name_cannot_be_given_twice_in_one_command() {
        let mut session = Session::new(None);

        let result = execute(&mut session, "add Bob Bob");

        assert!(matches!(result, Err(Error::DuplicateName(_))));
        assert!(names(&session).is_empty());
    }

    #[test]
    fn an_entrant_is_removed_by_name() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob");

        output(&mut session, "remove Alice");

        assert_eq!(names(&session), ["Bob"]);
        assert!(matches!(
            execute(&mut session, "remove Alice"),
            Err(Error::UnknownEntrant(_))
        ));
    }

    #[test]
    fn only_one_phase_can_be_added() {
        let mut session = Session::new(None);
        output(&mut session, "add-phase round-robin");

        let result = execute(&mut session, "add-phase single-elimination");

        assert!(matches!(result, Err(Error::PhaseAlreadyAdded)));
    }

    /// Every preset is its format with the plugins the format recommends, which the engine checks
    /// against what the plugins declare when the phase is added.
    #[test]
    fn every_preset_adds_the_phase_its_format_recommends() {
        let presets = [
            (
                "round-robin",
                "round robin, ranked by more points, then more Sonneborn-Berger",
            ),
            (
                "single-elimination",
                "single elimination (match for third place: yes), ranked by later first loss, \
                 then fewer losses",
            ),
            (
                "single-elimination --no-third-place-match",
                "single elimination (match for third place: no), ranked by later first loss",
            ),
            (
                "double-elimination",
                "double elimination (grand final reset: yes), ranked by later elimination, then \
                 fewer losses",
            ),
            (
                "random-rounds 3",
                "random rounds (rounds: 3), ranked by more points, then more Buchholz, then more \
                 Sonneborn-Berger, then more Buchholz of Buchholz",
            ),
            (
                "monster-dyp 4",
                "Monster DYP (rounds: 4), ranked by more points",
            ),
        ];

        for (preset, summary) in presets {
            let mut session = Session::new(None);

            match execute(&mut session, &format!("add-phase {preset}")) {
                Ok(Outcome::Continue(added)) => {
                    assert_eq!(added, format!("phase added: {summary}"));
                }
                other => panic!("{preset} adds no phase: {other:?}"),
            }
        }
    }

    /// A preset of a format offering points calculates them the way the points calculator offers,
    /// i.e. scoring a bye like a win.
    #[test]
    fn a_preset_calculates_the_metrics_its_format_recommends() {
        let mut session = started("round-robin");

        let ranking = output(&mut session, "ranking");

        assert_eq!(
            ranking
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["#", "name", "W-D-L", "points", "Sonneborn-Berger"],
            "{ranking}"
        );
    }

    /// A session executes commands and never asks the operator anything, so the phase of an
    /// `add-phase` without a preset has to be configured before the command reaches it.
    #[test]
    fn a_phase_to_be_configured_cannot_be_added_by_the_session_itself() {
        let mut session = Session::new(None);

        let result = execute(&mut session, "add-phase");

        assert!(matches!(result, Err(Error::NotConfigured)));
        assert!(session.competition().phases().is_empty());
    }

    /// A configured phase calculates exactly the metrics it was configured with, and scores them
    /// the way it was configured to.
    #[test]
    fn a_configured_phase_is_played_the_way_it_was_configured() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol Dave");

        let added = add_phase(
            &mut session,
            RoundRobin,
            vec![Box::new(MorePoints)],
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(PointsCalculator::new(1.0, 0.5)),
            ],
        )
        .unwrap();

        assert_eq!(added, "phase added: round robin, ranked by more points");
        output(&mut session, "start");
        for number in 1..=6 {
            output(&mut session, &format!("report {number} a"));
        }
        let ranking = output(&mut session, "ranking");
        assert_eq!(
            ranking
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["#", "name", "W-D-L", "points"],
            "the Sonneborn-Berger of the preset was not configured, so it is not calculated"
        );
        for row in ranking.lines().skip(HEADER_LINES) {
            let columns: Vec<_> = row.split_whitespace().collect();
            let wins = columns[2].split('-').next().unwrap();
            assert_eq!(
                columns[3], wins,
                "every win is worth a single point, {row:?}"
            );
        }
    }

    /// Which metrics a metric is derived from is the engine's business: the Sonneborn-Berger sums
    /// the points of the opponents a ranking met, so a phase calculating it also calculates the
    /// points and the results they were scored from, neither of which was configured.
    #[test]
    fn a_phase_calculates_the_metrics_its_own_are_derived_from() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol Dave");
        add_phase(
            &mut session,
            RoundRobin,
            Vec::new(),
            vec![Box::new(SonnebornBergerCalculator::new(1.0, 0.5))],
        )
        .unwrap();
        output(&mut session, "start");
        for number in 1..=6 {
            output(&mut session, &format!("report {number} a"));
        }

        let ranking = output(&mut session, "ranking");

        assert_eq!(
            ranking
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["#", "name", "W-D-L", "points", "Sonneborn-Berger"],
            "{ranking}"
        );
    }

    /// Wins, draws, losses and the matches they add up to are four criteria of a single metric, so
    /// a phase can be ranked by the wins alone, without calculating anything besides that metric.
    #[test]
    fn a_phase_can_be_ranked_by_wins_alone() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol Dave");
        let added = add_phase(
            &mut session,
            RoundRobin,
            vec![Box::new(MoreWins)],
            Vec::new(),
        )
        .unwrap();

        assert_eq!(added, "phase added: round robin, ranked by more wins");
        output(&mut session, "start");
        for number in 1..=6 {
            output(&mut session, &format!("report {number} a"));
        }
        let ranking = output(&mut session, "ranking");
        assert_eq!(
            ranking
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["#", "name", "W-D-L"],
            "ranking by wins needs no metric besides the one holding them"
        );
        let wins: Vec<&str> = ranking
            .lines()
            .skip(HEADER_LINES)
            .map(|row| {
                let columns: Vec<_> = row.split_whitespace().collect();
                columns[2].split('-').next().unwrap()
            })
            .collect();
        assert!(
            wins.windows(2).all(|pair| pair[0] >= pair[1]),
            "the entrants must be ordered by their wins, got {wins:?}"
        );
        // every match was won by a team, so the six wins cannot be spread evenly over the four
        // entrants, which is what makes the order above more than a tie
        assert!(
            wins.iter().any(|win| win != &wins[0]),
            "the entrants must differ in their wins, got {wins:?}"
        );
    }

    /// Fewer matches played is the better end of that criterion, which a knockout shows most
    /// plainly because it is the simplest way to have entrants play a different number of matches:
    /// the two that went out in the semifinal rank above the two finalists.
    #[test]
    fn ranking_by_the_matches_played_puts_the_entrants_that_played_fewest_first() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol Dave");
        add_phase(
            &mut session,
            // a match for third place would have every entrant play two matches, which is exactly
            // the tie this criterion is shown by breaking
            SingleElimination::new(),
            vec![Box::new(FewerMatchesPlayed)],
            Vec::new(),
        )
        .unwrap();
        output(&mut session, "start");

        output(&mut session, "report 1 a");
        output(&mut session, "report 2 b");
        output(&mut session, "report 3 a");

        let ranking = output(&mut session, "ranking");
        let ranks_and_matches: Vec<(&str, u32)> = ranking
            .lines()
            .skip(HEADER_LINES)
            .map(|row| {
                let columns: Vec<_> = row.split_whitespace().collect();
                let played = columns[2]
                    .split('-')
                    .map(|count| count.parse::<u32>().expect("a result count"))
                    .sum();
                (columns[0], played)
            })
            .collect();
        assert_eq!(
            ranks_and_matches,
            [("1", 1), ("1", 1), ("3", 2), ("3", 2)],
            "the two that played one match tie above the finalists, who played two: {ranking}"
        );
    }

    #[test]
    fn a_phase_configured_without_a_ranking_leaves_every_entrant_tied() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol Dave");
        let added = add_phase(
            &mut session,
            RoundRobin,
            Vec::new(),
            vec![Box::new(WinsDrawsLossesCalculator)],
        )
        .unwrap();
        assert_eq!(added, "phase added: round robin, not ranked by anything");
        output(&mut session, "start");

        for number in 1..=6 {
            output(&mut session, &format!("report {number} a"));
        }

        let ranking = output(&mut session, "ranking");
        assert!(
            ranking
                .lines()
                .skip(HEADER_LINES)
                .all(|row| row.starts_with('1')),
            "nothing sorts the entrants, so all of them share the first rank: {ranking}"
        );
    }

    #[test]
    fn matches_and_ranking_need_a_started_competition() {
        let mut session = Session::new(None);

        assert!(matches!(
            execute(&mut session, "matches"),
            Err(Error::NotStarted)
        ));
        assert!(matches!(
            execute(&mut session, "ranking"),
            Err(Error::NotStarted)
        ));
    }

    #[test]
    fn the_engine_rejecting_a_change_is_reported() {
        let mut session = started("round-robin");

        let result = execute(&mut session, "add Eve");

        assert_eq!(
            result.unwrap_err().to_string(),
            "competition already started"
        );
    }

    #[test]
    fn a_round_robin_is_played_to_the_end() {
        let mut session = started("round-robin");
        let matches = output(&mut session, "matches --open");
        assert_eq!(
            matches.lines().skip(HEADER_LINES).count(),
            6,
            "everyone plays everyone"
        );

        for number in 1..=5 {
            output(&mut session, &format!("report {number} a"));
        }
        let last = output(&mut session, "report 6 draw");

        assert_eq!(
            output(&mut session, "matches --open"),
            "there are no open matches"
        );
        assert!(last.ends_with("all matches are finished"), "{last}");
        let ranking = output(&mut session, "ranking");
        let header = ranking.lines().next().unwrap();
        assert_eq!(
            header.split_whitespace().collect::<Vec<_>>(),
            ["#", "name", "W-D-L", "points", "Sonneborn-Berger"]
        );
        assert_eq!(ranking.lines().skip(HEADER_LINES).count(), 4);
    }

    /// The number and the round of every match listed by `matches`.
    fn numbered_rounds(listing: &str) -> Vec<(&str, &str)> {
        listing
            .lines()
            .skip(HEADER_LINES)
            .map(|row| {
                let mut columns = row.split_whitespace();
                (columns.next().unwrap(), columns.next().unwrap())
            })
            .collect()
    }

    /// Filtering by round narrows the listing down to that round while leaving the numbers the
    /// matches are reported by untouched.
    #[test]
    fn matches_can_be_filtered_by_round() {
        let mut session = started("round-robin");
        let all = output(&mut session, "matches");
        let in_round_two: Vec<_> = numbered_rounds(&all)
            .into_iter()
            .filter(|(_, round)| *round == "2")
            .collect();
        assert_eq!(
            in_round_two.len(),
            2,
            "a round robin between four entrants plays two matches a round"
        );

        let filtered = output(&mut session, "matches --round 2");

        assert_eq!(numbered_rounds(&filtered), in_round_two);
    }

    /// The stage of every match listed by `matches`, i.e. its third column.
    fn listed_stages(listing: &str) -> Vec<&str> {
        listing
            .lines()
            .skip(HEADER_LINES)
            .map(|row| row.split_whitespace().nth(2).unwrap())
            .collect()
    }

    #[test]
    fn a_knockout_lists_the_stage_of_every_match() {
        let mut session = started("single-elimination");
        assert_eq!(
            listed_stages(&output(&mut session, "matches")),
            ["Semifinals"; 2]
        );
        output(&mut session, "report 1 a");
        output(&mut session, "report 2 a");

        let listing = output(&mut session, "matches --round 2");

        assert_eq!(listed_stages(&listing), ["Final", "Third"]);
        assert!(listing.contains("Third place"), "{listing}");
    }

    /// Side A wins every match, so the favourite of every match advances and the champion of the
    /// winner bracket wins the grand final, which needs no reset then.
    #[test]
    fn a_double_elimination_plays_through_both_brackets() {
        let mut session = started("double-elimination");
        for number in 1..=6 {
            output(&mut session, &format!("report {number} a"));
        }

        let listing = output(&mut session, "matches");
        let stages: Vec<_> = listing
            .lines()
            .skip(HEADER_LINES)
            .map(|row| {
                row.split("  ")
                    .filter(|cell| !cell.is_empty())
                    .nth(2)
                    .unwrap()
                    .trim()
            })
            .collect();
        assert_eq!(
            stages,
            [
                "Winners round 1",
                "Winners round 1",
                "Winners final",
                "Losers round 1",
                "Losers final",
                "Grand final",
            ],
            "{listing}"
        );

        let ranking = output(&mut session, "ranking");
        let rows: Vec<Vec<&str>> = ranking
            .lines()
            .skip(HEADER_LINES)
            .map(|row| row.split_whitespace().collect())
            .collect();
        assert_eq!(
            rows.iter().map(|row| row[0]).collect::<Vec<_>>(),
            ["1", "2", "3", "4"],
            "every entrant is knocked out in a round of its own: {ranking}"
        );
        assert_eq!(rows[0][2..], ["-", "3-0-0"], "{ranking}");
        assert_eq!(rows[1][2..], ["round", "4", "2-0-2"], "{ranking}");
    }

    #[test]
    fn double_elimination_rounds_are_named_by_their_bracket() {
        let names = [
            DoubleEliminationRound::Winners {
                round: 1,
                rounds: 3,
            },
            DoubleEliminationRound::Winners {
                round: 3,
                rounds: 3,
            },
            DoubleEliminationRound::Losers {
                round: 2,
                rounds: 4,
            },
            DoubleEliminationRound::Losers {
                round: 4,
                rounds: 4,
            },
            DoubleEliminationRound::GrandFinal,
            DoubleEliminationRound::GrandFinalReset,
        ]
        .map(double_elimination_round_name);

        assert_eq!(
            names,
            [
                "Winners round 1",
                "Winners final",
                "Losers round 2",
                "Losers final",
                "Grand final",
                "Grand final reset"
            ]
        );
    }

    /// A round robin of three has one entrant sit out every round, which is listed after the
    /// matches but not among the open ones.
    #[test]
    fn byes_are_listed_after_the_matches() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice Bob Carol");
        output(&mut session, "add-phase round-robin");
        output(&mut session, "start");

        let listing = output(&mut session, "matches --round 2");

        let (matches, byes) = listing.split_once("\n\n").expect("two tables");
        assert_eq!(matches.lines().skip(HEADER_LINES).count(), 1, "{listing}");
        let byes: Vec<Vec<&str>> = byes
            .lines()
            .map(|row| row.split_whitespace().collect())
            .collect();
        assert_eq!(byes[0], ["round", "bye"], "{listing}");
        assert_eq!(byes.len(), HEADER_LINES + 1, "{listing}");
        assert_eq!(byes[HEADER_LINES][0], "2", "{listing}");
        assert!(
            !output(&mut session, "matches --open").contains("bye"),
            "a bye is never open"
        );
    }

    #[test]
    fn a_format_without_stages_lists_no_stage_column() {
        let mut session = started("round-robin");

        let listing = output(&mut session, "matches");

        assert_eq!(
            listing
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["#", "round", "A", "vs", "B", "result"]
        );
    }

    #[test]
    fn knockout_rounds_are_named_by_their_fraction_of_the_final() {
        let names: Vec<_> = [2, 4, 8, 16, 32, 64, 128, 1024]
            .map(|entrants| knockout_round_name(KnockoutRound::RoundOf(entrants)))
            .into();

        assert_eq!(
            names,
            [
                "Final",
                "Semifinals",
                "Quarterfinals",
                "8th-finals",
                "16th-finals",
                "32nd-finals",
                "64th-finals",
                "512th-finals"
            ]
        );
    }

    #[test]
    fn ordinal_suffixes_follow_english_rules() {
        let suffixes = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 111, 112, 1001].map(ordinal_suffix);

        assert_eq!(
            suffixes,
            [
                "st", "nd", "rd", "th", "th", "th", "th", "st", "nd", "rd", "th", "th", "st"
            ]
        );
    }

    #[test]
    fn a_round_that_is_not_played_is_reported_as_empty() {
        let mut session = started("round-robin");

        assert_eq!(
            output(&mut session, "matches --round 4"),
            "there are no matches in round 4",
            "a round robin between four entrants is played in three rounds"
        );
    }

    #[test]
    fn the_round_filter_combines_with_the_open_filter() {
        let mut session = started("round-robin");
        let listing = output(&mut session, "matches --round 2");
        for (number, _) in numbered_rounds(&listing) {
            output(&mut session, &format!("report {number} a"));
        }

        assert_eq!(
            output(&mut session, "matches --open --round 2"),
            "there are no open matches in round 2"
        );
        assert_eq!(
            output(&mut session, "matches --open --round 1")
                .lines()
                .skip(HEADER_LINES)
                .count(),
            2,
            "reporting the matches of round 2 leaves those of round 1 open"
        );
    }

    #[test]
    fn a_match_is_reported_once() {
        let mut session = started("round-robin");
        output(&mut session, "report 1 b");

        let result = execute(&mut session, "report 1 a");

        assert_eq!(result.unwrap_err().to_string(), "match already finished");
    }

    #[test]
    fn an_unknown_match_number_is_rejected() {
        let mut session = started("round-robin");

        let result = execute(&mut session, "report 7 a");

        assert!(matches!(result, Err(Error::UnknownMatch(number)) if number.get() == 7));
    }

    #[test]
    fn single_elimination_schedules_the_final_and_rejects_draws() {
        let mut session = started("single-elimination");
        let log_len = session.machine.log().len();

        let draw = execute(&mut session, "report 1 draw");

        assert_eq!(
            draw.unwrap_err().to_string(),
            "the phase format does not allow draws"
        );
        assert_eq!(session.machine.log().len(), log_len);
        output(&mut session, "report 1 a");
        let semifinal = output(&mut session, "report 2 b");
        assert!(
            semifinal.ends_with("2 new match(es) scheduled"),
            "the final and the match for third place: {semifinal}"
        );
        output(&mut session, "report 3 a");
        let third_place = output(&mut session, "report 4 a");
        assert!(
            third_place.ends_with("all matches are finished"),
            "{third_place}"
        );

        let ranking = output(&mut session, "ranking");
        let rows: Vec<Vec<&str>> = ranking
            .lines()
            .skip(HEADER_LINES)
            .map(|row| row.split_whitespace().collect())
            .collect();
        assert_eq!(
            rows.iter().map(|row| row[0]).collect::<Vec<_>>(),
            ["1", "2", "3", "4"],
            "the match for third place leaves no two entrants tied: {ranking}"
        );
        assert_eq!(rows[0][2..], ["-", "2-0-0"], "{ranking}");
        assert_eq!(rows[1][2..], ["round", "2", "1-0-1"], "{ranking}");
        // both entrants that lost the semifinal first lost in round 1, so what tells them apart is
        // the match for third place, which the one ranked third won and the one ranked fourth lost
        assert_eq!(rows[2][2..], ["round", "1", "1-0-1"], "{ranking}");
        assert_eq!(rows[3][2..], ["round", "1", "0-0-2"], "{ranking}");
    }

    /// Turning the match for third place off ends the bracket with the final, which leaves the two
    /// entrants knocked out in the semifinals tied for the third rank.
    #[test]
    fn a_knockout_without_a_match_for_third_place_ends_with_the_final() {
        let mut session = started("single-elimination --no-third-place-match");

        output(&mut session, "report 1 a");
        let semifinal = output(&mut session, "report 2 b");
        assert!(
            semifinal.ends_with("1 new match(es) scheduled"),
            "the final and nothing else: {semifinal}"
        );
        let r#final = output(&mut session, "report 3 a");
        assert!(r#final.ends_with("all matches are finished"), "{final}");

        let ranking = output(&mut session, "ranking");
        let ranks: Vec<_> = ranking
            .lines()
            .skip(HEADER_LINES)
            .map(|row| row.split_whitespace().next().expect("a rank"))
            .collect();
        assert_eq!(ranks, ["1", "2", "3", "3"], "{ranking}");
    }

    /// The result of a match reported long ago is undone by its number, which leaves every result
    /// reported after it alone — unlike `undo`, which would walk back through all of them.
    #[test]
    fn a_reported_result_is_undone_by_match_number() {
        let mut session = started("round-robin");
        for number in 1..=4 {
            output(&mut session, &format!("report {number} a"));
        }

        let undone = output(&mut session, "undo-report 2");

        assert!(undone.starts_with("#2 is open again: "), "{undone}");
        let listing = output(&mut session, "matches --open");
        let open: Vec<_> = numbered_rounds(&listing)
            .into_iter()
            .map(|(number, _)| number)
            .collect();
        assert_eq!(open, ["2", "5", "6"]);
    }

    #[test]
    fn a_match_without_a_result_has_nothing_to_undo() {
        let mut session = started("round-robin");

        assert!(matches!(
            execute(&mut session, "undo-report 1"),
            Err(Error::NotReported(number)) if number.get() == 1
        ));
        assert!(matches!(
            execute(&mut session, "undo-report 7"),
            Err(Error::UnknownMatch(number)) if number.get() == 7
        ));
    }

    /// A reported result can be undone and reported again, which is what correcting a result that
    /// was reported wrongly a while ago comes down to.
    #[test]
    fn a_match_can_be_reported_again_after_its_result_was_undone() {
        let mut session = started("round-robin");
        output(&mut session, "report 1 a");
        let ranking = output(&mut session, "ranking");

        output(&mut session, "undo-report 1");
        output(&mut session, "report 1 b");

        assert_ne!(output(&mut session, "ranking"), ranking);
        output(&mut session, "undo-report 1");
        output(&mut session, "report 1 a");
        assert_eq!(output(&mut session, "ranking"), ranking);
    }

    /// Undoing the result of a semifinal unschedules the final it decided the participants of, so
    /// a final that has a result itself has to be undone first.
    #[test]
    fn undoing_a_result_a_later_result_depends_on_is_refused() {
        let mut session = started("single-elimination");
        output(&mut session, "report 1 a");
        output(&mut session, "report 2 a");
        output(&mut session, "report 3 a");

        assert!(matches!(
            execute(&mut session, "undo-report 1"),
            Err(Error::ReportDependedOn(number)) if number.get() == 1
        ));
        output(&mut session, "undo-report 3");
        let undone = output(&mut session, "undo-report 1");

        assert!(
            undone.ends_with("2 match(es) no longer scheduled"),
            "the final and the match for third place are unscheduled with it: {undone}"
        );
    }

    #[test]
    fn undo_and_redo_walk_back_and_forth_through_the_changes() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice");
        output(&mut session, "add Bob");

        output(&mut session, "undo");
        assert_eq!(names(&session), ["Alice"]);
        output(&mut session, "undo");
        assert!(names(&session).is_empty());
        output(&mut session, "redo");
        assert_eq!(names(&session), ["Alice"]);
        output(&mut session, "redo");
        assert_eq!(names(&session), ["Alice", "Bob"]);
        assert!(matches!(
            execute(&mut session, "redo"),
            Err(Error::NothingToRedo)
        ));
        output(&mut session, "undo");
        assert_eq!(names(&session), ["Alice"]);
    }

    #[test]
    fn a_new_change_discards_what_could_be_redone() {
        let mut session = Session::new(None);
        output(&mut session, "add Alice");
        output(&mut session, "undo");

        output(&mut session, "add Bob");

        assert!(matches!(
            execute(&mut session, "redo"),
            Err(Error::NothingToRedo)
        ));
    }

    #[test]
    fn the_seed_of_a_session_is_never_undone() {
        let mut session = Session::new(None);

        assert!(matches!(
            execute(&mut session, "undo"),
            Err(Error::NothingToUndo)
        ));
    }

    #[test]
    fn undoing_the_start_reopens_the_registration() {
        let mut session = started("round-robin");

        output(&mut session, "undo");

        assert!(!session.competition().is_started());
        output(&mut session, "add Eve");
    }

    #[test]
    fn a_saved_competition_loads_with_its_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cup.json");
        let mut session = started("round-robin");
        output(&mut session, "report 1 a");
        let ranking = output(&mut session, "ranking");

        output(&mut session, &format!("save {}", path.display()));
        let mut loaded = Session::load(path.clone()).unwrap();

        assert_eq!(loaded.path(), Some(path.as_path()));
        assert!(!loaded.has_unsaved_changes());
        assert_eq!(output(&mut loaded, "ranking"), ranking);
        assert_eq!(
            output(&mut loaded, "matches"),
            output(&mut session, "matches")
        );
        output(&mut loaded, "undo");
        assert!(
            output(&mut loaded, "matches")
                .lines()
                .skip(HEADER_LINES)
                .all(|line| line.ends_with("open"))
        );
    }

    #[test]
    fn a_save_without_a_path_goes_to_the_last_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cup.json");
        let mut session = Session::new(Some(path.clone()));
        output(&mut session, "add Alice");

        output(&mut session, "save");
        output(&mut session, "add Bob");
        output(&mut session, "save");

        assert_eq!(names(&Session::load(path).unwrap()), ["Alice", "Bob"]);
    }

    #[test]
    fn a_session_that_was_never_saved_needs_a_path() {
        let mut session = Session::new(None);

        assert!(matches!(execute(&mut session, "save"), Err(Error::NoPath)));
    }

    #[test]
    fn exit_refuses_to_discard_unsaved_changes_unless_forced() {
        let mut session = Session::new(None);
        assert_eq!(execute(&mut session, "exit").unwrap(), Outcome::Exit);
        output(&mut session, "add Alice");

        assert!(matches!(
            execute(&mut session, "exit"),
            Err(Error::UnsavedChanges)
        ));
        assert_eq!(
            execute(&mut session, "quit --force").unwrap(),
            Outcome::Exit
        );
    }

    #[test]
    fn undo_is_an_unsaved_change_too() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = Session::new(Some(directory.path().join("cup.json")));
        output(&mut session, "add Alice");
        output(&mut session, "save");

        output(&mut session, "undo");

        assert!(session.has_unsaved_changes());
    }

    #[test]
    fn a_file_that_is_no_competition_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.json");
        fs::write(&path, "{}").unwrap();

        assert!(matches!(Session::load(path), Err(Error::Load { .. })));
    }

    #[test]
    fn a_table_aligns_its_columns_under_an_underlined_header() {
        let rows = [
            vec!["1".to_owned(), "Alice".to_owned(), "3".to_owned()],
            vec!["10".to_owned(), "Bo".to_owned(), "0".to_owned()],
        ];

        assert_eq!(
            table(&["#", "name", "points"], &rows),
            "#   name   points\n--  -----  ------\n1   Alice  3\n10  Bo     0"
        );
    }
}
