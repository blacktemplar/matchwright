use std::{collections::HashMap, num::NonZeroU32};

use itertools::Itertools;
use rand::{RngExt, rng};
use tournament_engine::{
    entities::competition::{Competition, Entrant, EntrantId, MatchResult},
    plugins::phase_format::single_elimination::SingleElimination,
    transitions::{
        add_entrant::AddEntrant, add_phase::AddPhase, remove_entrant::RemoveEntrant,
        set_competition_seed::SetCompetitionSeed, start_competition::StartCompetition,
    },
};
use undoable_state_machine::{LogEntry, LogEntryKind, StateMachine, Transition, TransitionId};
use uuid::Uuid;

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
    let phase_id = Uuid::new_v4();
    let phase_format = Box::new(SingleElimination);

    apply(&mut sm, SetCompetitionSeed::new(seed)).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_a.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_b.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_c.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_d.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_e.clone())).unwrap();
    apply(&mut sm, RemoveEntrant::new(entrant_c.clone())).unwrap();
    apply(&mut sm, AddPhase::new(phase_id, phase_format)).unwrap();
    apply(&mut sm, StartCompetition).unwrap();

    assert!(is_attending(sm.current(), &entrant_a));
    assert!(is_attending(sm.current(), &entrant_b));
    assert!(!is_attending(sm.current(), &entrant_c));
    assert!(is_attending(sm.current(), &entrant_d));
    assert!(is_attending(sm.current(), &entrant_e));
    assert_eq!(sm.current().seed(), seed);
    assert_eq!(sm.log().len(), 9);
    assert!(sm.current().is_started());
    let phase = &sm.current().phases()[0];
    assert_eq!(phase.id(), phase_id);
    assert_eq!(phase.rankings().len(), 4);
    let expected_entrants_by_id: HashMap<_, _> = [&entrant_a, &entrant_b, &entrant_d, &entrant_e]
        .into_iter()
        .map(|e| (e.id(), e))
        .collect();
    assert!(phase.rankings().iter().all(|r| r.entrants().len() == 1));
    assert_eq!(
        phase.rankings().iter().map(|r| r.entrants()[0]).counts(),
        expected_entrants_by_id.keys().copied().counts()
    );
    assert!(phase.rankings().iter().enumerate().all(|(i, r)| {
        r.unique_rank() == NonZeroU32::new(i as u32 + 1).unwrap()
            && r.initial_unique_rank() == r.unique_rank()
            && r.rank() == NonZeroU32::new(1).unwrap()
    }));
    let rankings = phase.rankings();
    let matches = phase.matches();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].team_a(), &[rankings[0].id()]);
    assert_eq!(matches[0].team_b(), &[rankings[3].id()]);
    assert_eq!(matches[0].round(), 0);
    assert_eq!(matches[0].result(), MatchResult::NotFinished);
    assert_eq!(matches[1].team_a(), &[rankings[1].id()]);
    assert_eq!(matches[1].team_b(), &[rankings[2].id()]);
    assert_eq!(matches[1].round(), 0);
    assert_eq!(matches[1].result(), MatchResult::NotFinished);
}
