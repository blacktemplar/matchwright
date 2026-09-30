//! Round trip tests for [`Competition`]: every state a competition can be in has to survive being
//! serialized and deserialized again, including everything hidden behind the trait objects of its
//! plugins (phase format, ranker, metrics calculators) and the metrics of its rankings.

use itertools::Itertools;
use rand::RngExt;
use serde_json::{Value, json};
use tournament_engine::{
    entities::{
        competition::{Competition, Entrant, EntrantId},
        phase::{Match, MatchId, MatchResult, Phase, PhaseId},
        rankings::RankingView,
    },
    plugins::{
        metrics_calculator::{
            buchholz::{
                Buchholz, BuchholzCalculator, BuchholzComparatorProvider, BuchholzOfBuchholz,
                BuchholzOfBuchholzCalculator, BuchholzOfBuchholzComparatorProvider,
            },
            matches::{Matches, MatchesCalculator},
            points::{Points, PointsCalculator, PointsComparatorProvider},
            round_of_first_loss::{
                RoundOfFirstLoss, RoundOfFirstLossCalculator, RoundOfFirstLossComparatorProvider,
            },
            sonneborn_berger::{
                SonnebornBerger, SonnebornBergerCalculator, SonnebornBergerComparatorProvider,
            },
            wins_draws_losses::{WinsDrawsLosses, WinsDrawsLossesCalculator},
        },
        phase_format::single_elimination::SingleElimination,
        ranker::{HierarchicalRanker, LexicographicalMetrics},
    },
    transitions::{
        add_entrant::AddEntrant, add_phase::AddPhase, report_match_result::ReportMatchResult,
        set_competition_seed::SetCompetitionSeed, start_competition::StartCompetition,
    },
};
use undoable_state_machine::{LogEntry, LogEntryKind, StateMachine, Transition, TransitionId};
use uuid::Uuid;

type CompetitionStateMachine = StateMachine<Competition, Box<dyn Transition<Competition>>>;

/// Everything random here is seeded, so that all runs use the exact same tournament.
const SEED: [u8; 12] = *b"round-trip!!";

/// The scoring system of most association football leagues.
const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

/// The classical Sonneborn-Berger weights.
const SONNEBORN_BERGER: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);

fn phase_id() -> PhaseId {
    PhaseId::new(Uuid::from_u128(0xfa5e))
}

#[track_caller]
fn apply(sm: &mut CompetitionStateMachine, transition: impl Transition<Competition> + 'static) {
    sm.apply(LogEntry::new(
        TransitionId::new(),
        LogEntryKind::Apply(Box::new(transition)),
    ))
    .expect("transition must be applicable");
}

/// Applies `transition` to a competition that is not held by a [`StateMachine`], exactly like
/// [`StateMachine::apply`] would.
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
    apply(&mut sm, SetCompetitionSeed::new(SEED));
    for entrant in 0..4 {
        let id = EntrantId::new(Uuid::from_u128(entrant));
        apply(&mut sm, AddEntrant::new(Entrant::new(id)));
    }
    let ranker = HierarchicalRanker::new(vec![Box::new(
        LexicographicalMetrics::new()
            .add_metric(RoundOfFirstLossComparatorProvider)
            .add_metric(PointsComparatorProvider)
            .add_metric(BuchholzComparatorProvider)
            .add_metric(SonnebornBergerComparatorProvider)
            .add_metric(BuchholzOfBuchholzComparatorProvider),
    )]);
    apply(
        &mut sm,
        AddPhase::new(
            phase_id(),
            Box::new(SingleElimination),
            Box::new(ranker),
            // the calculators run in this order, so every one of them that reads the metric of
            // another one has to be listed after it
            vec![
                Box::new(RoundOfFirstLossCalculator),
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE),
                Box::new(MatchesCalculator),
                Box::new(BuchholzCalculator),
                Box::new(SONNEBORN_BERGER),
                Box::new(BuchholzOfBuchholzCalculator),
            ],
        ),
    );
    sm
}

/// [`competition_with_a_phase`] started, so that its rankings, metrics and the matches of the
/// first round exist.
fn started_competition() -> CompetitionStateMachine {
    let mut sm = competition_with_a_phase();
    apply(&mut sm, StartCompetition);
    sm
}

/// [`started_competition`] with both matches of the first round won by their team A, so that the
/// phase holds updated metrics, reranked rankings and the freshly scheduled final.
fn competition_in_the_final() -> CompetitionStateMachine {
    let mut sm = started_competition();
    for match_id in matches_of_round(&sm.current().phases()[&phase_id()], 1) {
        apply(
            &mut sm,
            ReportMatchResult::new(phase_id(), match_id, MatchResult::AWins),
        );
    }
    sm
}

/// The matches of `round`, ordered by the initial unique rank of the first ranking of their team
/// A, which is unique within a round and unaffected by any reported result, so that the order does
/// not depend on the iteration order of the match map.
fn matches_of_round(phase: &Phase, round: u32) -> Vec<MatchId> {
    phase
        .matches()
        .values()
        .filter(|r#match| r#match.round().get() == round)
        .sorted_by_key(|r#match| {
            phase
                .ranking_view(&r#match.team_a()[0])
                .initial_unique_rank()
        })
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
fn assert_same_metrics(left: RankingView<'_>, right: RankingView<'_>) {
    assert_eq!(
        left.metric::<RoundOfFirstLoss>(),
        right.metric::<RoundOfFirstLoss>()
    );
    assert_eq!(
        left.metric::<WinsDrawsLosses>(),
        right.metric::<WinsDrawsLosses>()
    );
    assert_eq!(left.metric::<Points>(), right.metric::<Points>());
    assert_eq!(left.metric::<Matches>(), right.metric::<Matches>());
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

/// Asserts that both competitions hold the same state, first through the public getters, so that a
/// failure points at the part that differs, and then through their whole serialized form, which
/// also covers the private fields and the plugins.
#[track_caller]
fn assert_same_competition(left: &Competition, right: &Competition) {
    assert_eq!(left.is_started(), right.is_started());
    assert_eq!(left.seed(), right.seed());
    assert_eq!(left.entrants().len(), right.entrants().len());
    for entrant in left.entrants() {
        assert_eq!(Some(entrant), right.entrants().get(&entrant.id()));
    }
    assert_eq!(left.phases().len(), right.phases().len());
    for phase in left.phases() {
        let other = right
            .phases()
            .get(&phase.id())
            .expect("the phase must exist");
        assert_eq!(phase.id(), other.id());
        assert_eq!(
            phase.ranking_ids_ordered_by_unique_rank(),
            other.ranking_ids_ordered_by_unique_rank()
        );
        for ranking_id in phase.ranking_ids_ordered_by_unique_rank() {
            let ranking = phase.ranking_view(ranking_id);
            let other_ranking = other.ranking_view(ranking_id);
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
        // the phase seed is private and only observable through the random streams it produces
        assert_eq!(
            phase.new_seeded_std_rng("entropy").random::<u64>(),
            other.new_seeded_std_rng("entropy").random::<u64>()
        );
    }
    assert_eq!(serialized(left), serialized(right));
}

#[test]
fn an_empty_competition_round_trips() {
    let competition = round_trip(&Competition::default());

    assert!(!competition.is_started());
    assert!(competition.entrants().is_empty());
    assert!(competition.phases().is_empty());
    assert_eq!(competition.seed(), [0; 12]);
    assert_same_competition(&Competition::default(), &competition);
}

#[test]
fn a_competition_that_has_not_started_round_trips() {
    let sm = competition_with_a_phase();

    let competition = round_trip(sm.current());

    assert!(!competition.is_started());
    assert_eq!(competition.seed(), SEED);
    assert_eq!(competition.entrants().len(), 4);
    let phase = &competition.phases()[&phase_id()];
    assert_eq!(phase.ranking_views().len(), 0);
    assert!(phase.matches().is_empty());
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_started_competition_round_trips() {
    let sm = started_competition();

    let competition = round_trip(sm.current());

    assert!(competition.is_started());
    let phase = &competition.phases()[&phase_id()];
    assert_eq!(phase.ranking_views().len(), 4);
    assert_eq!(phase.matches().len(), 2);
    // the metrics of a started competition are all at their default value, so this only shows
    // that they exist at all
    for ranking_id in phase.ranking_ids_ordered_by_unique_rank() {
        assert_eq!(phase.metric_of::<Points>(ranking_id), &Points(0.0));
    }
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_partially_played_competition_round_trips() {
    let sm = competition_in_the_final();

    let competition = round_trip(sm.current());

    let phase = &competition.phases()[&phase_id()];
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
        .ranking_views()
        .filter(|ranking| ranking.metric::<Points>() == &Points(3.0))
        .count();
    assert_eq!(winners, 2);
    assert_same_competition(sm.current(), &competition);
}

/// A competition must survive a format whose values look nothing like those of a human readable
/// one, which is what everything inside the plugin trait objects is easy to get wrong: `typetag`
/// serializes the content of an internally tagged trait object through an adapter that reports
/// itself as human readable no matter what the real format says, so an id below that content — like
/// the [`MatchId`]s of a [`Matches`] metric — would be written as a string and read back as raw
/// bytes. All plugin traits are therefore adjacently tagged, which keeps the content out of that
/// adapter.
#[test]
fn a_played_competition_round_trips_through_a_binary_format() {
    let sm = competition_in_the_final();
    // the match ids that a plain round trip would silently write in the wrong representation
    let phase = &sm.current().phases()[&phase_id()];
    assert!(
        phase
            .ranking_views()
            .any(|ranking| !ranking.metric::<Matches>().0.is_empty())
    );

    let competition = binary_round_trip(sm.current());

    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_round_tripped_competition_can_be_started() {
    let mut sm = competition_with_a_phase();
    let mut competition = round_trip(sm.current());

    // the phase format, the ranker and all metrics calculators are only used from here on, so
    // this is what shows that they survived the round trip as their concrete types
    apply(&mut sm, StartCompetition);
    apply_directly(&mut competition, &StartCompetition);

    assert_eq!(competition.phases()[&phase_id()].matches().len(), 2);
    assert_same_competition(sm.current(), &competition);
}

#[test]
fn a_round_tripped_competition_can_continue_the_tournament() {
    let mut sm = competition_in_the_final();
    let mut competition = round_trip(sm.current());
    let final_match = matches_of_round(&competition.phases()[&phase_id()], 2)[0];
    let report = ReportMatchResult::new(phase_id(), final_match, MatchResult::BWins);

    apply(
        &mut sm,
        ReportMatchResult::new(phase_id(), final_match, MatchResult::BWins),
    );
    apply_directly(&mut competition, &report);

    let phase = &competition.phases()[&phase_id()];
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
    assert_eq!(
        competition.new_seeded_std_rng("entropy").random::<u64>(),
        sm.current().new_seeded_std_rng("entropy").random::<u64>()
    );
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

/// The positions of the three elements a [`Rankings`](tournament_engine::entities::rankings::
/// Rankings) is serialized as. It is a tuple rather than a struct so that the metrics calculators
/// can only arrive before the rankings whose metrics are read as the types those calculators
/// describe.
const CALCULATORS: usize = 0;
const RANKINGS: usize = 1;

/// The positions of the metrics of a ranking, which are the positions of the calculators of the
/// phase in [`competition_with_a_phase`]. A metric is serialized as a bare value, so its position
/// is all that says which metric it is.
const WINS_DRAWS_LOSSES: usize = 1;
const BUCHHOLZ: usize = 4;
const SONNEBORN_BERGER_METRIC: usize = 5;

/// The serialized rankings of every phase of `competition`.
fn serialized_rankings(competition: &mut Value) -> impl Iterator<Item = &mut Value> {
    competition["phases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|phase| phase["rankings"][RANKINGS].as_array_mut().unwrap())
}

/// The metrics of a phase are not tagged with their type — the calculator at their position is
/// what says what they are — so an unknown calculator is what an unknown metric used to be.
#[test]
fn deserializing_a_competition_with_an_unknown_metrics_calculator_fails() {
    let sm = competition_in_the_final();
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for phase in value["phases"].as_array_mut().unwrap() {
        phase["rankings"][CALCULATORS][0]["type"] =
            Value::String("NotAMetricsCalculator".to_owned());
    }

    let Err(error) = serde_json::from_value::<Competition>(value) else {
        panic!("an unknown metrics calculator must not deserialize");
    };

    assert!(
        error.to_string().contains("NotAMetricsCalculator"),
        "the error must name the unknown metrics calculator, but was: {error}"
    );
}

/// A phase reads its metrics back by the position the layout of its calculators gives them, and
/// casts them to the type that layout says they have without checking. Reading a metric as the
/// type of the calculator at its position is what keeps that sound, so a value that is not of
/// that type must not become a [`Competition`] at all.
#[test]
fn deserializing_a_competition_with_a_metric_of_another_type_fails() {
    let sm = competition_in_the_final();
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for ranking in serialized_rankings(&mut value) {
        // where the three counts of a `WinsDrawsLosses` belong
        ranking["metrics"][WINS_DRAWS_LOSSES] = json!(0.0);
    }

    let Err(error) = serde_json::from_value::<Competition>(value) else {
        panic!("a metric of another type than its calculator produces must not deserialize");
    };

    assert!(
        error.to_string().contains("WinsDrawsLosses"),
        "the error must name the type the metric was expected to have, but was: {error}"
    );
}

/// Like [`deserializing_a_competition_with_a_metric_of_another_type_fails`], but for metrics that
/// are all of a type some calculator of the phase produces, only not the one at their position.
#[test]
fn deserializing_a_competition_whose_metrics_are_out_of_order_fails() {
    let sm = competition_in_the_final();
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for ranking in serialized_rankings(&mut value) {
        ranking["metrics"].as_array_mut().unwrap().reverse();
    }

    assert!(
        serde_json::from_value::<Competition>(value).is_err(),
        "metrics that do not follow the layout of the calculators must not deserialize"
    );
}

/// What leaving the type tag off the metrics costs: two metrics that serialize the same way are
/// indistinguishable once they are bare values, so swapping them is not an error but silently
/// gives every ranking the other one's value. Both are still of the type their position says they
/// are, so nothing that reads them is unsound — the competition is merely wrong.
///
/// The tags of the calculators are what bounds this: the set of types and the position of each is
/// still checked, only not which value was written for which of two positions of the same shape.
#[test]
fn swapping_two_metrics_that_serialize_the_same_way_is_not_detected() {
    let sm = competition_in_the_final();
    let phase = &sm.current().phases()[&phase_id()];
    // the two metrics are both a single `f32`, and they differ, so the swap is observable
    let swapped: Vec<_> = phase
        .ranking_ids_ordered_by_unique_rank()
        .iter()
        .map(|id| {
            let ranking = phase.ranking_view(id);
            (
                *ranking.metric::<Buchholz>(),
                *ranking.metric::<SonnebornBerger>(),
            )
        })
        .collect();
    assert!(swapped.iter().any(|(b, s)| b.0 != s.0));
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for ranking in serialized_rankings(&mut value) {
        ranking["metrics"]
            .as_array_mut()
            .unwrap()
            .swap(BUCHHOLZ, SONNEBORN_BERGER_METRIC);
    }

    let competition: Competition =
        serde_json::from_value(value).expect("the swap is not something deserializing can catch");

    let phase = &competition.phases()[&phase_id()];
    for (id, (buchholz, sonneborn_berger)) in phase
        .ranking_ids_ordered_by_unique_rank()
        .iter()
        .zip(swapped)
    {
        let ranking = phase.ranking_view(id);
        assert_eq!(ranking.metric::<Buchholz>().0, sonneborn_berger.0);
        assert_eq!(ranking.metric::<SonnebornBerger>().0, buchholz.0);
    }
}

/// The layout of a phase holds one position per metric type, so two calculators producing the same
/// type would leave one of them without a position of its own. Nothing can stop a serialized
/// competition from listing such a pair, which is why the metrics of every ranking are checked
/// against the layout before the rankings exist.
#[test]
fn deserializing_a_competition_with_two_calculators_of_the_same_metric_fails() {
    let sm = competition_in_the_final();
    let mut value = serde_json::to_value(sm.current()).unwrap();
    for phase in value["phases"].as_array_mut().unwrap() {
        let calculators = phase["rankings"][CALCULATORS].as_array_mut().unwrap();
        // both calculate a single `f32`, so every metric still reads back as some known type
        calculators[BUCHHOLZ] = calculators[SONNEBORN_BERGER_METRIC].clone();
    }

    let Err(error) = serde_json::from_value::<Competition>(value) else {
        panic!("two calculators of the same metric must not deserialize");
    };

    assert!(
        error.to_string().contains("do not follow the layout"),
        "the error must say that the metrics do not follow the layout, but was: {error}"
    );
}
