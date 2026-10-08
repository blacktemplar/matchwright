//! Round trip tests for the transition log: a [`CompetitionStateMachine`] serializes as its log,
//! so every transition this crate ships has to survive being written and read back as a
//! `Box<dyn CompetitionTransition>`, and the restored log has to replay into exactly the
//! competition it was written from.
//!
//! This is the second persistence path, next to the snapshot of a [`Competition`] alone covered by
//! `competition_serialization.rs`. It is the richer of the two: a snapshot holds the current state,
//! while a log also holds how that state came about, which is what keeps undo — and undoing an
//! undo — available after a restore.

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tournament_engine::{
    entities::competition::{
        Competition, Entrant, EntrantId, Match, MatchId, MatchResult, Phase, PhaseId,
    },
    plugins::{
        metrics_calculator::{
            buchholz::BuchholzCalculator,
            matches::MatchesCalculator,
            points::{MorePoints, Points, PointsCalculator},
            round_of_first_loss::{LaterFirstLoss, RoundOfFirstLossCalculator},
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
        phase_format::single_elimination::SingleElimination,
        ranker::{HierarchicalRanker, LexicographicalCriteria},
    },
    transitions::{
        CompetitionStateMachine, CompetitionTransition, add_entrant::AddEntrant,
        add_phase::AddPhase, remove_entrant::RemoveEntrant, report_match_result::ReportMatchResult,
        set_competition_seed::SetCompetitionSeed, start_competition::StartCompetition,
    },
    type_map::TypeMapValue,
};
use undoable_state_machine::{LogEntry, LogEntryKind, TransitionId};
use uuid::Uuid;

/// Everything random here is seeded, so that all runs use the exact same tournament.
const SEED: [u8; 12] = *b"log-trip!!!!";

/// The scoring system of most association football leagues.
const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

fn phase_id() -> PhaseId {
    PhaseId::new(Uuid::from_u128(0xfa5e))
}

fn entrant_id(entrant: u128) -> EntrantId {
    EntrantId::new(Uuid::from_u128(entrant))
}

/// A consumer annotation on an [`Entrant`], standing in for whatever a real consumer attaches. It
/// reaches the competition only through the [`AddEntrant`] that carries it, so a log that loses it
/// cannot replay into the same competition — which is what makes it worth having here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Nickname(String);

#[typetag::serde(name = "test::nickname")]
impl TypeMapValue for Nickname {}

/// Applies `transition` and returns the id of the log entry it was applied as, so that a later
/// entry can undo it.
#[track_caller]
fn apply(
    machine: &mut CompetitionStateMachine,
    transition: impl CompetitionTransition + 'static,
) -> TransitionId {
    let id = TransitionId::new();
    machine
        .apply(LogEntry::new(id, LogEntryKind::Apply(Box::new(transition))))
        .expect("transition must be applicable");
    id
}

#[track_caller]
fn undo(machine: &mut CompetitionStateMachine, target: TransitionId) -> TransitionId {
    let id = TransitionId::new();
    machine
        .apply(LogEntry::new(id, LogEntryKind::Undo(target)))
        .expect("the undo target must exist");
    id
}

/// A competition that was seeded, given five entrants of which one was removed again, given a
/// single elimination phase, started, and played up to its final — so its log holds at least one of
/// every transition this crate ships.
fn played_competition() -> CompetitionStateMachine {
    let mut machine = CompetitionStateMachine::default();

    apply(&mut machine, SetCompetitionSeed::new(SEED));
    for entrant in 0..5 {
        let entrant = Entrant::new(entrant_id(entrant))
            .with_annotation(Nickname(format!("entrant {entrant}")));
        apply(&mut machine, AddEntrant::new(entrant));
    }
    apply(
        &mut machine,
        RemoveEntrant::new(Entrant::new(entrant_id(4))),
    );
    let ranker = HierarchicalRanker::new(vec![Box::new(
        LexicographicalCriteria::new()
            .add_criterion(LaterFirstLoss)
            .add_criterion(MorePoints),
    )]);
    apply(
        &mut machine,
        AddPhase::new(
            phase_id(),
            Box::new(SingleElimination::new()),
            Box::new(ranker),
            // the calculators run in this order, so every one of them that reads the metric of
            // another one has to be listed after it
            vec![
                Box::new(RoundOfFirstLossCalculator),
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE),
                Box::new(MatchesCalculator),
                Box::new(BuchholzCalculator),
            ],
        )
        .unwrap(),
    );
    apply(&mut machine, StartCompetition);
    for match_id in matches_of_round(&machine.current().phases()[&phase_id()], 1) {
        apply(
            &mut machine,
            ReportMatchResult::new(phase_id(), match_id, MatchResult::AWins),
        );
    }

    machine
}

/// The matches of `round`, ordered by the initial unique rank of the first ranking of their team A,
/// which is unique within a round and unaffected by any reported result, so that the order does not
/// depend on the iteration order of the match map.
fn matches_of_round(phase: &Phase, round: u32) -> Vec<MatchId> {
    phase
        .matches()
        .values()
        .filter(|r#match| r#match.round().get() == round)
        .sorted_by_key(|r#match| phase.rankings()[&r#match.team_a()[0]].initial_unique_rank())
        .map(Match::id)
        .collect()
}

fn round_trip(machine: &CompetitionStateMachine) -> CompetitionStateMachine {
    let json = serde_json::to_string(machine).expect("serializing a log must not fail");
    serde_json::from_str(&json).expect("a serialized log must deserialize again")
}

/// A round trip through [`postcard`], a compact binary format that is not self-describing and, more
/// importantly here, not human readable.
fn binary_round_trip(machine: &CompetitionStateMachine) -> CompetitionStateMachine {
    let bytes = postcard::to_stdvec(machine).expect("serializing a log must not fail");
    postcard::from_bytes(&bytes).expect("a serialized log must deserialize again")
}

fn serialized_log(machine: &CompetitionStateMachine) -> Value {
    serde_json::to_value(machine).expect("serializing a log must not fail")
}

/// The serialized form of a competition, compared as is — see `competition_serialization.rs` for
/// why two competitions holding the same state serialize identically however each was built.
fn serialized(competition: &Competition) -> Value {
    serde_json::to_value(competition).expect("serializing a competition must not fail")
}

/// Asserts that `machine` holds `expected` as its current state, first through a few public getters
/// so that a failure points at the part that differs, and then through the whole serialized form,
/// which also covers the private fields, the plugins and the metrics.
#[track_caller]
fn assert_current_is(machine: &CompetitionStateMachine, expected: &Competition) {
    let current = machine.current();

    assert_eq!(current.is_started(), expected.is_started());
    assert_eq!(current.seed(), expected.seed());
    assert_eq!(current.entrants().len(), expected.entrants().len());
    assert_eq!(current.phases().len(), expected.phases().len());
    assert_eq!(serialized(current), serialized(expected));
}

/// A tag is part of the serialized form of every log ever written, so a change to it is a breaking
/// one and has to be deliberate. Pinning every tag here makes a rename or a move of a transition
/// fail this test instead of silently invalidating old logs.
///
/// Asking for the tags in order of first appearance also asserts that the log below really does
/// exercise all six transitions.
#[test]
fn every_transition_is_serialized_under_its_explicitly_chosen_tag() {
    let machine = played_competition();

    let Value::Array(entries) = serialized_log(&machine) else {
        panic!("a log must serialize to a sequence");
    };

    let tags = entries
        .iter()
        .map(|entry| {
            entry["kind"]["Apply"]["type"]
                .as_str()
                .expect("every applied transition must carry a tag")
        })
        .unique()
        .collect_vec();
    assert_eq!(
        tags,
        [
            "tournament_engine::set_competition_seed",
            "tournament_engine::add_entrant",
            "tournament_engine::remove_entrant",
            "tournament_engine::add_phase",
            "tournament_engine::start_competition",
            "tournament_engine::report_match_result",
        ]
    );
}

/// Adjacent tagging puts the payload of a transition beside its tag rather than inside it, which is
/// what keeps a binary format working — see the test further down.
#[test]
fn a_transition_is_serialized_with_its_payload_next_to_its_tag() {
    let mut machine = CompetitionStateMachine::default();
    let seed_id = apply(&mut machine, SetCompetitionSeed::new(SEED));

    let log = serialized_log(&machine);

    assert_eq!(
        log,
        json!([{
            "id": seed_id,
            "kind": {
                "Apply": {
                    "type": "tournament_engine::set_competition_seed",
                    "value": { "seed": SEED },
                },
            },
        }])
    );
}

/// A transition without any data of its own still carries its tag, which is all that identifies it.
#[test]
fn a_transition_without_data_is_serialized_as_its_tag_alone() {
    let machine = played_competition();

    let Value::Array(entries) = serialized_log(&machine) else {
        panic!("a log must serialize to a sequence");
    };

    let starts = entries
        .iter()
        .filter(|entry| entry["kind"]["Apply"]["type"] == "tournament_engine::start_competition")
        .collect_vec();
    assert_eq!(starts.len(), 1, "the log must hold exactly one start");
    assert_eq!(
        starts[0]["kind"]["Apply"],
        json!({ "type": "tournament_engine::start_competition", "value": null })
    );
}

#[test]
fn an_empty_machine_round_trips() {
    let machine = round_trip(&CompetitionStateMachine::default());

    assert!(machine.log().is_empty());
    assert_current_is(&machine, &Competition::default());
}

/// The whole point of the log path: a competition can be rebuilt from nothing but its transitions.
/// Deserializing replays them, which also needs every plugin inside the round-tripped [`AddPhase`]
/// back as its concrete type, since starting the phase and reporting a result is what runs them.
#[test]
fn a_log_round_trips_and_replays_into_the_same_competition() {
    let machine = played_competition();

    let restored = round_trip(&machine);

    assert_eq!(restored.log().len(), machine.log().len());
    assert_current_is(&restored, machine.current());
}

/// A log must survive a format whose values look nothing like those of a human readable one, which
/// is easy to get wrong for everything below a trait object: `typetag` serializes the content of an
/// *internally* tagged trait object through an adapter that reports itself as human readable no
/// matter what the real format says, so a [`Uuid`] below that content — the entrant ids of an
/// [`AddEntrant`], the match id of a [`ReportMatchResult`] — would be written as a string and read
/// back as raw bytes. [`CompetitionTransition`] is therefore adjacently tagged, like every plugin
/// trait, which keeps the content out of that adapter.
#[test]
fn a_log_round_trips_through_a_binary_format() {
    let machine = played_competition();

    let restored = binary_round_trip(&machine);

    assert_current_is(&restored, machine.current());
}

#[test]
fn a_round_trip_does_not_change_the_serialized_form_of_a_log() {
    let machine = played_competition();
    let expected = serialized_log(&machine);

    let restored = round_trip(&round_trip(&machine));

    assert_eq!(serialized_log(&restored), expected);
}

/// Consumer annotations are only ever in the log, never derivable from state, so this is the one
/// thing a round trip of an [`AddEntrant`] cannot afford to drop.
#[test]
fn a_round_tripped_log_still_carries_its_consumer_annotations() {
    let machine = round_trip(&played_competition());

    let competition = machine.current();

    assert_eq!(competition.entrants().len(), 4);
    // entrant 4 was removed again, so only the first four are left
    for entrant in 0..4 {
        let found = competition
            .entrants()
            .get(&entrant_id(entrant))
            .expect("the entrant must exist");
        assert_eq!(
            found.annotation::<Nickname>(),
            Some(&Nickname(format!("entrant {entrant}")))
        );
    }
}

/// What a snapshot of the current state cannot do: the restored machine still knows the history, so
/// a result reported before the restore can be taken back after it.
#[test]
fn a_round_tripped_log_can_still_be_undone() {
    let machine = played_competition();
    let last_result = machine
        .log()
        .last()
        .expect("the log must hold a result")
        .id();
    let mut restored = round_trip(&machine);

    undo(&mut restored, last_result);

    // one of the two first round results is gone, so the final it scheduled is gone with it and
    // only one ranking holds the points of a win
    let phase = &restored.current().phases()[&phase_id()];
    assert_eq!(phase.matches().len(), 2);
    assert_eq!(
        phase
            .rankings()
            .values()
            .filter(|ranking| ranking.metric::<Points>() == Some(&Points(3.0)))
            .count(),
        1
    );
}

/// An undo is itself a log entry, so it has to round trip like any other — and stay undoable, which
/// is what a redo is.
#[test]
fn an_undo_survives_a_round_trip_and_can_itself_be_undone() {
    let mut machine = played_competition();
    let played = serialized(machine.current());
    let last_result = machine
        .log()
        .last()
        .expect("the log must hold a result")
        .id();
    let undo_id = undo(&mut machine, last_result);
    let undone = serialized(machine.current());
    assert_ne!(undone, played, "the undo must have changed the state");

    let mut restored = round_trip(&machine);

    // the undo came back as an undo, not as a shorter log
    assert_eq!(restored.log().len(), machine.log().len());
    assert_eq!(serialized(restored.current()), undone);
    undo(&mut restored, undo_id);
    assert_eq!(serialized(restored.current()), played);
}

#[test]
fn a_round_tripped_log_can_continue_the_tournament() {
    let mut machine = round_trip(&played_competition());
    let final_match = matches_of_round(&machine.current().phases()[&phase_id()], 2)[0];

    apply(
        &mut machine,
        ReportMatchResult::new(phase_id(), final_match, MatchResult::BWins),
    );

    let phase = &machine.current().phases()[&phase_id()];
    assert!(phase.matches().values().all(Match::is_finished));
}

#[test]
fn deserializing_a_log_with_an_unknown_transition_fails() {
    let machine = played_competition();
    let mut log = serialized_log(&machine);
    log[0]["kind"]["Apply"]["type"] = Value::String("NotATransition".to_owned());

    let Err(error) = serde_json::from_value::<CompetitionStateMachine>(log) else {
        panic!("an unknown transition must not deserialize");
    };

    assert!(
        error.to_string().contains("NotATransition"),
        "the error must name the unknown transition, but was: {error}"
    );
}

/// A log is replayed against a fresh competition, so a transition that was applicable when it was
/// written is not trusted to be applicable again: `check_applicable` is re-run on every entry while
/// deserializing, reaching the transition through the blanket `Transition` impl of a box.
#[test]
fn deserializing_a_log_whose_transitions_do_not_apply_fails() {
    let machine = played_competition();
    let mut log = serialized_log(&machine);
    let Value::Array(entries) = &mut log else {
        panic!("a log must serialize to a sequence");
    };
    // drop the phase, which the start below it and every reported result after that one need
    entries.retain(|entry| entry["kind"]["Apply"]["type"] != "tournament_engine::add_phase");

    let Err(error) = serde_json::from_value::<CompetitionStateMachine>(log) else {
        panic!("a log that does not replay must not deserialize");
    };

    // the start is the first entry the missing phase makes inapplicable, so replaying stops there
    // rather than carrying on to the results that would also have failed
    assert!(
        error.to_string().contains("no phases"),
        "the error must carry the reason the log stopped replaying, but was: {error}"
    );
}
