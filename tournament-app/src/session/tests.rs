use std::{
    cell::Cell,
    fmt,
    pin::pin,
    task::{Context, Poll, Waker},
};

use plugin_factory::{
    PluginType,
    config::{Path as ConfigPath, PluginConfig},
};
use tournament_engine::plugins::{
    metrics_calculator::MoreWins,
    phase_format::{MonsterDyp, RoundRobin, SingleElimination},
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use super::*;
use crate::storage::{MemoryStorage, Stored};

/// The output of `future`, which a [`MemoryStorage`] finishes right away.
#[track_caller]
fn now<T>(future: impl Future<Output = T>) -> T {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the storage of a test never waits"),
    }
}

/// Builds the session a test starts from: Alice, Bob, Carol and Dave entered in a round robin
/// with the defaults of its format, unless the test says otherwise.
struct Given {
    entrants: Vec<&'static str>,
    format: &'static str,
}

impl Given {
    fn new() -> Self {
        Self {
            entrants: vec!["Alice", "Bob", "Carol", "Dave"],
            format: RoundRobin::ID,
        }
    }

    fn entrants(self, entrants: &[&'static str]) -> Self {
        Self {
            entrants: entrants.to_vec(),
            ..self
        }
    }

    fn format(self, format: &'static str) -> Self {
        Self { format, ..self }
    }

    /// The session with the entrants and the phase added, not started.
    fn added(self) -> Session {
        let mut session = Session::new("cup");
        for name in self.entrants {
            session.add_entrant(name).unwrap();
        }
        let draft = session.phase_draft(self.format).unwrap();
        session.add_phase(&draft).unwrap();
        session
    }

    fn started(self) -> Session {
        let mut session = self.added();
        session.start().unwrap();
        session
    }
}

fn names(entrants: &[EntrantRow]) -> Vec<&str> {
    entrants
        .iter()
        .map(|entrant| entrant.name.as_deref().unwrap())
        .collect()
}

/// Every match of the phase, round by round.
fn matches(session: &Session) -> Vec<MatchRow> {
    session
        .rounds()
        .iter()
        .flat_map(|summary| session.round(summary.round).unwrap().matches)
        .collect()
}

/// The match listed at `number`.
fn numbered(session: &Session, number: usize) -> MatchId {
    matches(session)
        .into_iter()
        .find(|row| row.number == number)
        .unwrap()
        .id
}

fn round(number: u32) -> NonZeroU32 {
    NonZeroU32::new(number).unwrap()
}

#[test]
fn entrants_are_listed_by_name_in_the_order_they_were_added() {
    let mut session = Session::new("cup");

    session.add_entrant("Bob").unwrap();
    session.add_entrant("  Alice ").unwrap();

    assert_eq!(names(&session.entrants()), ["Bob", "Alice"]);
}

#[test]
fn an_empty_or_taken_name_adds_no_entrant() {
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();

    assert!(matches!(
        session.add_entrant(" "),
        Err(AddEntrantError::EmptyName)
    ));
    assert!(matches!(
        session.add_entrant("Alice "),
        Err(AddEntrantError::DuplicateName(name)) if name == "Alice"
    ));
    assert_eq!(names(&session.entrants()), ["Alice"]);
}

#[test]
fn a_removed_entrant_comes_back_with_its_name_by_undo() {
    let mut session = Session::new("cup");
    let alice = session.add_entrant("Alice").unwrap();
    session.add_entrant("Bob").unwrap();

    session.remove_entrant(alice).unwrap();
    assert_eq!(names(&session.entrants()), ["Bob"]);
    assert!(matches!(
        session.remove_entrant(alice),
        Err(RemoveEntrantError::UnknownEntrant)
    ));
    session.undo().unwrap();

    assert_eq!(names(&session.entrants()), ["Alice", "Bob"]);
}

#[test]
fn no_entrant_is_added_or_removed_after_the_start() {
    let mut session = Given::new().started();
    let alice = session.entrants()[0].id;

    assert!(matches!(
        session.add_entrant("Eve"),
        Err(AddEntrantError::Started)
    ));
    assert!(matches!(
        session.remove_entrant(alice),
        Err(RemoveEntrantError::Started)
    ));
}

/// What a GUI offers to pick from is every registered format, and each of them is played with
/// what it offers without anything else answered.
#[test]
fn every_format_adds_a_phase_with_the_answers_it_offers() {
    let formats = Session::formats();
    assert!(formats.iter().any(|format| format.id == RoundRobin::ID));

    for format in formats {
        let started = Given::new().format(format.id).started();

        assert!(!started.rounds().is_empty(), "{format:?}");
    }
}

#[test]
fn an_unknown_format_has_no_draft() {
    let session = Session::new("cup");

    assert!(matches!(
        session.phase_draft("swiss"),
        Err(PhaseDraftError::UnknownFormat(format)) if format == "swiss"
    ));
}

/// The points can be scored in many ways, so a phase ranked by them without a points calculator
/// is not added with one the operator was not asked about.
#[test]
fn a_phase_whose_plugins_do_not_fit_is_not_added() {
    let mut session = Session::new("cup");
    let mut draft = session.phase_draft(RoundRobin::ID).unwrap();
    draft
        .set(
            &ConfigPath::ROOT.field("metrics"),
            Vec::<PluginConfig>::new(),
        )
        .unwrap();

    let result = session.add_phase(&draft);

    assert!(
        matches!(result, Err(AddPhaseError::Invalid(_))),
        "{result:?}"
    );
    assert!(session.competition().phases().is_empty());
}

#[test]
fn only_one_phase_is_added() {
    let mut session = Given::new().added();
    let draft = session.phase_draft(SingleElimination::ID).unwrap();

    assert!(matches!(
        session.add_phase(&draft),
        Err(AddPhaseError::PhaseAlreadyAdded)
    ));
}

#[test]
fn the_start_needs_a_phase_and_two_entrants() {
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    session.add_entrant("Bob").unwrap();
    assert!(matches!(session.start(), Err(StartError::NoPhase(_))));

    let mut session = Given::new().entrants(&["Alice"]).added();
    assert!(matches!(session.start(), Err(StartError::TooFewEntrants)));

    let mut session = Given::new().started();
    assert!(matches!(session.start(), Err(StartError::Started)));
}

#[test]
fn what_can_be_done_follows_the_competition() {
    let mut session = Session::new("cup");
    let actions = session.actions();
    assert!(actions.add_entrant && actions.add_phase);
    assert!(!actions.remove_entrant && !actions.edit_phase && !actions.start);
    assert!(!actions.report && !actions.undo && !actions.redo);

    session.add_entrant("Alice").unwrap();
    assert!(session.actions().remove_entrant && session.actions().undo);
    let draft = session.phase_draft(RoundRobin::ID).unwrap();
    session.add_phase(&draft).unwrap();
    assert!(!session.actions().add_phase && session.actions().edit_phase);
    assert!(!session.actions().start, "one entrant is too few");
    session.add_entrant("Bob").unwrap();
    assert!(session.actions().start);

    session.start().unwrap();
    let actions = session.actions();
    assert!(actions.report && actions.edit_phase);
    assert!(!actions.add_entrant && !actions.remove_entrant && !actions.start);

    session
        .report(numbered(&session, 1), MatchResult::Draw)
        .unwrap();
    assert!(!session.actions().report, "the only match has its result");
    session.undo().unwrap();
    assert!(session.actions().report && session.actions().redo);
}

#[test]
fn the_matches_are_listed_by_round_with_the_names_of_their_entrants() {
    let session = Given::new().started();

    let rounds = session.rounds();

    assert_eq!(
        rounds,
        (1..=3)
            .map(|number| RoundSummary {
                round: round(number),
                matches: 2,
                open: 2,
            })
            .collect::<Vec<_>>(),
        "a round robin of four plays three rounds of two matches"
    );
    let first = session.round(round(1)).unwrap();
    assert_eq!(
        first
            .matches
            .iter()
            .map(|row| (row.number, row.round))
            .collect::<Vec<_>>(),
        [(1, round(1)), (2, round(1))]
    );
    let mut entrants: Vec<_> = first
        .matches
        .iter()
        .flat_map(|row| names(&row.a).into_iter().chain(names(&row.b)))
        .collect();
    entrants.sort_unstable();
    assert_eq!(entrants, ["Alice", "Bob", "Carol", "Dave"]);
    assert!(first.matches.iter().all(|row| row.stage.is_none()));
    assert!(first.byes.is_empty());
    assert_eq!(session.round(round(4)), None);
}

#[test]
fn nothing_is_played_before_the_start() {
    let session = Given::new().added();

    assert!(session.rounds().is_empty());
    assert_eq!(session.round(round(1)), None);
    assert_eq!(session.standings(), None);
}

/// A round robin of three has one entrant sit out every round.
#[test]
fn a_round_lists_its_byes() {
    let session = Given::new().entrants(&["Alice", "Bob", "Carol"]).started();

    let second = session.round(round(2)).unwrap();

    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.byes.len(), 1);
    assert_eq!(second.byes[0].entrants.len(), 1);
}

#[test]
fn a_knockout_names_the_stage_of_every_match() {
    let mut session = Given::new().format(SingleElimination::ID).started();
    let stages = |session: &Session| -> Vec<Option<String>> {
        matches(session).into_iter().map(|row| row.stage).collect()
    };
    assert_eq!(
        stages(&session),
        [Some("Semifinals".to_owned()), Some("Semifinals".to_owned())]
    );

    let first = session
        .report(numbered(&session, 1), MatchResult::AWins)
        .unwrap();
    let second = session
        .report(numbered(&session, 2), MatchResult::AWins)
        .unwrap();

    assert_eq!(
        first,
        Reported {
            new_matches: 0,
            all_finished: false
        }
    );
    assert_eq!(
        second,
        Reported {
            new_matches: 2,
            all_finished: false
        },
        "the final and the match for third place"
    );
    assert_eq!(
        stages(&session)[2..],
        [Some("Final".to_owned()), Some("Third place".to_owned())]
    );
    session
        .report(numbered(&session, 3), MatchResult::AWins)
        .unwrap();
    let last = session
        .report(numbered(&session, 4), MatchResult::BWins)
        .unwrap();
    assert_eq!(
        last,
        Reported {
            new_matches: 0,
            all_finished: true
        }
    );
}

#[test]
fn a_result_the_match_does_not_take_is_refused() {
    let mut session = Given::new().format(SingleElimination::ID).started();
    let first = numbered(&session, 1);
    let elsewhere = numbered(&Given::new().started(), 1);

    assert!(matches!(
        session.report(first, MatchResult::Draw),
        Err(ReportError::DrawNotAllowed)
    ));
    session.report(first, MatchResult::AWins).unwrap();
    assert!(matches!(
        session.report(first, MatchResult::BWins),
        Err(ReportError::AlreadyFinished)
    ));
    assert!(matches!(
        session.report(elsewhere, MatchResult::AWins),
        Err(ReportError::UnknownMatch)
    ));
    assert!(matches!(
        Given::new().added().report(first, MatchResult::AWins),
        Err(ReportError::NotStarted)
    ));
}

/// The result of a match reported long ago is undone by its id, which leaves every result reported
/// after it alone, unlike `undo`, which would walk back through all of them.
#[test]
fn a_result_is_undone_by_its_match() {
    let mut session = Given::new().started();
    for number in 1..=3 {
        session
            .report(numbered(&session, number), MatchResult::AWins)
            .unwrap();
    }

    let reopened = session.undo_report(numbered(&session, 1)).unwrap();

    assert_eq!(reopened, Reopened { unscheduled: 0 });
    let results: Vec<_> = matches(&session).iter().map(|row| row.result).collect();
    assert_eq!(
        results[..3],
        [None, Some(MatchResult::AWins), Some(MatchResult::AWins)]
    );
    assert!(matches!(
        session.undo_report(numbered(&session, 1)),
        Err(UndoReportError::NotReported)
    ));
}

/// Undoing the result of a semifinal unschedules the final it decided the participants of, so a
/// final that has a result itself has to be undone first.
#[test]
fn a_result_a_later_one_depends_on_is_undone_after_it() {
    let mut session = Given::new().format(SingleElimination::ID).started();
    for number in 1..=3 {
        session
            .report(numbered(&session, number), MatchResult::AWins)
            .unwrap();
    }
    let semifinal = numbered(&session, 1);

    assert!(matches!(
        session.undo_report(semifinal),
        Err(UndoReportError::DependedOn)
    ));
    session.undo_report(numbered(&session, 3)).unwrap();
    let reopened = session.undo_report(semifinal).unwrap();

    assert_eq!(
        reopened,
        Reopened { unscheduled: 2 },
        "the final and the match for third place go with it"
    );
}

#[test]
fn the_standings_show_the_columns_of_the_metrics() {
    let mut session = Given::new().started();
    for number in 1..=6 {
        session
            .report(numbered(&session, number), MatchResult::AWins)
            .unwrap();
    }

    let standings = session.standings().unwrap();

    assert_eq!(
        standings.columns,
        [
            Column {
                calculator: "tournament_engine::wins_draws_losses",
                header: "W-D-L"
            },
            Column {
                calculator: "tournament_engine::points",
                header: "points"
            },
            Column {
                calculator: "tournament_engine::sonneborn_berger",
                header: "Sonneborn-Berger"
            },
        ]
    );
    let points: Vec<f64> = standings
        .rows
        .iter()
        .map(|row| row.cells[1].as_deref().unwrap().parse().unwrap())
        .collect();
    assert_eq!(points.len(), 4);
    assert!(
        points.windows(2).all(|pair| pair[0] >= pair[1]),
        "{standings:?}"
    );
    assert!(points[0] > points[3], "{standings:?}");
}

#[test]
fn undo_and_redo_walk_back_and_forth_through_the_actions() {
    let mut session = Session::new("cup");
    assert!(
        matches!(session.undo(), Err(NothingToUndo)),
        "the seed is no action of the session"
    );
    session.add_entrant("Alice").unwrap();
    session.add_entrant("Bob").unwrap();

    session.undo().unwrap();
    session.undo().unwrap();
    assert!(session.entrants().is_empty());
    assert!(matches!(session.undo(), Err(NothingToUndo)));
    session.redo().unwrap();
    assert_eq!(names(&session.entrants()), ["Alice"]);

    session.add_entrant("Carol").unwrap();
    assert!(
        matches!(session.redo(), Err(NothingToRedo)),
        "a new action discards what could be redone"
    );
    assert_eq!(names(&session.entrants()), ["Alice", "Carol"]);
}

/// `undo_report` is an action like any other, so `undo` brings the result back rather than undoing
/// the action before it.
#[test]
fn undo_brings_back_a_result_undone_by_its_match() {
    let mut session = Given::new().started();
    let first = numbered(&session, 1);
    session.report(first, MatchResult::AWins).unwrap();
    session.undo_report(first).unwrap();

    session.undo().unwrap();

    assert_eq!(matches(&session)[0].result, Some(MatchResult::AWins));
}

/// An edit starts out at the configuration the phase is played with, so changing the ranking keeps
/// the metrics, and the results reported so far stay.
#[test]
fn a_phase_is_edited_starting_out_at_its_configuration() {
    let mut session = Given::new().started();
    session
        .report(numbered(&session, 1), MatchResult::AWins)
        .unwrap();
    let mut draft = session.phase_draft_to_edit().unwrap();
    draft
        .set(
            &ConfigPath::ROOT.field("ranking"),
            vec![PluginConfig::of::<MoreWins>()],
        )
        .unwrap();

    assert!(session.edit_phase(&draft).unwrap());

    assert_eq!(matches(&session)[0].result, Some(MatchResult::AWins));
    let headers: Vec<_> = session
        .standings()
        .unwrap()
        .columns
        .iter()
        .map(|column| column.header)
        .collect();
    assert_eq!(headers, ["W-D-L", "points", "Sonneborn-Berger"]);
    session.undo().unwrap();
    assert_eq!(
        session.phase_draft_to_edit().unwrap().answers(),
        Given::new()
            .started()
            .phase_draft_to_edit()
            .unwrap()
            .answers()
    );
}

#[test]
fn a_phase_edited_without_a_change_is_left_as_it_is() {
    let mut session = Given::new().started();
    let draft = session.phase_draft_to_edit().unwrap();
    let undoable = session.actions().undo;

    assert!(!session.edit_phase(&draft).unwrap());
    session.undo().unwrap();

    assert!(undoable);
    assert!(
        !session.competition().is_started(),
        "the edit that changed nothing is no action, so undo undoes the start"
    );
}

/// The teams of a Monster DYP are pairs, so it never schedules the match of single entrants whose
/// result was reported.
#[test]
fn an_edit_a_reported_result_does_not_fit_is_refused() {
    let mut session = Given::new().started();
    session
        .report(numbered(&session, 1), MatchResult::AWins)
        .unwrap();
    let draft = session.phase_draft(MonsterDyp::ID).unwrap();

    let result = session.edit_phase(&draft);

    assert!(
        matches!(result, Err(EditPhaseError::DoesNotFit(_))),
        "{result:?}"
    );
    assert_eq!(matches(&session)[0].result, Some(MatchResult::AWins));
    assert!(matches!(
        Session::new("cup").phase_draft_to_edit(),
        Err(NoPhase)
    ));
}

/// What can be undone is what a session did itself, so a loaded competition has the state it was
/// saved with but nothing to undo.
#[test]
fn a_saved_competition_loads_as_it_was_saved_without_anything_to_undo() {
    let storage = MemoryStorage::default();
    let mut session = Given::new().started();
    session
        .report(numbered(&session, 1), MatchResult::AWins)
        .unwrap();

    now(session.save(&storage)).unwrap();
    let mut loaded = now(Session::load(&storage, session.key())).unwrap();

    assert_eq!(loaded.name(), "cup");
    assert!(!loaded.has_unsaved_changes());
    assert_eq!(loaded.entrants(), session.entrants());
    assert_eq!(matches(&loaded), matches(&session));
    assert_eq!(loaded.standings(), session.standings());
    assert!(matches!(loaded.undo(), Err(NothingToUndo)));
    // a single result can still be undone, as it is found in the log
    loaded.undo_report(numbered(&loaded, 1)).unwrap();
    assert_eq!(
        now(storage.list()).unwrap(),
        [Listing {
            key: session.key(),
            name: "cup".to_owned(),
        }]
    );
}

#[test]
fn a_save_writes_what_changed_since_the_last_one() {
    let storage = MemoryStorage::default();
    let mut session = Session::new("cup");
    assert!(session.has_unsaved_changes(), "the seed is not saved yet");
    now(session.save(&storage)).unwrap();
    assert!(!session.has_unsaved_changes());

    session.add_entrant("Alice").unwrap();
    let changes = session.unsaved_changes();

    assert_eq!(changes.first_entry(), 1);
    assert_eq!(changes.entries().len(), 1);
    assert_eq!(changes.people().len(), 1);
    now(session.save(&storage)).unwrap();
    assert!(!session.has_unsaved_changes());
    session.undo().unwrap();
    assert!(
        session.has_unsaved_changes(),
        "an undo is an entry of the log too"
    );
}

/// A GUI writes changes while the operator goes on, so a write may finish after a later one.
#[test]
fn changes_written_out_of_order_are_all_saved() {
    let storage = MemoryStorage::default();
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    let first = session.unsaved_changes();
    session.add_entrant("Bob").unwrap();
    let second = session.unsaved_changes();

    now(storage.write(&second)).unwrap();
    session.mark_saved(&second);
    now(storage.write(&first)).unwrap();
    session.mark_saved(&first);

    assert!(!session.has_unsaved_changes());
    let loaded = now(Session::load(&storage, session.key())).unwrap();
    assert_eq!(names(&loaded.entrants()), ["Alice", "Bob"]);
}

/// Changes taken before a person changed leave that change unsaved once they are written.
#[test]
fn a_person_changed_after_the_changes_were_taken_stays_unsaved() {
    let mut session = Session::new("cup");
    let alice = session.add_entrant("Alice").unwrap();
    let changes = session.unsaved_changes();
    session.people.add(Person::new(alice, "Alicia"));

    session.mark_saved(&changes);

    assert!(session.has_unsaved_changes());
}

/// Another tab, say, shows what the session changing the competition saved, without replaying the
/// log each time.
#[test]
fn a_session_catches_up_with_what_another_one_saved() {
    let storage = MemoryStorage::default();
    let mut changing = Given::new().started();
    now(changing.save(&storage)).unwrap();
    let mut following = now(Session::load(&storage, changing.key())).unwrap();

    changing
        .report(numbered(&changing, 1), MatchResult::AWins)
        .unwrap();
    changing
        .report(numbered(&changing, 2), MatchResult::BWins)
        .unwrap();
    // entries that undo an earlier one, which replay the log after it
    changing.undo_report(numbered(&changing, 1)).unwrap();
    changing.undo().unwrap();
    now(changing.save(&storage)).unwrap();

    assert_eq!(now(following.catch_up(&storage)).unwrap(), 4);
    assert_eq!(matches(&following), matches(&changing));
    assert_eq!(following.standings(), changing.standings());
    assert!(!following.has_unsaved_changes());
    assert!(matches!(following.undo(), Err(NothingToUndo)));
    assert_eq!(now(following.catch_up(&storage)).unwrap(), 0);
}

#[test]
fn catching_up_takes_the_people_as_stored() {
    let storage = MemoryStorage::default();
    let mut changing = Session::new("cup");
    changing.add_entrant("Alice").unwrap();
    now(changing.save(&storage)).unwrap();
    let mut following = now(Session::load(&storage, changing.key())).unwrap();

    changing.add_entrant("Bob").unwrap();
    now(changing.save(&storage)).unwrap();
    now(following.catch_up(&storage)).unwrap();

    assert_eq!(names(&following.entrants()), ["Alice", "Bob"]);
}

#[test]
fn an_entry_stored_that_does_not_apply_is_not_caught_up_with() {
    let storage = MemoryStorage::default();
    let mut changing = Session::new("cup");
    now(changing.save(&storage)).unwrap();
    let mut following = now(Session::load(&storage, changing.key())).unwrap();
    changing.add_entrant("Alice").unwrap();
    let mut changes = changing.unsaved_changes();
    changes.entries.push(r#"{"unknown": true}"#.to_owned());
    now(storage.write(&changes)).unwrap();

    assert!(matches!(
        now(following.catch_up(&storage)),
        Err(LoadError::Entry { position: 2, .. })
    ));
    assert!(following.entrants().is_empty(), "nothing is appended");
}

#[test]
#[should_panic = "a session catches up only with nothing unsaved"]
#[cfg_attr(target_arch = "wasm32", ignore = "WebAssembly aborts on a panic")]
fn a_session_with_unsaved_changes_does_not_catch_up() {
    let storage = MemoryStorage::default();
    let mut session = Session::new("cup");
    now(session.save(&storage)).unwrap();
    session.add_entrant("Alice").unwrap();

    _ = now(session.catch_up(&storage));
}

/// A storage whose writes fail as long as it is told to.
#[derive(Default)]
struct Flaky {
    storage: MemoryStorage,
    failing: Cell<bool>,
}

#[derive(Debug)]
struct WriteFailed;

impl fmt::Display for WriteFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the write failed")
    }
}

impl std::error::Error for WriteFailed {}

impl Storage for Flaky {
    type Error = WriteFailed;

    async fn list(&self) -> Result<Vec<Listing>, WriteFailed> {
        Ok(now(self.storage.list()).unwrap())
    }

    async fn read(
        &self,
        key: CompetitionKey,
        first_entry: usize,
    ) -> Result<Option<Stored>, WriteFailed> {
        Ok(now(self.storage.read(key, first_entry)).unwrap())
    }

    async fn write(&self, changes: &Changes) -> Result<(), WriteFailed> {
        if self.failing.get() {
            return Err(WriteFailed);
        }
        now(self.storage.write(changes)).unwrap();
        Ok(())
    }
}

#[test]
fn a_failed_save_leaves_the_changes_to_the_next_one() {
    let storage = Flaky::default();
    let mut session = Session::new("cup");
    now(session.save(&storage)).unwrap();
    session.add_entrant("Alice").unwrap();

    storage.failing.set(true);
    assert!(now(session.save(&storage)).is_err());
    assert!(session.has_unsaved_changes());
    storage.failing.set(false);
    session.add_entrant("Bob").unwrap();
    now(session.save(&storage)).unwrap();

    let loaded = now(Session::load(&storage, session.key())).unwrap();
    assert_eq!(names(&loaded.entrants()), ["Alice", "Bob"]);
}

/// The log knows the entrants by id only, so that their names can be deleted without touching the
/// log.
#[test]
fn names_are_stored_apart_from_the_log() {
    let storage = MemoryStorage::default();
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();

    now(session.save(&storage)).unwrap();

    let stored = now(storage.read(session.key(), 0)).unwrap().unwrap();
    assert!(stored.entries.iter().all(|entry| !entry.contains("Alice")));
    assert!(stored.people[0].contains("Alice"));
}

#[test]
fn an_exported_competition_is_imported_as_a_new_one_as_it_was() {
    let storage = MemoryStorage::default();
    let mut session = Given::new().started();
    session
        .report(numbered(&session, 1), MatchResult::AWins)
        .unwrap();

    let mut imported = Session::import(&session.export(), "other").unwrap();

    assert_eq!(imported.name(), "cup", "the file names the competition");
    assert_ne!(imported.key(), session.key());
    assert_eq!(imported.entrants(), session.entrants());
    assert_eq!(matches(&imported), matches(&session));
    assert_eq!(imported.standings(), session.standings());
    assert!(matches!(imported.undo(), Err(NothingToUndo)));
    assert!(imported.has_unsaved_changes(), "nothing of it is saved yet");
    now(imported.save(&storage)).unwrap();
    let loaded = now(Session::load(&storage, imported.key())).unwrap();
    assert_eq!(loaded.entrants(), session.entrants());
    assert_eq!(matches(&loaded), matches(&session));
}

/// The CLI saves the same file without the name.
#[test]
fn a_file_without_a_name_is_imported_under_the_name_given() {
    let session = Given::new().added();
    let mut file: serde_json::Value = serde_json::from_str(&session.export()).unwrap();
    file.as_object_mut().unwrap().remove("name");

    let imported = Session::import(&file.to_string(), "cup.json").unwrap();

    assert_eq!(imported.name(), "cup.json");
    assert_eq!(imported.entrants(), session.entrants());
}

#[test]
fn a_file_that_is_no_competition_is_not_imported() {
    assert!(Session::import("", "cup").is_err());
    assert!(Session::import(r#"{"people": []}"#, "cup").is_err());
    assert!(Session::import(r#"{"log": [{}], "people": []}"#, "cup").is_err());
}

/// A log written by a version with a plugin this one does not have, say, cannot be loaded, but the
/// operator can still keep a copy of it.
#[test]
fn what_is_stored_is_exported_whether_it_replays_or_not() {
    let storage = MemoryStorage::default();
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    let mut changes = session.unsaved_changes();
    changes.entries.push(r#"{"unknown": true}"#.to_owned());
    now(storage.write(&changes)).unwrap();
    assert!(now(Session::load(&storage, session.key())).is_err());

    let file = now(storage.read(session.key(), 0))
        .unwrap()
        .unwrap()
        .export();

    let exported: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(exported["name"], "cup");
    assert_eq!(exported["log"].as_array().unwrap().len(), 3);
    assert_eq!(exported["log"][2], serde_json::json!({ "unknown": true }));
    assert_eq!(exported["people"][0]["name"], "Alice");
    assert!(Session::import(&file, "cup").is_err());
}

#[test]
fn what_does_not_replay_into_a_competition_is_not_loaded() {
    let storage = MemoryStorage::default();
    let key = CompetitionKey::new(Uuid::from_u128(1));
    let write = |entries: &[&str]| {
        now(storage.write(&Changes {
            listing: Listing {
                key,
                name: "cup".to_owned(),
            },
            first_entry: 0,
            entries: entries.iter().map(|entry| (*entry).to_owned()).collect(),
            people: Vec::new(),
        }))
        .unwrap();
    };

    assert!(matches!(
        now(Session::load(&storage, key)),
        Err(LoadError::NotFound)
    ));
    write(&["{}"]);
    assert!(matches!(
        now(Session::load(&storage, key)),
        Err(LoadError::Entry { position: 0, .. })
    ));
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    let undo_of_nothing = format!(
        r#"{{"id":"{}","kind":{{"Undo":"{}"}}}}"#,
        Uuid::from_u128(2),
        Uuid::from_u128(3)
    );
    write(&[&session.unsaved_changes().entries()[0], &undo_of_nothing]);
    assert!(matches!(
        now(Session::load(&storage, key)),
        Err(LoadError::Replay(_))
    ));
}
