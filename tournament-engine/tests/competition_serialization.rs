//! Round trip tests for [`Competition`]: every state a competition can be in has to survive being
//! serialized and deserialized again, including everything hidden behind the trait objects of its
//! plugins (phase format, ranker, metrics calculators) and the metrics of its rankings.

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tournament_engine::{
    AddEntrant, AddPhase, Annotated, Competition, CompetitionStateMachine,
    CompetitionStateMachineExt, Entrant, EntrantId, Match, MatchId, MatchResult, PhaseData,
    PhaseId, PhasePluginsBuilder, Ranking, ReportMatchResult, Seed, SetCompetitionSeed,
    StartCompetition, TypeMapValue,
    plugins::{
        metrics_calculator::{
            Buchholz, BuchholzCalculator, BuchholzOfBuchholz, BuchholzOfBuchholzCalculator,
            LaterFirstLoss, MoreBuchholz, MoreBuchholzOfBuchholz, MorePoints, MoreSonnebornBerger,
            Points, PointsCalculator, RoundOfFirstLoss, RoundOfFirstLossCalculator,
            SonnebornBerger, SonnebornBergerCalculator, WinsDrawsLosses, WinsDrawsLossesCalculator,
        },
        phase_format::{KnockoutRound, SingleElimination},
    },
    undoable_state_machine::Transition,
    uuid::Uuid,
};

/// Everything random here is seeded, so that all runs use the exact same tournament.
const SEED: Seed = Seed::new(*b"round-trip!!");

/// The scoring system of most association football leagues.
const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

/// The classical Sonneborn-Berger weights.
const SONNEBORN_BERGER: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);

fn phase_id() -> PhaseId {
    PhaseId::new(Uuid::from_u128(0xfa5e))
}

/// A consumer annotation on an [`Entrant`], standing in for whatever a real consumer attaches.
/// Core never reads it, so all a round trip has to do is hand it back as the same concrete type
/// holding the same value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Nickname(String);

#[typetag::serde(name = "test::nickname")]
impl TypeMapValue for Nickname {}

/// Applies `transition` to a competition that is not held by a [`CompetitionStateMachine`], exactly
/// like [`StateMachine::apply`] would.
///
/// [`StateMachine::apply`]: undoable_state_machine::StateMachine::apply
#[track_caller]
fn apply_directly(competition: &mut Competition, transition: &impl Transition<Competition>) {
    transition
        .check_applicable(competition)
        .expect("transition must be applicable");
    transition
        .apply(competition)
        .expect("applying the transition must not fail");
}

/// A competition with four entrants and a single elimination phase that calculates every metric
/// this crate offers, not started yet.
fn competition_with_a_phase() -> CompetitionStateMachine {
    let mut sm = CompetitionStateMachine::default();
    sm.apply_transition(SetCompetitionSeed::new(SEED)).unwrap();
    for entrant in 0..4 {
        let id = EntrantId::new(Uuid::from_u128(entrant));
        let entrant = Entrant::new(id).with_annotation(Nickname(format!("entrant {entrant}")));
        sm.apply_transition(AddEntrant::new(entrant)).unwrap();
    }
    let plugins = PhasePluginsBuilder::new()
        .format(SingleElimination::new())
        .add_ranking(LaterFirstLoss)
        .add_ranking(MorePoints)
        .add_ranking(MoreBuchholz)
        .add_ranking(MoreSonnebornBerger)
        .add_ranking(MoreBuchholzOfBuchholz)
        .add_metric(RoundOfFirstLossCalculator)
        .add_metric(WinsDrawsLossesCalculator)
        .add_metric(THREE_ONE)
        .add_metric(BuchholzCalculator)
        .add_metric(SONNEBORN_BERGER)
        .add_metric(BuchholzOfBuchholzCalculator)
        .build()
        .unwrap();
    sm.apply_transition(AddPhase::new(phase_id(), plugins))
        .unwrap();
    sm
}

/// [`competition_with_a_phase`] started, so that its rankings, metrics and the matches of the
/// first round exist.
fn started_competition() -> CompetitionStateMachine {
    let mut sm = competition_with_a_phase();
    sm.apply_transition(StartCompetition).unwrap();
    sm
}

/// [`started_competition`] with both matches of the first round won by their team A, so that the
/// phase holds updated metrics, reranked rankings and the freshly scheduled final.
fn competition_in_the_final() -> CompetitionStateMachine {
    let mut sm = started_competition();
    for match_id in matches_of_round(sm.current().phases()[&phase_id()].data(), 1) {
        sm.apply_transition(ReportMatchResult::new(
            phase_id(),
            match_id,
            MatchResult::AWins,
        ))
        .unwrap();
    }
    sm
}

/// The matches of `round`, ordered by the initial unique rank of the first ranking of their team
/// A, which is unique within a round and unaffected by any reported result, so that the order does
/// not depend on the iteration order of the match map.
fn matches_of_round(phase: &PhaseData, round: u32) -> Vec<MatchId> {
    phase
        .matches()
        .values()
        .filter(|r#match| r#match.round().get() == round)
        .sorted_by_key(|r#match| phase.rankings()[&r#match.team_a()[0]].initial_unique_rank())
        .map(Match::id)
        .collect()
}

fn round_trip(competition: &Competition) -> Competition {
    let json = serde_json::to_string(competition).expect("serializing a competition must not fail");
    serde_json::from_str(&json).expect("a serialized competition must deserialize again")
}

/// A round trip through [`postcard`], a compact binary format that is not self-describing and, more
/// importantly here, not human readable.
fn binary_round_trip(competition: &Competition) -> Competition {
    let bytes = postcard::to_stdvec(competition).expect("serializing a competition must not fail");
    postcard::from_bytes(&bytes).expect("a serialized competition must deserialize again")
}

/// The serialized form of `competition`, compared as is.
///
/// Nothing about it needs normalizing first, which is worth being explicit about: an
/// [`IdMap`](tournament_engine::map::IdMap) and a
/// [`TypeMap`](tournament_engine::type_map::TypeMap) both serialize to a sequence in the iteration
/// order of a [`Map`](tournament_engine::map::Map), and that order is insertion order, so
/// serializing writes the values in the order they were inserted and deserializing inserts them in
/// the same one. Two competitions that hold the same state therefore serialize identically, however
/// each of them was built. A hash map would not manage that: its iteration order depends on how it
/// was filled and really does differ between a map that was filled entry by entry and one that was
/// filled from a serialized sequence, which is what makes the comparisons below possible at all.
fn serialized(competition: &Competition) -> Value {
    serde_json::to_value(competition).expect("serializing a competition must not fail")
}

/// Asserts that both rankings hold the same value of every metric calculated by the phase.
#[track_caller]
fn assert_same_metrics(left: &Ranking, right: &Ranking) {
    assert_eq!(
        left.metric::<RoundOfFirstLoss>(),
        right.metric::<RoundOfFirstLoss>()
    );
    assert_eq!(
        left.metric::<WinsDrawsLosses>(),
        right.metric::<WinsDrawsLosses>()
    );
    assert_eq!(left.metric::<Points>(), right.metric::<Points>());
    assert_eq!(left.played_matches(), right.played_matches());
    assert_eq!(left.metric::<Buchholz>(), right.metric::<Buchholz>());
    assert_eq!(
        left.metric::<SonnebornBerger>(),
        right.metric::<SonnebornBerger>()
    );
    assert_eq!(
        left.metric::<BuchholzOfBuchholz>(),
        right.metric::<BuchholzOfBuchholz>()
    );
}

/// Asserts that both entrants hold the same id and the same annotations.
///
/// [`Entrant`] holds a [`TypeMap`](tournament_engine::type_map::TypeMap), which cannot implement
/// [`PartialEq`] — a boxed annotation has no way to compare itself to another one — so the
/// comparison goes field by field, asking for each annotation by its type.
#[track_caller]
fn assert_same_entrant(left: &Entrant, right: &Entrant) {
    assert_eq!(left.id(), right.id());
    assert_eq!(
        left.annotation::<Nickname>(),
        right.annotation::<Nickname>()
    );
}

/// Asserts that both competitions hold the same state, first through the public getters, so that a
/// failure points at the part that differs, and then through their whole serialized form, which
/// also covers the private fields and the plugins.
#[track_caller]
fn assert_same_competition(left: &Competition, right: &Competition) {
    assert_eq!(left.is_started(), right.is_started());
    assert_eq!(left.seed(), right.seed());
    assert_eq!(left.entrants().len(), right.entrants().len());
    for entrant in left.entrants() {
        let other = right
            .entrants()
            .get(&entrant.id())
            .expect("the entrant must exist");
        assert_same_entrant(entrant, other);
    }
    assert_eq!(left.phases().len(), right.phases().len());
    for phase in left.phases() {
        let other = right
            .phases()
            .get(&phase.id())
            .expect("the phase must exist");
        assert_eq!(phase.id(), other.id());
        let (phase, other) = (phase.data(), other.data());
        assert_eq!(phase.seed(), other.seed());
        assert_eq!(phase.standings().len(), other.standings().len());
        for (ranking, other_ranking) in phase.standings().zip(other.standings()) {
            assert_eq!(ranking.id(), other_ranking.id());
            assert_eq!(ranking.entrants(), other_ranking.entrants());
            assert_eq!(ranking.rank(), other_ranking.rank());
            assert_eq!(ranking.unique_rank(), other_ranking.unique_rank());
            assert_eq!(
                ranking.initial_unique_rank(),
                other_ranking.initial_unique_rank()
            );
            assert_same_metrics(ranking, other_ranking);
        }
        assert_eq!(phase.matches().len(), other.matches().len());
        for r#match in phase.matches() {
            let other_match = other
                .matches()
                .get(&r#match.id())
                .expect("the match must exist");
            assert_eq!(r#match.team_a(), other_match.team_a());
            assert_eq!(r#match.team_b(), other_match.team_b());
            assert_eq!(r#match.round(), other_match.round());
            assert_eq!(r#match.result(), other_match.result());
        }
        assert_eq!(phase.byes().len(), other.byes().len());
        for bye in phase.byes() {
            let other_bye = other.byes().get(&bye.id()).expect("the bye must exist");
            assert_eq!(bye.ranking(), other_bye.ranking());
            assert_eq!(bye.round(), other_bye.round());
        }
    }
    assert_eq!(serialized(left), serialized(right));
}

#[test]
fn an_empty_competition_round_trips() {
    let competition = round_trip(&Competition::default());

    assert!(!competition.is_started());
    assert!(competition.entrants().is_empty());
    assert!(competition.phases().is_empty());
    assert_eq!(competition.seed(), Seed::default());
    assert_same_competition(&Competition::default(), &competition);
}

#[test]
fn a_competition_that_has_not_started_round_trips() {
    let sm = competition_with_a_phase();

    let competition = round_trip(sm.current());

    assert!(!competition.is_started());
    assert_eq!(competition.seed(), SEED);
    assert_eq!(competition.entrants().len(), 4);
    let phase = competition.phases()[&phase_id()].data();
    assert!(phase.rankings().is_empty());
    assert!(phase.matches().is_empty());
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_started_competition_round_trips() {
    let sm = started_competition();

    let competition = round_trip(sm.current());

    assert!(competition.is_started());
    let phase = competition.phases()[&phase_id()].data();
    assert_eq!(phase.rankings().len(), 4);
    assert_eq!(phase.matches().len(), 2);
    // the metrics of a started competition are all at their default value, so this only shows
    // that they exist at all
    for ranking in phase.standings() {
        assert_eq!(ranking.metric::<Points>(), Some(&Points(0.0)));
    }
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_partially_played_competition_round_trips() {
    let sm = competition_in_the_final();

    let competition = round_trip(sm.current());

    let phase = competition.phases()[&phase_id()].data();
    // the two first round matches plus the final that reporting them scheduled
    assert_eq!(phase.matches().len(), 3);
    assert_eq!(
        phase
            .matches()
            .values()
            .filter(|r#match| r#match.is_finished())
            .count(),
        2
    );
    // the two winners hold the points of their win, and the losers the round they lost in
    let winners = phase
        .rankings()
        .values()
        .filter(|ranking| ranking.metric::<Points>() == Some(&Points(3.0)))
        .count();
    assert_eq!(winners, 2);
    assert_same_competition(sm.current(), &competition);
}

/// A competition must survive a format whose values look nothing like those of a human readable
/// one, which is what everything inside the plugin trait objects is easy to get wrong: `typetag`
/// serializes the content of an internally tagged trait object through an adapter that reports
/// itself as human readable no matter what the real format says, so an id below that content — like
/// the [`MatchId`]s in the bracket the knockout format keeps — would be written as a string and
/// read back as raw bytes. All plugin traits are therefore adjacently tagged, which keeps the
/// content out of that adapter.
#[test]
fn a_played_competition_round_trips_through_a_binary_format() {
    let sm = competition_in_the_final();
    // the bracket holds the ids of the matches, which a plain round trip would silently write in
    // the wrong representation
    assert!(
        !sm.current().phases()[&phase_id()]
            .data()
            .matches()
            .is_empty()
    );

    let competition = binary_round_trip(sm.current());

    assert_same_competition(sm.current(), &competition);
}

/// Three entrants leave the best seed with a bye in the semifinals, which has to survive both
/// formats along with the annotation it carries.
#[test]
fn a_competition_with_a_bye_round_trips() {
    let mut sm = CompetitionStateMachine::default();
    sm.apply_transition(SetCompetitionSeed::new(SEED)).unwrap();
    for entrant in 0..3 {
        let entrant = Entrant::new(EntrantId::new(Uuid::from_u128(entrant)));
        sm.apply_transition(AddEntrant::new(entrant)).unwrap();
    }
    let plugins = PhasePluginsBuilder::new()
        .format(SingleElimination::new())
        .add_metric(WinsDrawsLossesCalculator)
        .build()
        .unwrap();
    sm.apply_transition(AddPhase::new(phase_id(), plugins))
        .unwrap();
    sm.apply_transition(StartCompetition).unwrap();
    let bye = sm.current().phases()[&phase_id()]
        .data()
        .byes()
        .values()
        .exactly_one()
        .unwrap();
    assert_eq!(
        bye.annotation::<KnockoutRound>(),
        Some(&KnockoutRound::RoundOf(4))
    );

    for competition in [round_trip(sm.current()), binary_round_trip(sm.current())] {
        assert_same_competition(sm.current(), &competition);
    }
}

#[test]
fn a_round_tripped_competition_can_be_started() {
    let mut sm = competition_with_a_phase();
    let mut competition = round_trip(sm.current());

    // the phase format, the ranker and all metrics calculators are only used from here on, so
    // this is what shows that they survived the round trip as their concrete types
    sm.apply_transition(StartCompetition).unwrap();
    apply_directly(&mut competition, &StartCompetition);

    assert_eq!(competition.phases()[&phase_id()].data().matches().len(), 2);
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_round_tripped_competition_can_continue_the_tournament() {
    let mut sm = competition_in_the_final();
    let mut competition = round_trip(sm.current());
    let final_match = matches_of_round(competition.phases()[&phase_id()].data(), 2)[0];
    let report = ReportMatchResult::new(phase_id(), final_match, MatchResult::BWins);

    sm.apply_transition(ReportMatchResult::new(
        phase_id(),
        final_match,
        MatchResult::BWins,
    ))
    .unwrap();
    apply_directly(&mut competition, &report);

    let phase = competition.phases()[&phase_id()].data();
    assert!(phase.matches().values().all(Match::is_finished));
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_round_trip_does_not_change_the_serialized_form() {
    let sm = competition_in_the_final();

    let competition = round_trip(&round_trip(sm.current()));

    assert_eq!(serialized(&competition), serialized(sm.current()));
}

#[test]
fn the_competition_seed_survives_a_round_trip() {
    let sm = started_competition();

    let competition = round_trip(sm.current());

    assert_eq!(competition.seed(), SEED);
}

#[test]
fn deserializing_a_competition_with_an_unknown_phase_format_fails() {
    let sm = started_competition();
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for phase in value["phases"].as_array_mut().unwrap() {
        phase["phase_format"]["type"] = Value::String("NotAPhaseFormat".to_owned());
    }

    let Err(error) = serde_json::from_value::<Competition>(value) else {
        panic!("an unknown phase format must not deserialize");
    };

    assert!(
        error.to_string().contains("NotAPhaseFormat"),
        "the error must name the unknown phase format, but was: {error}"
    );
}

#[test]
fn deserializing_a_competition_with_an_unknown_metric_fails() {
    let sm = competition_in_the_final();
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for phase in value["phases"].as_array_mut().unwrap() {
        for ranking in phase["data"]["rankings"].as_array_mut().unwrap() {
            ranking["metrics"][0]["type"] = Value::String("NotAMetric".to_owned());
        }
    }

    let Err(error) = serde_json::from_value::<Competition>(value) else {
        panic!("an unknown metric must not deserialize");
    };

    assert!(
        error.to_string().contains("NotAMetric"),
        "the error must name the unknown metric, but was: {error}"
    );
}
