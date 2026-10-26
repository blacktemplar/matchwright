use std::num::NonZeroU32;

use itertools::Itertools;
use rand::{RngExt, rng};
use tournament_engine::{
    AddEntrant, AddPhase, Competition, CompetitionStateMachine, CompetitionStateMachineExt,
    Entrant, EntrantId, Match, MatchId, MatchResult, PhaseId, PhasePluginsBuilder, Ranking,
    RemoveEntrant, ReportMatchResult, Score, Seed, SetCompetitionSeed, StartCompetition,
    plugins::{
        metrics_calculator::{
            Buchholz, BuchholzCalculator, BuchholzOfBuchholz, BuchholzOfBuchholzCalculator,
            LaterFirstLoss, Points, PointsCalculator, RoundOfFirstLoss, RoundOfFirstLossCalculator,
            WinsDrawsLosses, WinsDrawsLossesCalculator,
        },
        phase_format::SingleElimination,
    },
    undoable_state_machine::StateMachineError,
    uuid::Uuid,
};
use vec1::vec1;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

fn entrant() -> Entrant {
    Entrant::new(EntrantId::new(Uuid::new_v4()))
}

fn is_attending(competition: &Competition, entrant: &Entrant) -> bool {
    competition.entrants().contains_key(&entrant.id())
}

/// The scoring system of most association football leagues, used by the phase of this test.
const THREE_ONE: PointsCalculator = PointsCalculator::new(Score::new(3.0), Score::ONE);

/// Asserts that `ranking` holds exactly the given value of every metric calculated by the phase.
///
/// `round_of_first_loss` is given as a plain round number, `wins_draws_losses` as
/// wins/draws/losses, and the points and both Buchholz values as whole numbers, which they are
/// with [`THREE_ONE`].
#[track_caller]
fn assert_metrics(
    ranking: &Ranking,
    round_of_first_loss: Option<u32>,
    wins_draws_losses: (u32, u32, u32),
    points: u32,
    matches: &[MatchId],
    buchholz: u32,
    buchholz_of_buchholz: u32,
) {
    let [points, buchholz, buchholz_of_buchholz] =
        [points, buchholz, buchholz_of_buchholz].map(Score::from);
    let round_of_first_loss = round_of_first_loss.map(|r| NonZeroU32::new(r).unwrap());
    assert_eq!(
        ranking.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(round_of_first_loss))
    );
    let (wins, draws, losses) = wins_draws_losses;
    assert_eq!(
        ranking.metric::<WinsDrawsLosses>(),
        Some(&WinsDrawsLosses {
            wins,
            draws,
            losses,
            byes: 0,
        })
    );
    assert_eq!(ranking.metric::<Points>(), Some(&Points(points)));
    assert_eq!(ranking.played_matches(), matches);
    assert_eq!(ranking.metric::<Buchholz>(), Some(&Buchholz(buchholz)));
    assert_eq!(
        ranking.metric::<BuchholzOfBuchholz>(),
        Some(&BuchholzOfBuchholz(buchholz_of_buchholz))
    );
}

#[test]
fn full_tournament() {
    let entrant_a = entrant();
    let entrant_b = entrant();
    let entrant_c = entrant();
    let entrant_d = entrant();
    let entrant_e = entrant();

    let mut sm = CompetitionStateMachine::default();
    let seed = Seed::new(rng().random());
    let phase_id = PhaseId::new(Uuid::new_v4());

    sm.apply_transition(SetCompetitionSeed::new(seed)).unwrap();
    sm.apply_transition(AddEntrant::new(entrant_a.clone()))
        .unwrap();
    sm.apply_transition(AddEntrant::new(entrant_b.clone()))
        .unwrap();
    sm.apply_transition(AddEntrant::new(entrant_c.clone()))
        .unwrap();
    sm.apply_transition(AddEntrant::new(entrant_d.clone()))
        .unwrap();
    sm.apply_transition(AddEntrant::new(entrant_e.clone()))
        .unwrap();
    sm.apply_transition(RemoveEntrant::new(entrant_c.id()))
        .unwrap();
    let plugins = PhasePluginsBuilder::new()
        .format(SingleElimination::new())
        .add_ranking(LaterFirstLoss)
        .add_metric(RoundOfFirstLossCalculator)
        .add_metric(WinsDrawsLossesCalculator)
        .add_metric(THREE_ONE)
        .add_metric(BuchholzCalculator)
        .add_metric(BuchholzOfBuchholzCalculator)
        .build()
        .unwrap();
    sm.apply_transition(AddPhase::new(phase_id, plugins))
        .unwrap();
    sm.apply_transition(StartCompetition).unwrap();

    // asserts for the state after the competition start
    assert!(is_attending(sm.current(), &entrant_a));
    assert!(is_attending(sm.current(), &entrant_b));
    assert!(!is_attending(sm.current(), &entrant_c));
    assert!(is_attending(sm.current(), &entrant_d));
    assert!(is_attending(sm.current(), &entrant_e));
    assert_eq!(sm.current().seed(), seed);
    assert_eq!(sm.log().len(), 9);
    assert!(sm.current().is_started());
    let phase = &sm.current().phases().values().next().unwrap();
    assert_eq!(phase.id(), phase_id);
    let phase = phase.data();
    assert_eq!(phase.rankings().len(), 4);
    assert!(phase.rankings().values().all(|t| t.entrants().len() == 1));
    let ranking_id_by_initial_rank = phase.standings().map(Ranking::id).collect_vec();
    let entrants_by_initial_rank = phase
        .standings()
        .map(|ranking| ranking.entrants()[0])
        .collect_vec();

    assert_eq!(
        entrants_by_initial_rank.iter().counts(),
        [
            entrant_a.id(),
            entrant_b.id(),
            entrant_d.id(),
            entrant_e.id()
        ]
        .iter()
        .counts()
    );
    assert!(phase.standings().enumerate().all(|(index, r)| {
        r.unique_rank() == NonZeroU32::new(index as u32 + 1).unwrap()
            && r.initial_unique_rank() == r.unique_rank()
            && r.rank() == NonZeroU32::new(1).unwrap()
    }));
    // every metric starts out at its default value
    for ranking in phase.standings() {
        assert_metrics(ranking, None, (0, 0, 0), 0, &[], 0, 0);
    }
    let matches = phase.matches();
    assert_eq!(matches.len(), 2);
    let match_0_vs_3_id = Match::new(
        vec1![ranking_id_by_initial_rank[0]],
        vec1![ranking_id_by_initial_rank[3]],
        NonZeroU32::new(1).unwrap(),
    )
    .id();
    let match_1_vs_2_id = Match::new(
        vec1![ranking_id_by_initial_rank[1]],
        vec1![ranking_id_by_initial_rank[2]],
        NonZeroU32::new(1).unwrap(),
    )
    .id();
    assert!(matches.contains_key(&match_0_vs_3_id));
    assert!(matches.contains_key(&match_1_vs_2_id));
    let match_0_vs_3 = &matches[&match_0_vs_3_id];
    let match_1_vs_2 = &matches[&match_1_vs_2_id];
    assert_eq!(match_0_vs_3.team_a(), &[ranking_id_by_initial_rank[0]]);
    assert_eq!(match_0_vs_3.team_b(), &[ranking_id_by_initial_rank[3]]);
    assert_eq!(match_0_vs_3.round(), NonZeroU32::new(1).unwrap(),);
    assert_eq!(match_0_vs_3.result(), None);
    assert_eq!(match_1_vs_2.team_a(), &[ranking_id_by_initial_rank[1]]);
    assert_eq!(match_1_vs_2.team_b(), &[ranking_id_by_initial_rank[2]]);
    assert_eq!(match_1_vs_2.round(), NonZeroU32::new(1).unwrap(),);
    assert_eq!(match_1_vs_2.result(), None);

    // now continue the tournament
    // rankings[3] wins against rankings[0]
    sm.apply_transition(ReportMatchResult::new(
        phase_id,
        match_0_vs_3_id,
        MatchResult::BWins,
    ))
    .unwrap();
    // rankings[1] wins against rankings[2]
    sm.apply_transition(ReportMatchResult::new(
        phase_id,
        match_1_vs_2_id,
        MatchResult::AWins,
    ))
    .unwrap();

    // asserts after the first round of single elimination
    let phase = sm.current().phases().values().next().unwrap().data();
    let matches = phase.matches();
    let match_0_vs_3 = &matches[&match_0_vs_3_id];
    let match_1_vs_2 = &matches[&match_1_vs_2_id];
    assert_eq!(match_0_vs_3.result(), Some(MatchResult::BWins));
    assert_eq!(match_1_vs_2.result(), Some(MatchResult::AWins));
    // assert ranking data
    let rankings = phase.rankings();
    let ranking_0 = &rankings[&ranking_id_by_initial_rank[0]];
    let ranking_1 = &rankings[&ranking_id_by_initial_rank[1]];
    let ranking_2 = &rankings[&ranking_id_by_initial_rank[2]];
    let ranking_3 = &rankings[&ranking_id_by_initial_rank[3]];

    assert_eq!(ranking_0.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.initial_unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_1.rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_1.unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_1.initial_unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_2.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_2.unique_rank(), NonZeroU32::new(4).unwrap());
    assert_eq!(ranking_2.initial_unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_3.rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_3.unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_3.initial_unique_rank(), NonZeroU32::new(4).unwrap());
    // assert metrics: every ranking played exactly one match, so the buchholz of the two losers
    // is the three points of the winner that knocked them out, whereas the two winners beat
    // opponents without any points. The buchholz of buchholz mirrors that.
    assert_metrics(ranking_0, Some(1), (0, 0, 1), 0, &[match_0_vs_3_id], 3, 0);
    assert_metrics(ranking_1, None, (1, 0, 0), 3, &[match_1_vs_2_id], 0, 3);
    assert_metrics(ranking_2, Some(1), (0, 0, 1), 0, &[match_1_vs_2_id], 3, 0);
    assert_metrics(ranking_3, None, (1, 0, 0), 3, &[match_0_vs_3_id], 0, 3);
    // assert next round matches
    let match_3_vs_1_id = Match::new(
        vec1![ranking_id_by_initial_rank[3]],
        vec1![ranking_id_by_initial_rank[1]],
        NonZeroU32::new(2).unwrap(),
    )
    .id();
    assert_eq!(matches.len(), 3);
    assert!(matches.contains_key(&match_0_vs_3_id));
    assert!(matches.contains_key(&match_1_vs_2_id));
    assert!(matches.contains_key(&match_3_vs_1_id));
    let match_3_vs_1 = &matches[&match_3_vs_1_id];
    assert_eq!(match_3_vs_1.team_a(), &[ranking_id_by_initial_rank[3]]);
    assert_eq!(match_3_vs_1.team_b(), &[ranking_id_by_initial_rank[1]]);
    assert_eq!(match_3_vs_1.round(), NonZeroU32::new(2).unwrap(),);
    assert_eq!(match_3_vs_1.result(), None);

    // finish the tournament
    sm.apply_transition(ReportMatchResult::new(
        phase_id,
        match_3_vs_1_id,
        MatchResult::AWins,
    ))
    .unwrap();

    // final asserts
    let phase = sm.current().phases().values().next().unwrap().data();
    let matches = phase.matches();
    let match_0_vs_3 = &matches[&match_0_vs_3_id];
    let match_1_vs_2 = &matches[&match_1_vs_2_id];
    let match_3_vs_1 = &matches[&match_3_vs_1_id];
    assert_eq!(match_0_vs_3.result(), Some(MatchResult::BWins));
    assert_eq!(match_1_vs_2.result(), Some(MatchResult::AWins));
    assert_eq!(match_3_vs_1.result(), Some(MatchResult::AWins));

    // assert ranking data
    let rankings = phase.rankings();
    let ranking_0 = &rankings[&ranking_id_by_initial_rank[0]];
    let ranking_1 = &rankings[&ranking_id_by_initial_rank[1]];
    let ranking_2 = &rankings[&ranking_id_by_initial_rank[2]];
    let ranking_3 = &rankings[&ranking_id_by_initial_rank[3]];

    assert_eq!(ranking_0.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.initial_unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_1.rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_1.unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_1.initial_unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_2.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_2.unique_rank(), NonZeroU32::new(4).unwrap());
    assert_eq!(ranking_2.initial_unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_3.rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_3.unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_3.initial_unique_rank(), NonZeroU32::new(4).unwrap());
    // assert metrics
    // the only opponent of `0` is the winner `3` (6 points), whose own buchholz is 3
    assert_metrics(ranking_0, Some(1), (0, 0, 1), 0, &[match_0_vs_3_id], 6, 3);
    // `1` faced `2` (0 points, buchholz 3) and `3` (6 points, buchholz 3)
    assert_metrics(
        ranking_1,
        Some(2),
        (1, 0, 1),
        3,
        &[match_1_vs_2_id, match_3_vs_1_id],
        6,
        6,
    );
    // the only opponent of `2` is `1` (3 points, buchholz 6)
    assert_metrics(ranking_2, Some(1), (0, 0, 1), 0, &[match_1_vs_2_id], 3, 6);
    // the winner `3` faced `0` (0 points, buchholz 6) and `1` (3 points, buchholz 6)
    assert_metrics(
        ranking_3,
        None,
        (2, 0, 0),
        6,
        &[match_0_vs_3_id, match_3_vs_1_id],
        3,
        12,
    );
}

/// In a round robin of three, every entrant has a bye in one of the three rounds. The byes are
/// scheduled when the competition starts, but each is scored only once its entrant has played the
/// rounds before it: that of round 1 right away, that of round 2 after round 1's match.
#[test]
fn byes_are_scored_when_their_ranking_reaches_them() {
    let mut sm = CompetitionStateMachine::default();
    let phase_id = PhaseId::new(Uuid::new_v4());
    for _ in 0..3 {
        sm.apply_transition(AddEntrant::new(entrant())).unwrap();
    }
    // a round robin
    let plugins = PhasePluginsBuilder::new()
        .add_metric(WinsDrawsLossesCalculator)
        .add_metric(THREE_ONE.with_points_for_bye(Score::new(3.0)))
        .build()
        .unwrap();
    sm.apply_transition(AddPhase::new(phase_id, plugins))
        .unwrap();

    sm.apply_transition(StartCompetition).unwrap();
    // the byes scored by each ranking, by the round of its bye
    let byes_scored = |sm: &CompetitionStateMachine| {
        let phase = sm.current().phases()[&phase_id].data();
        phase
            .byes()
            .values()
            .map(|bye| {
                let ranking = &phase.rankings()[&bye.ranking()];
                let WinsDrawsLosses { byes, draws, .. } = *ranking.metric().unwrap();
                let points = ranking.metric::<Points>().unwrap().0;
                assert_eq!(points, Score::new(f64::from(3 * byes + draws)));
                assert_eq!(bye.is_reached(), byes == 1);
                (bye.round().get(), byes)
            })
            .sorted()
            .collect_vec()
    };

    let phase = sm.current().phases()[&phase_id].data();
    assert_eq!(phase.matches().len(), 3);
    assert_eq!(phase.byes().len(), 3);
    assert_eq!(byes_scored(&sm), [(1, 1), (2, 0), (3, 0)]);

    let first_round = phase
        .matches()
        .values()
        .filter(|r#match| r#match.round().get() == 1)
        .exactly_one()
        .unwrap()
        .id();
    sm.apply_transition(ReportMatchResult::new(
        phase_id,
        first_round,
        MatchResult::Draw,
    ))
    .unwrap();

    assert_eq!(byes_scored(&sm), [(1, 1), (2, 1), (3, 0)]);
}

/// A single elimination match has to produce a winner, so a draw is rejected as not applicable
/// instead of reaching the phase format, and the match can still be decided afterwards.
#[test]
fn single_elimination_rejects_a_draw() {
    let mut sm = CompetitionStateMachine::default();
    let phase_id = PhaseId::new(Uuid::new_v4());
    sm.apply_transition(AddEntrant::new(entrant())).unwrap();
    sm.apply_transition(AddEntrant::new(entrant())).unwrap();
    let plugins = PhasePluginsBuilder::new()
        .format(SingleElimination::new())
        .build()
        .unwrap();
    sm.apply_transition(AddPhase::new(phase_id, plugins))
        .unwrap();
    sm.apply_transition(StartCompetition).unwrap();
    let final_id = *sm.current().phases()[&phase_id]
        .data()
        .matches()
        .keys()
        .exactly_one()
        .unwrap();

    let draw = sm.apply_transition(ReportMatchResult::new(
        phase_id,
        final_id,
        MatchResult::Draw,
    ));

    let Err(StateMachineError::Transition(_, error)) = draw else {
        panic!("a draw must not be applicable, got {draw:?}");
    };
    assert_eq!(error.to_string(), "the phase format does not allow draws");
    assert!(!sm.current().phases()[&phase_id].data().matches()[&final_id].is_finished());
    sm.apply_transition(ReportMatchResult::new(
        phase_id,
        final_id,
        MatchResult::AWins,
    ))
    .unwrap();
}
