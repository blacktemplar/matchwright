use std::num::NonZeroU32;

use itertools::Itertools;
use rand::{RngExt, rng};
use tournament_engine::{
    entities::competition::{Competition, Entrant, EntrantId, Match, MatchResult, PhaseId},
    plugins::{
        metrics_calculator::round_of_first_loss::{RoundOfFirstLoss, RoundOfFirstLossCalculator},
        phase_format::single_elimination::SingleElimination,
        ranker::{HierarchicalRanker, LexicographicalMetrics},
    },
    transitions::{
        add_entrant::AddEntrant, add_phase::AddPhase, remove_entrant::RemoveEntrant,
        report_match_result::ReportMatchResult, set_competition_seed::SetCompetitionSeed,
        start_competition::StartCompetition,
    },
};
use undoable_state_machine::{LogEntry, LogEntryKind, StateMachine, Transition, TransitionId};
use uuid::Uuid;
use vec1::vec1;

type CompetitionStateMachine = StateMachine<Competition, Box<dyn Transition<Competition>>>;

fn entrant() -> Entrant {
    Entrant::new(EntrantId::new(Uuid::new_v4()))
}

fn is_attending(competition: &Competition, entrant: &Entrant) -> bool {
    competition.entrants().contains_key(&entrant.id())
}

fn apply(
    sm: &mut CompetitionStateMachine,
    transition: impl Transition<Competition> + 'static,
) -> Result<(), undoable_state_machine::StateMachineError> {
    sm.apply(LogEntry::new(
        TransitionId::new(),
        LogEntryKind::Apply(Box::new(transition)),
    ))
}

#[test]
fn full_tournament() {
    let entrant_a = entrant();
    let entrant_b = entrant();
    let entrant_c = entrant();
    let entrant_d = entrant();
    let entrant_e = entrant();

    let mut sm = CompetitionStateMachine::default();
    let seed = rng().random();
    let phase_id = PhaseId::new(Uuid::new_v4());
    let phase_format = Box::new(SingleElimination);
    let ranker = Box::new(HierarchicalRanker::new(vec![Box::new(
        LexicographicalMetrics::new().add_metric::<RoundOfFirstLoss>(),
    )]));

    apply(&mut sm, SetCompetitionSeed::new(seed)).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_a.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_b.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_c.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_d.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_e.clone())).unwrap();
    apply(&mut sm, RemoveEntrant::new(entrant_c.clone())).unwrap();
    apply(
        &mut sm,
        AddPhase::new(
            phase_id,
            phase_format,
            ranker,
            vec![Box::new(RoundOfFirstLossCalculator)],
        ),
    )
    .unwrap();
    apply(&mut sm, StartCompetition).unwrap();

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
    assert_eq!(phase.rankings().len(), 4);
    assert!(phase.rankings().values().all(|t| t.entrants().len() == 1));
    let ranking_id_by_initial_rank = phase.ranking_ids_ordered_by_unique_rank().to_vec();
    let entrants_by_initial_rank = ranking_id_by_initial_rank
        .iter()
        .map(|id| phase.rankings()[id].entrants()[0])
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
    assert!(
        phase
            .ranking_ids_ordered_by_unique_rank()
            .iter()
            .enumerate()
            .all(|(index, id)| {
                let r = &phase.rankings()[id];
                r.unique_rank() == NonZeroU32::new(index as u32 + 1).unwrap()
                    && r.initial_unique_rank() == r.unique_rank()
                    && r.rank() == NonZeroU32::new(1).unwrap()
            })
    );
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
    assert_eq!(match_0_vs_3.result(), MatchResult::NotFinished);
    assert_eq!(match_1_vs_2.team_a(), &[ranking_id_by_initial_rank[1]]);
    assert_eq!(match_1_vs_2.team_b(), &[ranking_id_by_initial_rank[2]]);
    assert_eq!(match_1_vs_2.round(), NonZeroU32::new(1).unwrap(),);
    assert_eq!(match_1_vs_2.result(), MatchResult::NotFinished);

    // now continue the tournament
    // rankings[3] wins against rankings[0]
    apply(
        &mut sm,
        ReportMatchResult::new(phase_id, match_0_vs_3_id, MatchResult::BWins),
    )
    .unwrap();
    // rankings[1] wins against rankings[2]
    apply(
        &mut sm,
        ReportMatchResult::new(phase_id, match_1_vs_2_id, MatchResult::AWins),
    )
    .unwrap();

    // asserts after the first round of single elimination
    let phase = &sm.current().phases().values().next().unwrap();
    let matches = phase.matches();
    let match_0_vs_3 = &matches[&match_0_vs_3_id];
    let match_1_vs_2 = &matches[&match_1_vs_2_id];
    assert_eq!(match_0_vs_3.result(), MatchResult::BWins);
    assert_eq!(match_1_vs_2.result(), MatchResult::AWins);
    // assert ranking data
    let rankings = phase.rankings();
    let ranking_0 = &rankings[&ranking_id_by_initial_rank[0]];
    let ranking_1 = &rankings[&ranking_id_by_initial_rank[1]];
    let ranking_2 = &rankings[&ranking_id_by_initial_rank[2]];
    let ranking_3 = &rankings[&ranking_id_by_initial_rank[3]];

    assert_eq!(ranking_0.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.initial_unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(
        ranking_0.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(Some(NonZeroU32::new(1).unwrap())))
    );
    assert_eq!(ranking_1.rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_1.unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_1.initial_unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(
        ranking_1.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(None))
    );
    assert_eq!(ranking_2.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_2.unique_rank(), NonZeroU32::new(4).unwrap());
    assert_eq!(ranking_2.initial_unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(
        ranking_2.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(Some(NonZeroU32::new(1).unwrap())))
    );
    assert_eq!(ranking_3.rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_3.unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_3.initial_unique_rank(), NonZeroU32::new(4).unwrap());
    assert_eq!(
        ranking_3.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(None))
    );
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
    assert_eq!(match_3_vs_1.result(), MatchResult::NotFinished);

    // finish the tournament
    apply(
        &mut sm,
        ReportMatchResult::new(phase_id, match_3_vs_1_id, MatchResult::AWins),
    )
    .unwrap();

    // final asserts
    let phase = &sm.current().phases().values().next().unwrap();
    let matches = phase.matches();
    let match_0_vs_3 = &matches[&match_0_vs_3_id];
    let match_1_vs_2 = &matches[&match_1_vs_2_id];
    let match_3_vs_1 = &matches[&match_3_vs_1_id];
    assert_eq!(match_0_vs_3.result(), MatchResult::BWins);
    assert_eq!(match_1_vs_2.result(), MatchResult::AWins);
    assert_eq!(match_3_vs_1.result(), MatchResult::AWins);

    // assert ranking data
    let rankings = phase.rankings();
    let ranking_0 = &rankings[&ranking_id_by_initial_rank[0]];
    let ranking_1 = &rankings[&ranking_id_by_initial_rank[1]];
    let ranking_2 = &rankings[&ranking_id_by_initial_rank[2]];
    let ranking_3 = &rankings[&ranking_id_by_initial_rank[3]];

    assert_eq!(ranking_0.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_0.initial_unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(
        ranking_0.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(Some(NonZeroU32::new(1).unwrap())))
    );
    assert_eq!(ranking_1.rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_1.unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(ranking_1.initial_unique_rank(), NonZeroU32::new(2).unwrap());
    assert_eq!(
        ranking_1.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(Some(NonZeroU32::new(2).unwrap())))
    );
    assert_eq!(ranking_2.rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(ranking_2.unique_rank(), NonZeroU32::new(4).unwrap());
    assert_eq!(ranking_2.initial_unique_rank(), NonZeroU32::new(3).unwrap());
    assert_eq!(
        ranking_2.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(Some(NonZeroU32::new(1).unwrap())))
    );
    assert_eq!(ranking_3.rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_3.unique_rank(), NonZeroU32::new(1).unwrap());
    assert_eq!(ranking_3.initial_unique_rank(), NonZeroU32::new(4).unwrap());
    assert_eq!(
        ranking_3.metric::<RoundOfFirstLoss>(),
        Some(&RoundOfFirstLoss(None))
    );
}
