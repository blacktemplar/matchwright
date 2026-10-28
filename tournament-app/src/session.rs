//! A competition being run by an operator, from creating or opening it to the last result.
//!
//! A [`Session`] owns the [`CompetitionStateMachine`] of the competition and turns what the
//! operator does into its transitions. It keeps the names of the entrants apart from the log, as
//! [`People`], and is saved to a [`Storage`] as the log entries and people it added since the
//! last save. What can be undone and redone is what the session did itself, which is not saved: a
//! loaded competition starts with nothing to undo.

use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    num::NonZeroU32,
};

use indexmap::IndexSet;
use plugin_factory::{
    catalog::{AnyAxis, PluginEntry},
    config::{Config, PluginConfig},
    draft::Draft,
};
use tournament_engine::{
    AddEntrant, AddPhase, Annotated as _, Competition, CompetitionStateMachine,
    CompetitionStateMachineExt as _, CompetitionTransition, Entrant, EntrantId, Match, MatchId,
    MatchResult, Phase, PhaseData, PhaseId, PhasePlugins, RankingId, RemoveEntrant,
    ReportMatchResult, Seed, SetCompetitionSeed, StartCompetition, TypeMap,
    map::IdAble as _,
    plugins::{metrics_calculator::MetricsCalculator, phase_format::PhaseFormat},
    transitions::{
        self as engine, CompetitionAlreadyStarted, ReconfigurePhaseError, UndoMatchResultError,
    },
    undoable_state_machine::{
        LogEntry, LogEntryKind, RedoLastError, SessionHistory, StateMachineError, TransitionError,
        UndoLastError,
    },
    uuid::Uuid,
};
use unicode_normalization::UnicodeNormalization as _;

use crate::{
    error::{
        AddEntrantError, AddPhaseError, EditPhaseError, ImportError, LoadError, NoPhase,
        NothingToRedo, NothingToUndo, PhaseDraftError, RemoveEntrantError, ReportEnteredError,
        ReportError, StartError, UndoReportError,
    },
    file::{self, File},
    form::PhaseForm,
    people::{People, Person},
    storage::{Changes, CompetitionKey, Listing, Progress, Storage, Stored},
    view::{
        Action, Actions, ByeRow, Column, EntrantRow, Format, MatchRow, Medal, Reopened, Reported,
        Round, RoundSummary, StageName, StandingRow, Standings,
    },
};

/// A competition being run by an operator.
///
/// The first entry of the log of every competition is the [`SetCompetitionSeed`] it was created
/// with, which is no action of the session and so never undone.
pub struct Session {
    key: CompetitionKey,
    name: String,
    machine: CompetitionStateMachine,
    /// Everyone ever entered in the log, removed entrants included, since undoing their removal
    /// brings them back.
    people: People,
    /// What this session did, which `undo` and `redo` walk through.
    history: SessionHistory,
    /// The actions of `history` that can be undone, the latest last, and those that can be
    /// redone, the one undone last at the end, which it does not tell.
    done: Vec<Action>,
    undone: Vec<Action>,
    /// The configuration of the phase whose adding this session undid last, which the form of a
    /// new phase starts out at.
    undone_phase: Option<Config>,
    /// The length of the log when it was last saved or loaded. The log is append-only, so every
    /// change since makes it longer, undo and redo included.
    saved_entries: usize,
    /// The people added or changed since the last save.
    unsaved_people: IndexSet<EntrantId>,
}

impl Session {
    /// Creates a new competition named `name`, with a random seed. Nothing of it is saved yet.
    ///
    /// # Panics
    /// Panics if the platform provides no randomness.
    pub fn new(name: impl Into<String>) -> Self {
        let mut seed = [0; 12];
        getrandom::fill(&mut seed).expect("the platform must provide randomness");
        let mut machine = CompetitionStateMachine::default();
        machine
            .apply_transition(SetCompetitionSeed::new(Seed::new(seed)))
            .expect("a new competition must accept a seed");
        Self {
            key: CompetitionKey::new(Uuid::new_v4()),
            name: name.into(),
            machine,
            people: People::default(),
            history: SessionHistory::new(),
            done: Vec::new(),
            undone: Vec::new(),
            undone_phase: None,
            saved_entries: 0,
            unsaved_people: IndexSet::new(),
        }
    }

    /// Opens the competition stored in `storage` under `key`. The session has done nothing yet, so
    /// there is nothing to undo.
    ///
    /// # Errors
    /// Returns an error if nothing is stored under `key`, the storage cannot be read, or what is
    /// stored is no log that replays into a competition.
    pub async fn load<S: Storage>(
        storage: &S,
        key: CompetitionKey,
    ) -> Result<Self, LoadError<S::Error>> {
        let stored = read(storage, key, 0).await?;
        let machine =
            CompetitionStateMachine::build(entries(&stored, 0)?).map_err(LoadError::Replay)?;
        let people = people(&stored)?;
        Ok(Self {
            key,
            name: stored.name,
            saved_entries: machine.log().len(),
            machine,
            people,
            history: SessionHistory::new(),
            done: Vec::new(),
            undone: Vec::new(),
            undone_phase: None,
            unsaved_people: IndexSet::new(),
        })
    }

    /// Catches up with what was saved of the competition in `storage` since this session loaded or
    /// caught up last, e.g. by a session in another tab, see [`Self::catch_up_with`]: reads what
    /// is stored from [`Self::next_entry`] on and hands it over. Returns how many entries it
    /// appended.
    ///
    /// # Errors
    /// Returns an error if nothing is stored under the session's key, the storage cannot be read,
    /// or an entry cannot be read or does not apply, see [`Self::catch_up_with`].
    ///
    /// # Panics
    /// Panics if the session has unsaved changes, see [`Self::catch_up_with`].
    pub async fn catch_up<S: Storage>(
        &mut self,
        storage: &S,
    ) -> Result<usize, LoadError<S::Error>> {
        let first_entry = self.next_entry();
        let stored = read(storage, self.key, first_entry).await?;
        self.catch_up_with(&stored, first_entry)
    }

    /// The position in the log of the entry appended next, from which a storage is read to catch
    /// up, see [`Self::catch_up_with`].
    pub fn next_entry(&self) -> usize {
        self.machine.log().len()
    }

    /// Catches up with `stored`, what a storage holds of this session's competition with the
    /// entries from the position `first_entry` on, which is [`Self::next_entry`] when it was read:
    /// appends the entries, each as [`Self::load`] would have replayed it, and takes the people as
    /// stored. Returns how many entries it appended. They are no actions of this session, so
    /// there is nothing more to undo. [`Self::catch_up`] reads and catches up in one; reading
    /// apart lets a GUI hold no borrow of the session while it reads.
    ///
    /// An entry that undoes or replaces an earlier one replays the log after that one, as it did
    /// when it was appended; any other entry only applies to the current state.
    ///
    /// # Errors
    /// Returns an error if an entry cannot be read or does not apply. If one cannot be read,
    /// nothing is appended; if one does not apply, the entries before it stay appended.
    ///
    /// # Panics
    /// Panics if `first_entry` is not [`Self::next_entry`], or the session has unsaved changes:
    /// what was stored after them is of another session that took over the competition, and the
    /// log would fork.
    pub fn catch_up_with<E>(
        &mut self,
        stored: &Stored,
        first_entry: usize,
    ) -> Result<usize, LoadError<E>> {
        assert!(
            !self.has_unsaved_changes(),
            "a session catches up only with nothing unsaved"
        );
        assert_eq!(
            first_entry,
            self.next_entry(),
            "a session catches up from the end of its log"
        );
        let entries = entries(stored, first_entry)?;
        self.people = people(stored)?;
        let appended = entries.len();
        for entry in entries {
            self.machine.apply(entry).map_err(LoadError::Replay)?;
            self.saved_entries = self.machine.log().len();
        }
        Ok(appended)
    }

    /// The key the competition is stored under.
    pub fn key(&self) -> CompetitionKey {
        self.key
    }

    /// The name the operator gave the competition.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn competition(&self) -> &Competition {
        self.machine.current()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.machine.log().len() != self.saved_entries || !self.unsaved_people.is_empty()
    }

    /// What changed since the last save, to be written to a storage and then handed to
    /// [`Self::mark_saved`]; [`Self::save`] does both. Saving the changes of a session that
    /// has not been saved yet creates the stored competition.
    ///
    /// # Panics
    /// Panics if an entry of the log or a person does not serialize as JSON.
    pub fn unsaved_changes(&self) -> Changes {
        Changes {
            listing: Listing {
                key: self.key,
                name: self.name.clone(),
                progress: Some(self.progress()),
            },
            first_entry: self.saved_entries,
            entries: self.machine.log()[self.saved_entries..]
                .iter()
                .map(|entry| serde_json::to_string(entry).expect("a log entry serializes as JSON"))
                .collect(),
            people: self
                .unsaved_people
                .iter()
                .map(|id| (*id, record(self.person(id))))
                .collect(),
        }
    }

    /// Notes that `changes`, which [`Self::unsaved_changes`] of this session returned, have been
    /// written to its storage. What changed after they were taken stays unsaved, so that changes
    /// whose writes finish out of order are all saved once every write has.
    ///
    /// # Panics
    /// Panics if `changes` are of another competition.
    pub fn mark_saved(&mut self, changes: &Changes) {
        assert_eq!(
            changes.listing.key, self.key,
            "the changes must be of this session's competition"
        );
        self.saved_entries = self
            .saved_entries
            .max(changes.first_entry + changes.entries.len());
        for (id, saved) in &changes.people {
            if record(self.person(id)) == *saved {
                self.unsaved_people.shift_remove(id);
            }
        }
    }

    /// Writes what changed since the last save to `storage`.
    ///
    /// # Errors
    /// Returns the error of the storage, in which case the changes stay unsaved and the next save
    /// writes them.
    pub async fn save<S: Storage>(&mut self, storage: &S) -> Result<(), S::Error> {
        let changes = self.unsaved_changes();
        storage.write(&changes).await?;
        self.mark_saved(&changes);
        Ok(())
    }

    /// The competition as a file, with its name, log and people, see [`crate::file`].
    pub fn export(&self) -> String {
        file::to_json(&File {
            name: Some(self.name.clone()),
            log: self.machine.log(),
            people: &self.people,
        })
    }

    /// A new competition with what the file `file` holds, e.g. one [`Self::export`] or the CLI
    /// wrote, named as in the file or else `name`, followed by the first number from 2 on in
    /// parentheses that makes it a name none of `stored` has, if one has it, e.g. `"Cup (2)"`. It
    /// is stored under a key of its own, so that importing a file twice makes two competitions;
    /// nothing of it is saved yet.
    ///
    /// # Errors
    /// Returns an error if `file` is no competition's file, or its log does not replay.
    pub fn import(file: &str, name: &str, stored: &[Listing]) -> Result<Self, ImportError> {
        let File {
            name: named,
            log: machine,
            people,
        } = serde_json::from_str::<File<CompetitionStateMachine, People>>(file)
            .map_err(ImportError)?;
        let name = named.unwrap_or_else(|| name.to_owned());
        let taken = |name: &str| stored.iter().any(|listing| listing.name == name);
        let name = if taken(&name) {
            (2..)
                .map(|number| format!("{name} ({number})"))
                .find(|copy| !taken(copy))
                .expect("a name of the numbers is not taken")
        } else {
            name
        };
        Ok(Self {
            key: CompetitionKey::new(Uuid::new_v4()),
            name,
            unsaved_people: people.iter().map(|person| person.id()).collect(),
            machine,
            people,
            history: SessionHistory::new(),
            done: Vec::new(),
            undone: Vec::new(),
            undone_phase: None,
            saved_entries: 0,
        })
    }

    /// What the operator can do now.
    pub fn actions(&self) -> Actions {
        let competition = self.competition();
        let started = competition.is_started();
        let has_phase = !competition.phases().is_empty();
        Actions {
            add_entrant: !started,
            remove_entrant: !started && !competition.entrants().is_empty(),
            add_phase: !started && !has_phase,
            edit_phase: has_phase,
            start: self.start_refusal().is_none(),
            report: self.started_phase().is_some_and(|phase| {
                phase
                    .data()
                    .matches()
                    .values()
                    .any(|r#match| !r#match.is_finished())
            }),
            undo: !self.done.is_empty(),
            redo: !self.undone.is_empty(),
        }
    }

    /// Why [`Self::start`] would refuse to start the competition now, or `None` if it would start
    /// it.
    pub fn start_refusal(&self) -> Option<StartError> {
        let competition = self.competition();
        if competition.is_started() {
            Some(StartError::Started)
        } else if competition.phases().is_empty() {
            Some(NoPhase.into())
        } else if competition.entrants().len() < 2 {
            Some(StartError::TooFewEntrants)
        } else {
            None
        }
    }

    /// The entrants, in the order they were added.
    pub fn entrants(&self) -> Vec<EntrantRow> {
        self.competition()
            .entrants()
            .keys()
            .map(|id| self.entrant_row(id))
            .collect()
    }

    /// The name of an entrant that `name` would look like, e.g. `Alice` for `alice` or for an
    /// `Alice` whose characters are composed otherwise: equal ignoring case and the way Unicode
    /// composes them (NFKC), and leading and trailing whitespace. Adding it is not refused for
    /// that, unless the names are equal, but an operator may want to know.
    pub fn entrant_like(&self, name: &str) -> Option<String> {
        let folded = |name: &str| name.trim().nfkc().collect::<String>().to_lowercase();
        let name = folded(name);
        if name.is_empty() {
            return None;
        }
        self.competition()
            .entrants()
            .keys()
            .filter_map(|id| self.people.name(id))
            .find(|entered| folded(entered) == name)
            .map(str::to_owned)
    }

    /// Adds an entrant shown by `name`, with leading and trailing whitespace removed.
    ///
    /// # Errors
    /// Returns an error if `name` is empty or another entrant's, or if the competition has started.
    pub fn add_entrant(&mut self, name: &str) -> Result<EntrantId, AddEntrantError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AddEntrantError::EmptyName);
        }
        if self
            .competition()
            .entrants()
            .keys()
            .any(|id| self.people.name(id) == Some(name))
        {
            return Err(AddEntrantError::DuplicateName(name.to_owned()));
        }
        let id = EntrantId::new(Uuid::new_v4());
        let action = Action::AddEntrant(EntrantRow {
            id,
            name: Some(name.to_owned()),
        });
        self.act(action, |machine, history| {
            machine.apply_in(history, AddEntrant::new(Entrant::new(id)))
        })
        .map_err(|error| {
            let error = rejection(error);
            if error.is::<CompetitionAlreadyStarted>() {
                AddEntrantError::Started
            } else {
                AddEntrantError::Rejected(error)
            }
        })?;
        self.people.add(Person::new(id, name));
        self.unsaved_people.insert(id);
        Ok(id)
    }

    /// Removes the entrant `id`. Its name is kept, in case the removal is undone.
    ///
    /// # Errors
    /// Returns an error if there is no such entrant, or the competition has started.
    pub fn remove_entrant(&mut self, id: EntrantId) -> Result<(), RemoveEntrantError> {
        let action = Action::RemoveEntrant(self.entrant_row(&id));
        self.act(action, |machine, history| {
            machine.apply_in(history, RemoveEntrant::new(id))
        })
        .map_err(|error| {
            let error = rejection(error);
            if error.is::<CompetitionAlreadyStarted>() {
                RemoveEntrantError::Started
            } else if error.is::<engine::RemoveEntrantError>() {
                RemoveEntrantError::UnknownEntrant
            } else {
                RemoveEntrantError::Rejected(error)
            }
        })?;
        Ok(())
    }

    /// Every format a phase can be played in.
    pub fn formats() -> Vec<Format> {
        AnyAxis::of::<dyn PhaseFormat>()
            .entries()
            .map(|entry| Format {
                id: entry.id(),
                label: entry.label(),
                description: entry.description(),
            })
            .collect()
    }

    /// A draft of a phase of this competition played in the format `format`, a [`Format::id`],
    /// with every field answered as offered, see [`Draft::with_defaults`]. It can be changed
    /// before it is handed to [`Self::add_phase`].
    ///
    /// # Errors
    /// Returns an error if there is no such format, or it offers an answer it does not take.
    pub fn phase_draft(&self, format: &str) -> Result<Draft<PhasePlugins>, PhaseDraftError> {
        let entry = AnyAxis::of::<dyn PhaseFormat>()
            .entries()
            .find(|entry| entry.id() == format)
            .ok_or_else(|| PhaseDraftError::UnknownFormat(format.to_owned()))?;
        Ok(self
            .competition()
            .phase_draft()
            .with("format", PluginConfig::new(entry.id()))?
            .with_defaults()?)
    }

    /// A form of a new phase of this competition, answered as the phase was played whose adding
    /// this session undid last, so that adding it again adds the same phase; if it undid none,
    /// with every field answered as offered: in the format a phase offers, with what that
    /// recommends. Once filled, its [draft](PhaseForm::draft) is handed to [`Self::add_phase`].
    ///
    /// # Errors
    /// Returns an error if a plugin offers an answer it does not take.
    pub fn phase_form(&self) -> Result<PhaseForm, PhaseDraftError> {
        let draft = self.competition().phase_draft();
        let draft = match &self.undone_phase {
            Some(config) => draft.with_answers(config)?,
            None => draft.with_defaults()?,
        };
        Ok(PhaseForm::new(draft))
    }

    /// A form of the phase as it is played, see [`Self::phase_draft_to_edit`]. Once changed, its
    /// [draft](PhaseForm::draft) is handed to [`Self::edit_phase`].
    ///
    /// # Errors
    /// Returns [`NoPhase`] if the competition has no phase.
    pub fn phase_form_to_edit(&self) -> Result<PhaseForm, NoPhase> {
        self.phase_draft_to_edit().map(PhaseForm::new)
    }

    /// Adds the phase `draft` configures, which is the only phase the competition is played in.
    /// `draft` is a draft of this competition's phase, e.g. from [`Self::phase_draft`].
    ///
    /// A calculator that has nothing to be configured is added for a metric the phase needs and
    /// nothing calculates, see [`Draft::add_implied`]; `draft` itself is left as it is.
    ///
    /// # Errors
    /// Returns an error if the plugins `draft` configures do not fit together, the competition has
    /// a phase already, or it has started.
    pub fn add_phase(&mut self, draft: &Draft<PhasePlugins>) -> Result<(), AddPhaseError> {
        let plugins = completed(draft).map_err(AddPhaseError::Invalid)?;
        self.act(Action::AddPhase, |machine, history| {
            machine.apply_in(
                history,
                AddPhase::new(PhaseId::new(Uuid::new_v4()), plugins),
            )
        })
        .map_err(|error| {
            let error = rejection(error);
            if error.is::<CompetitionAlreadyStarted>() {
                AddPhaseError::Started
            } else if error.is::<engine::AddPhaseError>() {
                AddPhaseError::PhaseAlreadyAdded
            } else {
                AddPhaseError::Rejected(error)
            }
        })?;
        Ok(())
    }

    /// A draft holding the configuration the phase is played with, see [`Phase::config`], to be
    /// changed and handed to [`Self::edit_phase`].
    ///
    /// # Errors
    /// Returns [`NoPhase`] if the competition has no phase.
    ///
    /// # Panics
    /// Panics if the draft does not take the configuration of the phase, which it was built from.
    pub fn phase_draft_to_edit(&self) -> Result<Draft<PhasePlugins>, NoPhase> {
        let config = self.phase().ok_or(NoPhase)?.config();
        Ok(self
            .competition()
            .phase_draft()
            .with_answers(&config)
            .expect("a phase's draft takes the configuration the phase was built from"))
    }

    /// Plays the phase as `draft` configures it, as if it had been added that way, and returns
    /// whether that changed anything. `draft` is completed as by [`Self::add_phase`].
    ///
    /// # Errors
    /// Returns an error if the competition has no phase, the plugins `draft` configures do not fit
    /// together, or a change made after the phase was added, e.g. a reported result, does not fit
    /// the edited phase.
    pub fn edit_phase(&mut self, draft: &Draft<PhasePlugins>) -> Result<bool, EditPhaseError> {
        let phase_id = self.phase().ok_or(NoPhase)?.id();
        let plugins = completed(draft).map_err(EditPhaseError::Invalid)?;
        let edited = self
            .act(Action::EditPhase, |machine, history| {
                machine.reconfigure_phase(history, phase_id, plugins)
            })
            .map_err(|error| match error {
                ReconfigurePhaseError::Rejected(error) => {
                    EditPhaseError::DoesNotFit(rejection(error))
                }
                ReconfigurePhaseError::NotAdded => {
                    unreachable!("the phase of a session is added by an `AddPhase` in effect")
                }
            })?;
        Ok(edited.is_some())
    }

    /// Starts the competition, which schedules the first matches.
    ///
    /// # Errors
    /// Returns an error if the competition has started already, has no phase, or fewer than two
    /// entrants.
    pub fn start(&mut self) -> Result<(), StartError> {
        self.act(Action::Start, |machine, history| {
            machine.apply_in(history, StartCompetition)
        })
        .map_err(|error| {
            let error = rejection(error);
            if error.is::<CompetitionAlreadyStarted>() {
                return StartError::Started;
            }
            match error.downcast_ref() {
                Some(engine::StartCompetitionError::NoPhases) => NoPhase.into(),
                Some(engine::StartCompetitionError::TooFewEntrants) => StartError::TooFewEntrants,
                None => StartError::Rejected(error),
            }
        })?;
        Ok(())
    }

    /// How far the competition is, as its [`Listing`] keeps it.
    pub fn progress(&self) -> Progress {
        if !self.competition().is_started() {
            return Progress::Setup {
                entrants: self.competition().entrants().len(),
            };
        }
        if let Some(summary) = self.rounds().into_iter().find(|summary| summary.open > 0) {
            return Progress::Playing {
                round: summary.round,
                open: summary.open,
            };
        }
        let winners = self.winners().unwrap_or_default();
        Progress::Won {
            winners: winners
                .into_iter()
                .map(|team| team.into_iter().map(|entrant| entrant.name).collect())
                .collect(),
        }
    }

    /// The teams ranked first once every match scheduled has a result, more than one if they are
    /// tied, by the names of their entrants, each team by its entrants; `None` before the start or
    /// while a match has no result.
    pub fn winners(&self) -> Option<Vec<Vec<EntrantRow>>> {
        let data = self.started_phase()?.data();
        if data
            .matches()
            .values()
            .any(|r#match| r#match.result().is_none())
        {
            return None;
        }
        let mut winners: Vec<_> = data
            .standings()
            .filter(|ranking| ranking.rank() == NonZeroU32::MIN)
            .map(|ranking| self.team(data, &[ranking.id()]))
            .collect();
        // tied teams are in no order of their own
        winners.sort_by_cached_key(|team| {
            team.iter()
                .map(|entrant| entrant.name.as_deref().map(str::to_lowercase))
                .collect::<Vec<_>>()
        });
        Some(winners)
    }

    /// The rounds the phase has scheduled matches or byes in, in order; none before the start.
    pub fn rounds(&self) -> Vec<RoundSummary> {
        let Some(phase) = self.started_phase() else {
            return Vec::new();
        };
        let data = phase.data();
        let empty = |round| RoundSummary {
            round,
            matches: 0,
            open: 0,
        };
        let mut rounds = BTreeMap::new();
        for r#match in data.matches().values() {
            let round = r#match.round();
            let summary = rounds.entry(round).or_insert_with(|| empty(round));
            summary.matches += 1;
            if !r#match.is_finished() {
                summary.open += 1;
            }
        }
        for bye in data.byes().values() {
            let round = bye.round();
            rounds.entry(round).or_insert_with(|| empty(round));
        }
        rounds.into_values().collect()
    }

    /// The matches and byes of `round`, or `None` if it has neither, e.g. before the start.
    pub fn round(&self, round: NonZeroU32) -> Option<Round> {
        let phase = self.started_phase()?;
        let data = phase.data();
        let mut matches: Vec<_> = data
            .matches()
            .values()
            .filter(|r#match| r#match.round() == round)
            .map(|r#match| self.match_row(phase, r#match))
            .collect();
        // a bracket numbers its matches before it schedules them, in another order
        matches.sort_by_key(|row| row.number);
        let byes_of_round: Vec<_> = data
            .byes()
            .values()
            .filter(|bye| bye.round() == round)
            .map(|bye| (bye, stage(phase, bye.annotations())))
            .collect();
        let stages: HashSet<_> = matches
            .iter()
            .map(|row| &row.stage)
            .chain(byes_of_round.iter().map(|(_, stage)| stage))
            .collect();
        let several_stages = stages.len() > 1;
        let byes: Vec<_> = byes_of_round
            .into_iter()
            .map(|(bye, stage)| ByeRow {
                entrants: self.team(data, &[bye.ranking()]),
                stage: stage.filter(|_| several_stages),
            })
            .collect();
        (!matches.is_empty() || !byes.is_empty()).then_some(Round {
            round,
            matches,
            byes,
            draws: phase.format().allows_draws(),
        })
    }

    /// The matches that can be played now, in the order they were scheduled: those without a
    /// result none of whose entrants still has to play a match of an earlier round. None before
    /// the start.
    pub fn playable(&self) -> Vec<MatchRow> {
        let Some(phase) = self.started_phase() else {
            return Vec::new();
        };
        let data = phase.data();
        let open = || {
            data.matches()
                .values()
                .filter(|r#match| !r#match.is_finished())
        };
        let entrants = |r#match: &Match| -> Vec<EntrantId> {
            r#match
                .team_a()
                .iter()
                .chain(r#match.team_b())
                .flat_map(|ranking| data.rankings()[ranking].entrants())
                .copied()
                .collect()
        };
        // the earliest round each entrant still has to play a match in
        let mut next_round = HashMap::new();
        for r#match in open() {
            for entrant in entrants(r#match) {
                let round = next_round.entry(entrant).or_insert(r#match.round());
                *round = (*round).min(r#match.round());
            }
        }
        open()
            .filter(|r#match| {
                entrants(r#match)
                    .iter()
                    .all(|entrant| next_round[entrant] == r#match.round())
            })
            .map(|r#match| self.match_row(phase, r#match))
            .collect()
    }

    /// Reports the result `entry` says: the number of a match, as [`MatchRow::number`], and its
    /// result, separated by whitespace or a `-`. The result is `1` for a win of side A, `2` for one
    /// of side B, and `X` (or `x`) for a draw, which needs no separator, e.g. `"3 1"`, `"3-1"`,
    /// `"12 X"` or `"12x"`.
    ///
    /// # Errors
    /// Returns an error if `entry` does not read as a number and a result, there is no match of
    /// that number, it has a result already, or the result is not reported, see [`Self::report`].
    pub fn report_entered(&mut self, entry: &str) -> Result<Reported, ReportEnteredError> {
        let (number, result) = read_entry(entry).ok_or(ReportEnteredError::Unreadable)?;
        let phase = self.started_phase().ok_or(ReportError::NotStarted)?;
        let id = phase
            .data()
            .matches()
            .values()
            .find(|r#match| self::number(r#match) == number)
            .map(Match::id)
            .ok_or(ReportEnteredError::NoMatch(number))?;
        self.report(id, result).map_err(|error| match error {
            ReportError::AlreadyFinished => ReportEnteredError::AlreadyFinished(number),
            error => error.into(),
        })
    }

    /// Reports `result` for the match `id`.
    ///
    /// # Errors
    /// Returns an error if the competition has not started, has no such match, the match has a
    /// result already, or its format does not take the result.
    pub fn report(&mut self, id: MatchId, result: MatchResult) -> Result<Reported, ReportError> {
        let phase = self.started_phase().ok_or(ReportError::NotStarted)?;
        let Some(r#match) = phase.data().matches().get(&id) else {
            return Err(ReportError::UnknownMatch);
        };
        let row = MatchRow {
            result: Some(result),
            ..self.match_row(phase, r#match)
        };
        let phase_id = phase.id();
        let matches_before = phase.data().matches().len();
        self.act(Action::Report(row.clone()), |machine, history| {
            machine.apply_in(history, ReportMatchResult::new(phase_id, id, result))
        })
        .map_err(|error| {
            let error = rejection(error);
            match error.downcast_ref() {
                Some(engine::ReportMatchResultError::MatchAlreadyFinished) => {
                    ReportError::AlreadyFinished
                }
                Some(engine::ReportMatchResultError::DrawNotAllowed) => ReportError::DrawNotAllowed,
                _ => ReportError::Rejected(error),
            }
        })?;
        let matches = self
            .started_phase()
            .expect("still started")
            .data()
            .matches();
        Ok(Reported {
            row,
            new_matches: matches.len() - matches_before,
            all_finished: matches.values().all(Match::is_finished),
        })
    }

    /// Undoes the result reported for the match `id`, which may be any reported match rather than
    /// only the one reported last. Every other change stays in effect.
    ///
    /// # Errors
    /// Returns an error if the competition has not started, has no such match, the match has no
    /// result, or a later result depends on it.
    pub fn undo_report(&mut self, id: MatchId) -> Result<Reopened, UndoReportError> {
        let phase = self.started_phase().ok_or(UndoReportError::NotStarted)?;
        let Some(r#match) = phase.data().matches().get(&id) else {
            return Err(UndoReportError::UnknownMatch);
        };
        let row = self.match_row(phase, r#match);
        let phase_id = phase.id();
        let matches_before = phase.data().matches().len();
        self.act(Action::Reopen(row.clone()), |machine, history| {
            machine.undo_match_result(history, phase_id, id)
        })
        .map_err(|error| match error {
            UndoMatchResultError::NotReported => UndoReportError::NotReported,
            UndoMatchResultError::Rejected(error) => self
                .reported_number(&error)
                .map_or(UndoReportError::DependedOn, UndoReportError::DependedOnBy),
        })?;
        let matches = self
            .started_phase()
            .expect("still started")
            .data()
            .matches();
        Ok(Reopened {
            row,
            unscheduled: matches_before - matches.len(),
        })
    }

    /// The standings of the phase, with a column for every calculator that offers one; `None`
    /// before the start.
    pub fn standings(&self) -> Option<Standings> {
        let phase = self.started_phase()?;
        let columns: Vec<_> = phase
            .metrics_calculators()
            .iter()
            .filter_map(|calculator| {
                Some((plugin_factory::id_of(&**calculator), calculator.column()?))
            })
            .collect();
        let data = phase.data();
        let finished = data.matches().values().all(Match::is_finished);
        Some(Standings {
            columns: columns
                .iter()
                .map(|(calculator, column)| {
                    let entry = AnyAxis::of::<dyn MetricsCalculator>().entry(calculator);
                    Column {
                        calculator,
                        header: column.header(),
                        label: entry.map_or(column.header(), PluginEntry::label),
                        description: entry.and_then(PluginEntry::description),
                    }
                })
                .collect(),
            rows: data
                .standings()
                .map(|ranking| StandingRow {
                    rank: ranking.rank(),
                    entrants: self.team(data, &[ranking.id()]),
                    played: ranking.played_matches().len(),
                    cells: columns
                        .iter()
                        .map(|(_, column)| column.cell(ranking))
                        .collect(),
                    medal: finished.then(|| Medal::of(ranking.rank())).flatten(),
                })
                .collect(),
        })
    }

    /// The action [`Self::undo`] would undo now, if any, e.g. so that undoing the start, which
    /// leaves play, is asked about first.
    pub fn next_undo(&self) -> Option<&Action> {
        self.done.last()
    }

    /// Undoes the latest action of this session that is still in effect, which may be an
    /// [`Self::undo_report`], bringing the result back. Returns the action undone.
    ///
    /// # Errors
    /// Returns [`NothingToUndo`] if every action of the session is undone.
    pub fn undo(&mut self) -> Result<Action, NothingToUndo> {
        // read before the undo, which removes the phase
        let phase = (self.done.last() == Some(&Action::AddPhase))
            .then(|| self.phase().map(Phase::config))
            .flatten();
        match self.history.undo_last(&mut self.machine) {
            Ok(_) => {
                let action = self.done.pop().expect("an action was undone");
                self.undone.push(action.clone());
                if phase.is_some() {
                    self.undone_phase = phase;
                }
                Ok(action)
            }
            Err(UndoLastError::NothingToUndo) => Err(NothingToUndo),
            Err(error) => unreachable!(
                "only the session appends to its log, so its latest action can be undone: {error}"
            ),
        }
    }

    /// Redoes the action this session undid last, as long as it took no other action since.
    /// Returns the action redone.
    ///
    /// # Errors
    /// Returns [`NothingToRedo`] if there is no such action.
    pub fn redo(&mut self) -> Result<Action, NothingToRedo> {
        match self.history.redo_last(&mut self.machine) {
            Ok(_) => {
                let action = self.undone.pop().expect("an action was redone");
                self.done.push(action.clone());
                Ok(action)
            }
            Err(RedoLastError::NothingToRedo) => Err(NothingToRedo),
            Err(error) => unreachable!(
                "only the session appends to its log, so what it undid last can be redone: {error}"
            ),
        }
    }

    /// Runs `run`, which appends to the log as the action `action` of the session, if anything.
    fn act<T, E>(
        &mut self,
        action: Action,
        run: impl FnOnce(&mut CompetitionStateMachine, &mut SessionHistory) -> Result<T, E>,
    ) -> Result<T, E> {
        let log_len = self.machine.log().len();
        let done = run(&mut self.machine, &mut self.history)?;
        if self.machine.log().len() > log_len {
            self.done.push(action);
            self.undone.clear();
        }
        Ok(done)
    }

    /// The number of the match whose result the entry of the log that `error` names reports, if it
    /// is one reporting a result of a match of the phase, e.g. the later result an undo refused
    /// would no longer apply to.
    fn reported_number(&self, error: &StateMachineError) -> Option<usize> {
        let StateMachineError::Transition(id, _) = error else {
            return None;
        };
        let entry = self.machine.log().iter().find(|entry| entry.id() == *id)?;
        let LogEntryKind::Apply(transition) = entry.kind() else {
            return None;
        };
        let transition: &dyn Any = &**transition;
        let reported = transition.downcast_ref::<ReportMatchResult>()?;
        let r#match = self
            .started_phase()?
            .data()
            .matches()
            .get(&reported.match_id())?;
        Some(number(r#match))
    }

    /// The phase of the competition, whether started or not.
    fn phase(&self) -> Option<&Phase> {
        self.competition().phases().values().next()
    }

    fn started_phase(&self) -> Option<&Phase> {
        self.phase().filter(|_| self.competition().is_started())
    }

    /// The person entered as `id`, which every entrant a session added has.
    fn person(&self, id: &EntrantId) -> &Person {
        self.people
            .get(id)
            .expect("an entrant added by the session has a name")
    }

    fn entrant_row(&self, id: &EntrantId) -> EntrantRow {
        EntrantRow {
            id: *id,
            name: self.people.name(id).map(str::to_owned),
        }
    }

    fn team(&self, phase: &PhaseData, rankings: &[RankingId]) -> Vec<EntrantRow> {
        rankings
            .iter()
            .flat_map(|id| phase.rankings()[id].entrants())
            .map(|id| self.entrant_row(id))
            .collect()
    }

    /// The row of `r#match`, a match of `phase`.
    fn match_row(&self, phase: &Phase, r#match: &Match) -> MatchRow {
        let data = phase.data();
        MatchRow {
            id: r#match.id(),
            number: number(r#match),
            round: r#match.round(),
            stage: stage(phase, r#match.annotations()),
            a: self.team(data, r#match.team_a()),
            b: self.team(data, r#match.team_b()),
            result: r#match.result(),
        }
    }
}

/// The number of `r#match`, as [`MatchRow::number`] shows it.
fn number(r#match: &Match) -> usize {
    usize::try_from(r#match.number().get()).expect("a number of a match fits a usize")
}

/// The number of a match and its result that `entry` says, see [`Session::report_entered`].
fn read_entry(entry: &str) -> Option<(usize, MatchResult)> {
    let entry = entry.trim();
    let digits = entry.find(|char: char| !char.is_ascii_digit())?;
    let (number, rest) = entry.split_at(digits);
    let result = rest.trim_start_matches(|char: char| char.is_whitespace() || char == '-');
    // a win needs a separator, or its side would be read as a digit of the number
    let separated = result.len() < rest.len();
    let result = match result {
        "1" if separated => MatchResult::AWins,
        "2" if separated => MatchResult::BWins,
        "X" | "x" => MatchResult::Draw,
        _ => return None,
    };
    Some((number.parse().ok()?, result))
}

/// The stage the format of `phase` names a match or a bye with `annotations` by.
fn stage(phase: &Phase, annotations: &TypeMap) -> Option<StageName> {
    let format = phase.format();
    Some(StageName {
        format: plugin_factory::id_of(format),
        stage: format.stage(annotations)?,
    })
}

/// The plugins `draft`, a phase draft, configures, with the plugins its answers imply added, see
/// [`Draft::add_implied`].
fn completed(
    draft: &Draft<PhasePlugins>,
) -> Result<PhasePlugins, Vec<plugin_factory::draft::Issue>> {
    let mut draft = draft.clone();
    draft.add_implied();
    draft.build()
}

/// The error of the transition an action of the session was rejected with.
fn rejection(error: StateMachineError) -> TransitionError {
    match error {
        StateMachineError::Transition(_, error) => error,
        // the session only appends new entries, each with a new id and targeting one in effect
        error => unreachable!("an action of the session is rejected by a transition: {error}"),
    }
}

/// What is stored of the competition `key` in `storage`, with the entries from `first_entry` on.
async fn read<S: Storage>(
    storage: &S,
    key: CompetitionKey,
    first_entry: usize,
) -> Result<Stored, LoadError<S::Error>> {
    storage
        .read(key, first_entry)
        .await
        .map_err(LoadError::Storage)?
        .ok_or(LoadError::NotFound)
}

/// The entries of `stored`, the first of which is at the position `first_entry` of the log.
fn entries<E>(
    stored: &Stored,
    first_entry: usize,
) -> Result<Vec<LogEntry<Box<dyn CompetitionTransition>>>, LoadError<E>> {
    (first_entry..)
        .zip(&stored.entries)
        .map(|(position, entry)| {
            serde_json::from_str(entry).map_err(|source| LoadError::Entry { position, source })
        })
        .collect()
}

/// The people of `stored`.
fn people<E>(stored: &Stored) -> Result<People, LoadError<E>> {
    let mut people = People::default();
    for person in &stored.people {
        people.add(serde_json::from_str(person).map_err(LoadError::Person)?);
    }
    Ok(people)
}

/// What a person is stored as.
fn record(person: &Person) -> String {
    serde_json::to_string(person).expect("a person serializes as JSON")
}

mod bracket;
#[cfg(test)]
mod tests;
