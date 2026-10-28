//! A competition being run at the prompt, from the first command typed to the last.
//!
//! A [`Session`] owns the [`CompetitionStateMachine`] of the competition and executes every
//! [`Command`] against it. It is saved as that machine's transition log rather than as a snapshot
//! of the competition, next to the [`People`] the entrants are, which the log knows by id only.
//! What can be undone and redone is what the session did itself, which is not saved: a loaded
//! competition starts with nothing to undo.

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    io,
    num::{NonZeroU32, NonZeroUsize},
};

use plugin_factory::{
    catalog::{AnyAxis, PluginEntry},
    config::{Config, ConfigError, Path as ConfigPath, PluginConfig, Value},
    draft::{Draft, Issue},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tournament_app::file::Format;
use tournament_engine::{
    AddEntrant, AddPhase, Annotated as _, Competition, CompetitionStateMachine,
    CompetitionStateMachineExt as _, Entrant, EntrantId, Match, MatchId, MatchResult, Phase,
    PhaseData, PhaseId, PhasePlugins, RankingId, RemoveEntrant, ReportMatchResult, Seed,
    SetCompetitionSeed, StartCompetition, TypeMap,
    plugins::{metrics_calculator::MetricColumn, phase_format::PhaseFormat, ranker::SliceRanker},
    transitions::{ReconfigurePhaseError, UndoMatchResultError},
    undoable_state_machine::{
        RedoLastError, SessionHistory, StateMachineError, TransitionError, UndoLastError,
    },
    uuid::Uuid,
};

use crate::{
    command::{Command, ReportedResult},
    people::{People, Person},
    questionnaire::{self, AcceptDefaults, IssueList},
    storage::Storage,
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
    /// A session never reads from the terminal, so a phase that is not given by its format has to
    /// be configured before it reaches one, see [`questionnaire::fill`].
    #[error("configuring a phase needs a terminal to ask at; name a format, see `help add-phase`")]
    NotConfigured,
    /// A session never reads from the terminal, so the phase has to be edited before it reaches
    /// one, see [`Session::edit_phase`].
    #[error("editing the phase needs a terminal to ask at")]
    NotEdited,
    #[error("the competition has no phase yet")]
    NoPhase,
    /// The edited phase does not fit a change made after the phase was added, e.g. a result
    /// reported for a match the edited format does not schedule.
    #[error("the phase cannot be changed this way, since a later change no longer fits it: {0}")]
    EditRejected(TransitionError),
    /// No registered phase format goes by this name.
    #[error(
        "there is no format {0:?}; the formats are: {formats}",
        formats = format_names().join(", ")
    )]
    UnknownFormat(String),
    /// An answer to a field of a format is not written as `FIELD=VALUE`.
    #[error("{0:?} is no answer; write it as FIELD=VALUE")]
    NotAnAnswer(String),
    /// The format has no field of this name.
    #[error("{format} has no field {field:?}")]
    UnknownField { format: &'static str, field: String },
    /// The value given for a field is not one it takes.
    #[error("{value:?} does not answer {field:?}")]
    InvalidAnswer { field: String, value: String },
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
    #[error("the competition has not been saved yet, so a name to save it under is needed")]
    NoName,
    #[error("there are unsaved changes; save them, or exit with --force to discard them")]
    UnsavedChanges,
    /// The competition does not accept the change in its current state, e.g. an entrant added
    /// after the start.
    #[error("{0}")]
    Rejected(TransitionError),
    /// A phase given by its format leaves something unanswered that offers no answer, see
    /// [`AcceptDefaults`].
    #[error("the phase cannot be added: {0}")]
    Unanswered(#[from] questionnaire::Error),
    /// No phase can be built from its draft: its plugins do not fit together, or miss something no
    /// implied plugin provides.
    #[error("the phase cannot be played this way:{}", IssueList(.0))]
    InvalidPhase(Vec<Issue>),
    /// A plugin added to a phase does not fit its configuration.
    #[error(transparent)]
    Configuration(#[from] ConfigError),
    #[error(transparent)]
    StateMachine(StateMachineError),
    #[error("cannot read {name}: {source}")]
    Read { name: String, source: io::Error },
    #[error("{name} is not a competition saved by this CLI: {source}")]
    Load {
        name: String,
        source: serde_json::Error,
    },
    #[error("cannot serialize the competition: {0}")]
    Serialize(serde_json::Error),
    #[error("cannot save {name}: {source}")]
    Write { name: String, source: io::Error },
}

impl From<StateMachineError> for Error {
    fn from(error: StateMachineError) -> Self {
        match error {
            StateMachineError::Transition(_, error) => Self::Rejected(error),
            error => Self::StateMachine(error),
        }
    }
}

/// A competition being run at the prompt, together with the storage it is saved to and the name it
/// is saved under.
///
/// The first entry of the log of every competition is the [`SetCompetitionSeed`] it was created
/// with, which is no action of the session and so never undone.
pub struct Session {
    machine: CompetitionStateMachine,
    /// Everyone ever entered in the log, removed entrants included, since undoing their removal
    /// brings them back.
    people: People,
    /// What this session did, which `undo` and `redo` walk through.
    history: SessionHistory,
    storage: Box<dyn Storage>,
    name: Option<String>,
    /// The length of the log when it was last saved or loaded. The log is append-only, so every
    /// change since makes it longer, undo and redo included.
    saved_log_len: usize,
}

impl Session {
    /// Creates a session for a new competition with a random seed, saved to `storage`. It has no
    /// name yet, see [`Session::named`].
    pub fn new(storage: impl Storage + 'static) -> Self {
        let mut machine = CompetitionStateMachine::default();
        machine
            .apply_transition(SetCompetitionSeed::new(Seed::new(rand::random())))
            .expect("a new competition must accept a seed");
        Self {
            saved_log_len: machine.log().len(),
            machine,
            people: People::default(),
            history: SessionHistory::new(),
            storage: Box::new(storage),
            name: None,
        }
    }

    /// This session, saving its competition under `name` once it is saved without one.
    pub fn named(self, name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..self
        }
    }

    /// Loads the competition saved in `storage` under `name`, which also becomes the name it is
    /// saved under. The session has done nothing yet, so there is nothing to undo.
    ///
    /// # Errors
    /// Returns an error if nothing can be read under `name`, or what is read is no log that
    /// replays into a competition.
    pub fn load(storage: impl Storage + 'static, name: impl Into<String>) -> Result<Self, Error> {
        let name = name.into();
        let bytes = match storage.read(&name) {
            Ok(bytes) => bytes,
            Err(source) => return Err(Error::Read { name, source }),
        };
        let saved: Result<Saved<CompetitionStateMachine, People>, _> =
            serde_json::from_slice(&bytes);
        let Saved {
            format: _,
            log: machine,
            people,
        } = match saved {
            Ok(saved) => saved,
            Err(source) => return Err(Error::Load { name, source }),
        };
        Ok(Self {
            saved_log_len: machine.log().len(),
            machine,
            people,
            history: SessionHistory::new(),
            storage: Box::new(storage),
            name: Some(name),
        })
    }

    pub fn competition(&self) -> &Competition {
        self.machine.current()
    }

    /// The name the competition is saved under unless a save is given another one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
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
            Command::AddPhase { format, answers } => {
                self.add_phase_in(&format.ok_or(Error::NotConfigured)?, &answers)?
            }
            Command::EditPhase => return Err(Error::NotEdited),
            Command::Start => self.start()?,
            Command::Matches { open, round } => self.matches(open, round)?,
            Command::Report { number, result } => self.report(number, result)?,
            Command::UndoReport { number } => self.undo_report(number)?,
            Command::Ranking => self.ranking()?,
            Command::Undo => self.undo()?,
            Command::Redo => self.redo()?,
            Command::Save { name } => self.save(name)?,
            Command::Exit { force } => {
                if self.has_unsaved_changes() && !force {
                    return Err(Error::UnsavedChanges);
                }
                return Ok(Outcome::Exit);
            }
        };
        Ok(Outcome::Continue(output))
    }

    fn entrants(&self) -> String {
        let entrants = self.competition().entrants();
        if entrants.is_empty() {
            return "there are no entrants yet".to_owned();
        }
        entrants
            .keys()
            .map(|id| self.name_of(id))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn entrant_named(&self, name: &str) -> Option<&Entrant> {
        self.competition()
            .entrants()
            .values()
            .find(|entrant| self.name_of(&entrant.id()) == name)
    }

    /// The name the entrant `id` is shown with. Every entrant this CLI adds has one, so the
    /// fallback is only for a log written by someone else.
    fn name_of(&self, id: &EntrantId) -> &str {
        self.people.name(id).unwrap_or("<unnamed>")
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
            let id = EntrantId::new(Uuid::new_v4());
            self.machine
                .apply_in(&mut self.history, AddEntrant::new(Entrant::new(id)))?;
            self.people.add(Person::new(id, name.as_str()));
        }
        Ok(format!(
            "{} entrant(s) added, {} in total",
            names.len(),
            self.competition().entrants().len()
        ))
    }

    fn remove_entrant(&mut self, name: &str) -> Result<String, Error> {
        let entrant_id = self
            .entrant_named(name)
            .ok_or_else(|| Error::UnknownEntrant(name.to_owned()))?
            .id();
        self.machine
            .apply_in(&mut self.history, RemoveEntrant::new(entrant_id))?;
        Ok(format!("{name} removed"))
    }

    /// Adds the phase `draft` configures, which is the only phase the competition is played in.
    /// `draft` is a [`Competition::phase_draft`] of this session's competition.
    ///
    /// A calculator that has nothing to be configured is added for a metric the phase needs and
    /// nothing calculates, see [`Draft::add_implied`]; one with fields is never added unasked.
    /// `draft` itself is left as it is, so that it can be fixed and added again.
    ///
    /// # Errors
    /// Returns an error if the competition already has a phase, does not accept one anymore
    /// because it has started, or if the plugins `draft` configures do not fit together, e.g.
    /// [`Error::InvalidPhase`].
    pub fn add_phase(&mut self, draft: &Draft<PhasePlugins>) -> Result<String, Error> {
        let (plugins, summary) = completed(draft)?;
        self.machine.apply_in(
            &mut self.history,
            AddPhase::new(PhaseId::new(Uuid::new_v4()), plugins),
        )?;
        Ok(format!("phase added: {summary}"))
    }

    /// A draft of the phase of the competition holding the configuration it is played with, see
    /// [`Phase::config`], to be changed and handed to [`Self::edit_phase`].
    ///
    /// # Errors
    /// Returns [`Error::NoPhase`] if the competition has no phase.
    pub fn phase_draft_to_edit(&self) -> Result<Draft<PhasePlugins>, Error> {
        let config = self.added_phase()?.config();
        Ok(self.competition().phase_draft().with_answers(&config)?)
    }

    /// Plays the phase of the competition as `draft` configures it, as if it had been added that
    /// way, see [`reconfigure_phase`]. `draft` is completed, and left as it is, as by
    /// [`Self::add_phase`].
    ///
    /// [`reconfigure_phase`]: tournament_engine::CompetitionStateMachineExt::reconfigure_phase
    ///
    /// # Errors
    /// Returns [`Error::NoPhase`] if the competition has no phase, [`Error::InvalidPhase`] if the
    /// plugins `draft` configures do not fit together, and [`Error::EditRejected`] if a change
    /// made after the phase was added, e.g. a reported result, does not fit the edited phase.
    pub fn edit_phase(&mut self, draft: &Draft<PhasePlugins>) -> Result<String, Error> {
        let phase_id = self.added_phase()?.id();
        let (plugins, summary) = completed(draft)?;
        let edited = self
            .machine
            .reconfigure_phase(&mut self.history, phase_id, plugins)
            .map_err(|error| match error {
                ReconfigurePhaseError::Rejected(StateMachineError::Transition(_, error)) => {
                    Error::EditRejected(error)
                }
                ReconfigurePhaseError::Rejected(error) => Error::StateMachine(error),
                ReconfigurePhaseError::NotAdded => {
                    unreachable!("the phase of a session is added by an `AddPhase` in effect")
                }
            })?;
        Ok(match edited {
            Some(_) => format!("phase changed: {summary}"),
            None => "nothing changed".to_owned(),
        })
    }

    /// The phase of the competition, whether started or not.
    fn added_phase(&self) -> Result<&Phase, Error> {
        self.competition()
            .phases()
            .values()
            .next()
            .ok_or(Error::NoPhase)
    }

    /// Adds the phase played in the format named `format`, with its fields answered by `answers`,
    /// each written as `FIELD=VALUE`, and every other part answered as offered, see
    /// [`AcceptDefaults`].
    fn add_phase_in(&mut self, format: &str, answers: &[String]) -> Result<String, Error> {
        let entry = AnyAxis::of::<dyn PhaseFormat>()
            .entries()
            .find(|entry| entry.id() == format || format_name(entry).eq_ignore_ascii_case(format))
            .ok_or_else(|| Error::UnknownFormat(format.to_owned()))?;
        let schema = entry.schema();
        let mut config = PluginConfig::new(entry.id());
        for answer in answers {
            let (field, value) = answer
                .split_once('=')
                .ok_or_else(|| Error::NotAnAnswer(answer.clone()))?;
            let kind = schema
                .field_named(field)
                .ok_or_else(|| Error::UnknownField {
                    format: entry.label(),
                    field: field.to_owned(),
                })?
                .kind();
            let value =
                questionnaire::parse_answer(kind, value).ok_or_else(|| Error::InvalidAnswer {
                    field: field.to_owned(),
                    value: value.to_owned(),
                })?;
            config = config.with(field, value);
        }
        let mut draft = self.competition().phase_draft();
        draft.set(&ConfigPath::ROOT.field("format"), config)?;
        questionnaire::fill(&mut draft, &mut AcceptDefaults)?;
        self.add_phase(&draft)
    }

    fn start(&mut self) -> Result<String, Error> {
        self.machine.apply_in(&mut self.history, StartCompetition)?;
        Ok(format!(
            "competition started, {} match(es) scheduled",
            self.phase()?.data().matches().len()
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
        let format = self.phase()?.format();
        let stage_of = |entity: &TypeMap| format.stage(entity).map(|stage| stage.to_string());
        let phase = self.phase()?.data();
        // only a format that names its stages gets a column for them
        let has_stages = phase
            .matches()
            .values()
            .any(|r#match| stage_of(r#match.annotations()).is_some());
        let mut listed: Vec<&Match> = phase
            .matches()
            .values()
            .filter(|r#match| !(only_open && r#match.is_finished()))
            .filter(|r#match| only_round.is_none_or(|round| r#match.round() == round))
            .collect();
        // by number, which a bracket gives its matches before it schedules them
        listed.sort_by_key(|r#match| r#match.number());
        let rows: Vec<_> = listed
            .into_iter()
            .map(|r#match| {
                [r#match.number().to_string(), r#match.round().to_string()]
                    .into_iter()
                    .chain(has_stages.then(|| stage_of(r#match.annotations()).unwrap_or_default()))
                    .chain([
                        self.team_name(phase, r#match.team_a()),
                        "vs".to_owned(),
                        self.team_name(phase, r#match.team_b()),
                        self.result_text(phase, r#match),
                    ])
                    .collect()
            })
            .collect();
        // a bye is named by its stage only if its round has several, e.g. one of the loser
        // bracket in a round of the winner bracket
        let mut stages_of_round: HashMap<NonZeroU32, HashSet<Option<String>>> = HashMap::new();
        let all_stages = (phase.matches().values())
            .map(|r#match| (r#match.round(), stage_of(r#match.annotations())))
            .chain((phase.byes().values()).map(|bye| (bye.round(), stage_of(bye.annotations()))));
        for (round, stage) in all_stages {
            stages_of_round.entry(round).or_default().insert(stage);
        }
        // a bye is never open: it has nothing to report
        let byes: Vec<_> = phase
            .byes()
            .values()
            .filter(|_| !only_open)
            .filter(|bye| only_round.is_none_or(|round| bye.round() == round))
            .map(|bye| {
                let stage = (stages_of_round[&bye.round()].len() > 1)
                    .then(|| stage_of(bye.annotations()))
                    .flatten();
                (bye, stage)
            })
            .collect();
        let byes_have_stages = byes.iter().any(|(_, stage)| stage.is_some());
        let bye_rows: Vec<_> = byes
            .into_iter()
            .map(|(bye, stage)| {
                [bye.round().to_string()]
                    .into_iter()
                    .chain(byes_have_stages.then(|| stage.unwrap_or_default()))
                    .chain([self.team_name(phase, &[bye.ranking()])])
                    .collect()
            })
            .collect();
        let bye_header = ["round"]
            .into_iter()
            .chain(byes_have_stages.then_some("stage"))
            .chain(["bye"])
            .collect::<Vec<_>>();
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
            (!bye_rows.is_empty()).then(|| table(&bye_header, &bye_rows)),
        ];
        Ok(tables
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n"))
    }

    /// The phase of the match numbered `number` (see [`Match::number`]), as [`Self::matches`]
    /// lists it, and that match.
    ///
    /// # Errors
    /// Returns an error if the competition has not started or has no such match.
    fn match_numbered(&self, number: NonZeroUsize) -> Result<(PhaseId, MatchId), Error> {
        let phase = self.phase()?;
        let match_id = phase
            .data()
            .matches()
            .values()
            .find(|r#match| usize::try_from(r#match.number().get()) == Ok(number.get()))
            .map(Match::id)
            .ok_or(Error::UnknownMatch(number))?;
        Ok((phase.id(), match_id))
    }

    fn report(&mut self, number: NonZeroUsize, result: ReportedResult) -> Result<String, Error> {
        let (phase_id, match_id) = self.match_numbered(number)?;
        let matches_before = self.phase()?.data().matches().len();
        let result = match result {
            ReportedResult::A => MatchResult::AWins,
            ReportedResult::B => MatchResult::BWins,
            ReportedResult::Draw => MatchResult::Draw,
        };
        self.machine.apply_in(
            &mut self.history,
            ReportMatchResult::new(phase_id, match_id, result),
        )?;

        let phase = self.phase()?.data();
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
        let matches_before = self.phase()?.data().matches().len();
        self.machine
            .undo_match_result(&mut self.history, phase_id, match_id)
            .map_err(|error| match error {
                UndoMatchResultError::NotReported => Error::NotReported(number),
                UndoMatchResultError::Rejected(_) => Error::ReportDependedOn(number),
            })?;

        let phase = self.phase()?.data();
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
        let columns: Vec<_> = phase
            .metrics_calculators()
            .iter()
            .filter_map(|calculator| calculator.column())
            .collect();
        let phase = phase.data();
        let header: Vec<_> = ["#", "name"]
            .into_iter()
            .chain(columns.iter().map(MetricColumn::header))
            .collect();
        let rows: Vec<_> = phase
            .standings()
            .map(|ranking| {
                [
                    ranking.rank().to_string(),
                    self.team_name(phase, &[ranking.id()]),
                ]
                .into_iter()
                // a column of a metric its calculator does not calculate is left empty
                .chain(columns.iter().map(|column| {
                    column
                        .cell(ranking)
                        .map(|cell| cell.to_string())
                        .unwrap_or_default()
                }))
                .collect()
            })
            .collect();
        Ok(table(&header, &rows))
    }

    /// Undoes the latest change of this session that is still in effect, which may be an
    /// `undo-report`, bringing the result back.
    fn undo(&mut self) -> Result<String, Error> {
        self.history
            .undo_last(&mut self.machine)
            .map_err(|error| match error {
                UndoLastError::NothingToUndo => Error::NothingToUndo,
                UndoLastError::Rejected(error) => error.into(),
                UndoLastError::UndoneElsewhere => {
                    unreachable!("the session is the only one appending to its log")
                }
            })?;
        Ok("undone".to_owned())
    }

    /// Redoes the change this session undid last, as long as it made no other change since.
    fn redo(&mut self) -> Result<String, Error> {
        self.history
            .redo_last(&mut self.machine)
            .map_err(|error| match error {
                RedoLastError::NothingToRedo => Error::NothingToRedo,
                RedoLastError::Rejected(error) => error.into(),
                RedoLastError::RedoneElsewhere => {
                    unreachable!("the session is the only one appending to its log")
                }
            })?;
        Ok("redone".to_owned())
    }

    fn save(&mut self, name: Option<String>) -> Result<String, Error> {
        let name = name.or_else(|| self.name.clone()).ok_or(Error::NoName)?;
        let saved = Saved {
            format: Format::CURRENT,
            log: &self.machine,
            people: &self.people,
        };
        let json = serde_json::to_vec_pretty(&saved).map_err(Error::Serialize)?;
        if let Err(source) = self.storage.write(&name, &json) {
            return Err(Error::Write { name, source });
        }
        let output = format!("saved to {name}");
        self.name = Some(name);
        self.saved_log_len = self.machine.log().len();
        Ok(output)
    }

    fn team_name(&self, phase: &PhaseData, rankings: &[RankingId]) -> String {
        rankings
            .iter()
            .flat_map(|id| phase.rankings()[id].entrants())
            .map(|id| self.name_of(id))
            .collect::<Vec<_>>()
            .join(" & ")
    }

    fn result_text(&self, phase: &PhaseData, r#match: &Match) -> String {
        let a = self.team_name(phase, r#match.team_a());
        let b = self.team_name(phase, r#match.team_b());
        match r#match.result() {
            Some(MatchResult::AWins) => format!("{a} beat {b}"),
            Some(MatchResult::BWins) => format!("{b} beat {a}"),
            Some(MatchResult::Draw) => "draw".to_owned(),
            None => "open".to_owned(),
        }
    }
}

/// The plugins `draft`, a phase draft, configures with the plugins its answers imply added, see
/// [`Draft::add_implied`], together with their [`summary`].
///
/// # Errors
/// Returns [`Error::InvalidPhase`] if no phase can be built from them.
fn completed(draft: &Draft<PhasePlugins>) -> Result<(PhasePlugins, String), Error> {
    let mut draft = draft.clone();
    draft.add_implied();
    let plugins = draft.build().map_err(Error::InvalidPhase)?;
    Ok((plugins, summary(draft.answers())))
}

/// The name `add-phase` takes the format of `entry` by: its label with hyphens for spaces, e.g.
/// `round-robin`.
fn format_name(entry: &PluginEntry) -> String {
    entry.label().replace(' ', "-").to_lowercase()
}

/// The names of all formats, see [`format_name`].
fn format_names() -> Vec<String> {
    AnyAxis::of::<dyn PhaseFormat>()
        .entries()
        .map(format_name)
        .collect()
}

/// Names the format and the ranking `answers`, the answers of a phase draft, configure in
/// one line, the way a phase that was just added is reported, e.g. "round robin, ranked by more
/// points, then more Sonneborn-Berger".
fn summary(answers: &Config) -> String {
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

/// What a competition is saved as: the [`Format`] of the file, the transition log of its machine,
/// and the people its entrants are, apart from the log.
#[derive(Serialize, Deserialize)]
struct Saved<L, P> {
    // first, so that a file of a newer format is refused for that before its log is read
    #[serde(default)]
    format: Format,
    log: L,
    people: P,
}

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
    use plugin_factory::{PluginType, draft::Problem};
    use tournament_engine::plugins::{
        metrics_calculator::{
            FewerMatchesPlayed, MorePoints, MoreWins, PointsCalculator, SonnebornBergerCalculator,
            WinsDrawsLossesCalculator,
        },
        phase_format::{MonsterDyp, RoundRobin, SingleElimination},
    };
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;
    use crate::storage::MemoryStorage;

    /// The header and its underline, which every table shown by a command starts with.
    const HEADER_LINES: usize = 2;

    /// A draft of a phase of the competition of `session` played in `format`, ranked by `ranking`
    /// and calculating the metrics of `metrics`.
    fn draft(
        session: &Session,
        format: PluginConfig,
        ranking: Vec<PluginConfig>,
        metrics: Vec<PluginConfig>,
    ) -> Draft<PhasePlugins> {
        session
            .competition()
            .phase_draft()
            .with("format", format)
            .and_then(|draft| draft.with("ranking", ranking))
            .and_then(|draft| draft.with("metrics", metrics))
            .unwrap()
    }

    /// The plugin `P` with nothing answered.
    fn plugin<P: PluginType>() -> PluginConfig {
        PluginConfig::of::<P>()
    }

    /// The scoring system of chess: a win is worth a point, a draw half of one, a bye nothing.
    fn chess_points() -> PluginConfig {
        plugin::<PointsCalculator>()
            .with("points_for_win", 1.0)
            .with("points_for_draw", 0.5)
            .with("points_for_bye", 0.0)
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
            .keys()
            .map(|id| session.name_of(id))
            .collect()
    }

    /// A session with a started competition between Alice, Bob, Carol and Dave in a phase of
    /// `preset`.
    fn started(preset: &str) -> Session {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");
        output(&mut session, &format!("add-phase {preset}"));
        output(&mut session, "start");
        session
    }

    #[test]
    fn entrants_are_listed_by_name() {
        let mut session = Session::new(MemoryStorage::default());

        output(&mut session, "add Alice 'Bob B.'");

        assert_eq!(output(&mut session, "entrants"), "Alice\nBob B.");
    }

    #[test]
    fn a_duplicate_name_adds_none_of_the_entrants() {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice");

        let result = execute(&mut session, "add Bob Alice");

        assert!(matches!(result, Err(Error::DuplicateName(name)) if name == "Alice"));
        assert_eq!(names(&session), ["Alice"]);
    }

    #[test]
    fn a_name_cannot_be_given_twice_in_one_command() {
        let mut session = Session::new(MemoryStorage::default());

        let result = execute(&mut session, "add Bob Bob");

        assert!(matches!(result, Err(Error::DuplicateName(_))));
        assert!(names(&session).is_empty());
    }

    #[test]
    fn an_entrant_is_removed_by_name() {
        let mut session = Session::new(MemoryStorage::default());
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
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add-phase round-robin");

        let result = execute(&mut session, "add-phase single-elimination");

        let Err(Error::Rejected(error)) = &result else {
            panic!("got {result:?}");
        };
        assert!(error.to_string().contains("already has a phase"), "{error}");
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
                "single-elimination third_place_match=no",
                "single elimination (match for third place: no), ranked by later first loss",
            ),
            (
                "double-elimination",
                "double elimination (grand final reset: yes), ranked by later elimination, then \
                 fewer losses",
            ),
            (
                "random-rounds rounds=3",
                "random rounds (rounds: 3), ranked by more points, then more Buchholz, then more \
                 Sonneborn-Berger, then more Buchholz of Buchholz",
            ),
            (
                "monster-dyp",
                "Monster DYP (rounds: 5), ranked by more points",
            ),
            (
                "tournament_engine::monster_dyp rounds=4",
                "Monster DYP (rounds: 4), ranked by more points",
            ),
        ];

        for (preset, summary) in presets {
            let mut session = Session::new(MemoryStorage::default());

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

    /// A format or an answer the session cannot make sense of adds no phase, and says why.
    #[test]
    fn an_unknown_format_or_answer_is_rejected() {
        let mut session = Session::new(MemoryStorage::default());

        for (line, message) in [
            ("add-phase swiss", "there is no format \"swiss\""),
            ("add-phase random-rounds rounds", "\"rounds\" is no answer"),
            (
                "add-phase random-rounds turns=3",
                "random rounds has no field \"turns\"",
            ),
            (
                "add-phase random-rounds rounds=many",
                "\"many\" does not answer \"rounds\"",
            ),
        ] {
            let error = execute(&mut session, line).unwrap_err().to_string();
            assert!(error.starts_with(message), "{line}: {error}");
        }
        assert!(session.competition().phases().is_empty());
    }

    /// A session executes commands and never asks the operator anything, so the phase of an
    /// `add-phase` without a format has to be configured before the command reaches it.
    #[test]
    fn a_phase_to_be_configured_cannot_be_added_by_the_session_itself() {
        let mut session = Session::new(MemoryStorage::default());

        let result = execute(&mut session, "add-phase");

        assert!(matches!(result, Err(Error::NotConfigured)));
        assert!(session.competition().phases().is_empty());
    }

    /// An edit starts out at the configuration the phase is played with, so changing the ranking
    /// keeps the metrics, and the results reported so far stay.
    #[test]
    fn a_phase_is_edited_starting_out_at_its_configuration() {
        let mut session = started("round-robin");
        output(&mut session, "report 1 a");
        let result = output(&mut session, "matches")
            .lines()
            .nth(HEADER_LINES)
            .unwrap()
            .to_owned();

        let mut draft = session.phase_draft_to_edit().unwrap();
        draft
            .set(
                &ConfigPath::ROOT.field("ranking"),
                vec![plugin::<MoreWins>()],
            )
            .unwrap();
        let edited = session.edit_phase(&draft).unwrap();

        assert_eq!(edited, "phase changed: round robin, ranked by more wins");
        assert_eq!(
            output(&mut session, "matches").lines().nth(HEADER_LINES),
            Some(result.as_str())
        );
        assert_eq!(
            output(&mut session, "ranking")
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["#", "name", "W-D-L", "points", "Sonneborn-Berger"]
        );
    }

    #[test]
    fn a_phase_edited_without_a_change_is_left_as_it_is() {
        let mut session = started("round-robin");
        let log_len = session.machine.log().len();

        let draft = session.phase_draft_to_edit().unwrap();

        assert_eq!(session.edit_phase(&draft).unwrap(), "nothing changed");
        assert_eq!(session.machine.log().len(), log_len);
    }

    /// The teams of a Monster DYP are pairs, so it never schedules the match of single entrants
    /// whose result was reported.
    #[test]
    fn an_edit_a_reported_result_does_not_fit_is_rejected() {
        let mut session = started("round-robin");
        output(&mut session, "report 1 a");
        let mut draft = session.phase_draft_to_edit().unwrap();
        draft
            .set(
                &ConfigPath::ROOT.field("format"),
                plugin::<MonsterDyp>().with("rounds", 3),
            )
            .unwrap();

        let result = session.edit_phase(&draft);

        assert!(matches!(result, Err(Error::EditRejected(_))), "{result:?}");
        assert_eq!(
            format!("{:?}", session.phase().unwrap().format()),
            "RoundRobin",
            "the phase must stay as it was"
        );
    }

    #[test]
    fn an_edit_is_undone_like_any_change() {
        let mut session = started("round-robin");
        let before = session.phase_draft_to_edit().unwrap().answers().clone();
        let mut draft = session.phase_draft_to_edit().unwrap();
        draft
            .set(
                &ConfigPath::ROOT.field("ranking"),
                vec![plugin::<MoreWins>()],
            )
            .unwrap();
        session.edit_phase(&draft).unwrap();

        output(&mut session, "undo");

        assert_eq!(session.phase_draft_to_edit().unwrap().answers(), &before);
    }

    #[test]
    fn only_a_phase_that_was_added_can_be_edited() {
        let mut session = Session::new(MemoryStorage::default());

        assert!(matches!(session.phase_draft_to_edit(), Err(Error::NoPhase)));
        assert!(matches!(
            execute(&mut session, "edit-phase"),
            Err(Error::NotEdited)
        ));
    }

    /// A configured phase calculates exactly the metrics it was configured with, and scores them
    /// the way it was configured to.
    #[test]
    fn a_configured_phase_is_played_the_way_it_was_configured() {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");

        let draft = draft(
            &session,
            plugin::<RoundRobin>(),
            vec![plugin::<MorePoints>()],
            vec![plugin::<WinsDrawsLossesCalculator>(), chess_points()],
        );
        let added = session.add_phase(&draft).unwrap();

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
    /// results the points are scored from, which have nothing to be configured.
    #[test]
    fn a_phase_calculates_the_metrics_its_own_are_derived_from() {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");
        let sonneborn_berger = plugin::<SonnebornBergerCalculator>()
            .with("weight_for_win", 1.0)
            .with("weight_for_draw", 0.5);
        let draft = draft(
            &session,
            plugin::<RoundRobin>(),
            Vec::new(),
            vec![chess_points(), sonneborn_berger],
        );
        session.add_phase(&draft).unwrap();
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

    /// The points can be scored in many ways, so a phase ranked by them without a points
    /// calculator is not added with one the operator was not asked about.
    #[test]
    fn a_calculator_that_has_to_be_configured_is_not_added_unasked() {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");

        let draft = draft(
            &session,
            plugin::<RoundRobin>(),
            vec![plugin::<MorePoints>()],
            Vec::new(),
        );

        let result = session.add_phase(&draft);

        let Err(Error::InvalidPhase(issues)) = result else {
            panic!("got {result:?}");
        };
        assert!(
            issues.iter().any(|issue| matches!(
                issue.problem(),
                Problem::Unprovided { providers, .. } if providers.contains(&PointsCalculator::ID)
            )),
            "got {issues:?}"
        );
        assert!(session.competition().phases().is_empty());
    }

    /// Wins, draws, losses and the matches they add up to are four criteria of a single metric, so
    /// a phase can be ranked by the wins alone, without calculating anything besides that metric.
    #[test]
    fn a_phase_can_be_ranked_by_wins_alone() {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");
        let draft = draft(
            &session,
            plugin::<RoundRobin>(),
            vec![plugin::<MoreWins>()],
            Vec::new(),
        );
        let added = session.add_phase(&draft).unwrap();

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
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");
        // a match for third place would have every entrant play two matches, which is exactly the
        // tie this criterion is shown by breaking
        let draft = draft(
            &session,
            plugin::<SingleElimination>().with("third_place_match", false),
            vec![plugin::<FewerMatchesPlayed>()],
            Vec::new(),
        );
        session.add_phase(&draft).unwrap();
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
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add Alice Bob Carol Dave");
        let draft = draft(
            &session,
            plugin::<RoundRobin>(),
            Vec::new(),
            vec![plugin::<WinsDrawsLossesCalculator>()],
        );
        let added = session.add_phase(&draft).unwrap();
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
        let mut session = Session::new(MemoryStorage::default());

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

    /// A round robin of three has one entrant sit out every round, which is listed after the
    /// matches but not among the open ones.
    #[test]
    fn byes_are_listed_after_the_matches() {
        let mut session = Session::new(MemoryStorage::default());
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

    /// Of six entrants in a double elimination, the two that lose in the first round of the
    /// winner bracket sit out the first round of the loser bracket, in the round of the second
    /// round of the winner bracket.
    #[test]
    fn a_bye_is_listed_with_its_stage_where_its_round_has_several() {
        let mut session = Session::new(MemoryStorage::default());
        output(&mut session, "add A B C D E F");
        output(&mut session, "add-phase double-elimination");
        output(&mut session, "start");
        let bye_header = |session: &mut Session, round| {
            let listing = output(session, &format!("matches --round {round}"));
            let (_, byes) = listing.split_once("\n\n").expect("two tables");
            byes.lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            bye_header(&mut session, 1),
            "round bye",
            "every match and bye of the first round is of the winner bracket"
        );

        output(&mut session, "report 1 a");
        output(&mut session, "report 2 a");

        let listing = output(&mut session, "matches --round 2");
        let (_, byes) = listing.split_once("\n\n").expect("two tables");
        let byes: Vec<String> = byes
            .lines()
            .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(byes[0], "round stage bye", "{listing}");
        assert_eq!(byes.len(), HEADER_LINES + 2, "{listing}");
        assert!(
            byes[HEADER_LINES..]
                .iter()
                .all(|row| row.starts_with("2 Losers round 1 ")),
            "{listing}"
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
        let mut session = started("single-elimination third_place_match=no");

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
        let mut session = Session::new(MemoryStorage::default());
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
        let mut session = Session::new(MemoryStorage::default());
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
        let mut session = Session::new(MemoryStorage::default());

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

    /// What can be undone is what a session did itself, so a loaded competition has the state it
    /// was saved with but nothing to undo.
    #[test]
    fn a_saved_competition_loads_without_anything_to_undo() {
        let storage = MemoryStorage::default();
        let mut session = Session::new(storage.clone());
        output(&mut session, "add Alice Bob Carol Dave");
        output(&mut session, "add-phase round-robin");
        output(&mut session, "start");
        output(&mut session, "report 1 a");
        let ranking = output(&mut session, "ranking");

        output(&mut session, "save cup");
        let mut loaded = Session::load(storage, "cup").unwrap();

        assert_eq!(loaded.name(), Some("cup"));
        assert!(!loaded.has_unsaved_changes());
        assert_eq!(output(&mut loaded, "ranking"), ranking);
        assert_eq!(
            output(&mut loaded, "matches"),
            output(&mut session, "matches")
        );
        assert!(matches!(
            execute(&mut loaded, "undo"),
            Err(Error::NothingToUndo)
        ));
        // a single result can still be undone, as it is found in the log
        output(&mut loaded, "undo-report 1");
    }

    /// `undo-report` is a change like any other, so `undo` brings the result back rather than
    /// undoing the change made before it.
    #[test]
    fn undo_brings_back_a_result_undone_by_undo_report() {
        let mut session = started("round-robin");
        output(&mut session, "report 1 a");
        output(&mut session, "report 2 a");
        let reported = output(&mut session, "matches");
        output(&mut session, "undo-report 1");

        output(&mut session, "undo");

        assert_eq!(output(&mut session, "matches"), reported);
    }

    #[test]
    fn a_save_without_a_name_goes_under_the_last_one() {
        let storage = MemoryStorage::default();
        let mut session = Session::new(storage.clone()).named("cup");
        output(&mut session, "add Alice");

        output(&mut session, "save");
        output(&mut session, "add Bob");
        output(&mut session, "save");

        assert_eq!(
            names(&Session::load(storage, "cup").unwrap()),
            ["Alice", "Bob"]
        );
    }

    /// The log knows the entrants by id only, so that their names can be deleted without touching
    /// the log.
    #[test]
    fn names_are_saved_apart_from_the_log() {
        let storage = MemoryStorage::default();
        let mut session = Session::new(storage.clone()).named("cup");
        output(&mut session, "add Alice");

        output(&mut session, "save");

        let saved: Value = serde_json::from_slice(&storage.read("cup").unwrap()).unwrap();
        assert!(!saved["log"].to_string().contains("Alice"));
        assert_eq!(saved["people"][0]["name"], "Alice");
    }

    /// A competition moves between the CLI and the GUI as a file, which the CLI saves without the
    /// name the GUI adds.
    #[test]
    fn a_file_of_the_gui_is_loaded_and_the_gui_imports_what_is_saved() {
        let mut storage = MemoryStorage::default();
        let mut exported = tournament_app::session::Session::new("cup");
        exported.add_entrant("Alice").unwrap();
        storage.write("cup", exported.export().as_bytes()).unwrap();

        let mut session = Session::load(storage.clone(), "cup").unwrap();
        assert_eq!(names(&session), ["Alice"]);
        output(&mut session, "add Bob");
        output(&mut session, "save");

        let saved = String::from_utf8(storage.read("cup").unwrap()).unwrap();
        let imported = tournament_app::session::Session::import(&saved, "cup", &[]).unwrap();
        let entrants = imported.entrants();
        let names: Vec<_> = entrants
            .iter()
            .map(|entrant| entrant.name.as_deref())
            .collect();
        assert_eq!(names, [Some("Alice"), Some("Bob")]);
    }

    #[test]
    fn a_session_that_was_never_saved_needs_a_name() {
        let mut session = Session::new(MemoryStorage::default());

        assert!(matches!(execute(&mut session, "save"), Err(Error::NoName)));
    }

    #[test]
    fn exit_refuses_to_discard_unsaved_changes_unless_forced() {
        let mut session = Session::new(MemoryStorage::default());
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
        let mut session = Session::new(MemoryStorage::default()).named("cup");
        output(&mut session, "add Alice");
        output(&mut session, "save");

        output(&mut session, "undo");

        assert!(session.has_unsaved_changes());
    }

    #[test]
    fn what_is_no_competition_is_rejected() {
        let mut storage = MemoryStorage::default();
        storage.write("notes", b"{}").unwrap();

        assert!(matches!(
            Session::load(storage, "notes"),
            Err(Error::Load { .. })
        ));
    }

    #[test]
    fn a_file_of_a_newer_format_is_rejected_for_its_format() {
        let mut storage = MemoryStorage::default();
        let mut session = Session::new(storage.clone()).named("cup");
        output(&mut session, "save");
        let mut saved: Value = serde_json::from_slice(&storage.read("cup").unwrap()).unwrap();
        assert_eq!(saved["format"], 1);
        saved["format"] = 2.into();
        storage.write("cup", saved.to_string().as_bytes()).unwrap();

        let Err(Error::Load { source, .. }) = Session::load(storage, "cup") else {
            panic!("a file of a newer format must not load");
        };
        assert!(source.to_string().contains("format 2"), "{source}");
    }

    #[test]
    fn a_name_nothing_is_saved_under_cannot_be_loaded() {
        assert!(matches!(
            Session::load(MemoryStorage::default(), "cup"),
            Err(Error::Read { .. })
        ));
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
