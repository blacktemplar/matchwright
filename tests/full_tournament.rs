use rand::{RngExt, rng};
use tournament_engine::{
    entities::competition::{Competition, Entrant, Player},
    transitions::{
        add_entrant::AddEntrant, remove_entrant::RemoveEntrant,
        set_competition_seed::SetCompetitionSeed,
    },
};
use undoable_state_machine::{LogEntry, LogEntryKind, StateMachine, Transition, TransitionId};
use uuid::Uuid;
use vec1::Vec1;

type CompetitionStateMachine = StateMachine<Competition, Box<dyn Transition<Competition>>>;

fn player() -> Player {
    Player::new(Uuid::new_v4())
}

fn entrant_with_players(count: usize) -> Entrant {
    Entrant::new(
        Uuid::new_v4(),
        Vec1::try_from_vec((0..count).map(|_| player()).collect()).unwrap(),
    )
}

fn is_attending(competition: &Competition, entrant: &Entrant) -> bool {
    entrant
        .players()
        .iter()
        .all(|player| matches!(competition.entrant_of_player(player), Some(t) if &**t == entrant))
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
    let entrant_a = entrant_with_players(2);
    let entrant_b = entrant_with_players(3);
    let entrant_c = entrant_with_players(4);
    let entrant_d = entrant_with_players(5);
    let entrant_e = entrant_with_players(4);

    let mut sm = CompetitionStateMachine::default();
    let seed = rng().random();

    apply(&mut sm, SetCompetitionSeed::new(seed)).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_a.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_b.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_c.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_d.clone())).unwrap();
    apply(&mut sm, AddEntrant::new(entrant_e.clone())).unwrap();
    apply(&mut sm, RemoveEntrant::new(entrant_c.clone())).unwrap();

    assert!(is_attending(sm.current(), &entrant_a));
    assert!(is_attending(sm.current(), &entrant_b));
    assert!(!is_attending(sm.current(), &entrant_c));
    assert!(is_attending(sm.current(), &entrant_d));
    assert!(is_attending(sm.current(), &entrant_e));
    assert_eq!(sm.current().seed(), seed);
    assert_eq!(sm.log().len(), 7);
}
