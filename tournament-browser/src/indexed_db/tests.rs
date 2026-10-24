use tournament_app::session::Session;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

use super::*;

wasm_bindgen_test_configure!(run_in_browser);

/// A database of its own, as the tests run in the same browser.
async fn database() -> IndexedDb {
    IndexedDb::open(&format!("test-{}", uuid::Uuid::new_v4()))
        .await
        .unwrap()
}

fn names(session: &Session) -> Vec<Option<String>> {
    session
        .entrants()
        .into_iter()
        .map(|entrant| entrant.name)
        .collect()
}

#[wasm_bindgen_test]
async fn a_saved_competition_loads_as_it_was_saved() {
    let storage = database().await;
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    session.save(&storage).await.unwrap();
    session.add_entrant("Bob").unwrap();
    session.save(&storage).await.unwrap();

    let loaded = Session::load(&storage, session.key()).await.unwrap();

    assert_eq!(loaded.name(), "cup");
    assert_eq!(names(&loaded), names(&session));
    assert!(
        Session::load(&storage, CompetitionKey::new(uuid::Uuid::new_v4()))
            .await
            .is_err()
    );
}

#[wasm_bindgen_test]
async fn the_entries_are_read_from_a_position_on() {
    let storage = database().await;
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    session.add_entrant("Bob").unwrap();
    session.save(&storage).await.unwrap();
    let all = storage.read(session.key(), 0).await.unwrap().unwrap();

    let after = storage.read(session.key(), 2).await.unwrap().unwrap();

    assert_eq!(all.entries.len(), 3);
    assert_eq!(after.entries, all.entries[2..]);
    assert_eq!(after.people.len(), 2, "the people are read whole");
    let beyond = storage.read(session.key(), 3).await.unwrap().unwrap();
    assert!(beyond.entries.is_empty());
}

#[wasm_bindgen_test]
async fn a_session_catches_up_with_what_another_one_saved() {
    let storage = database().await;
    let mut changing = Session::new("cup");
    changing.add_entrant("Alice").unwrap();
    changing.save(&storage).await.unwrap();
    let mut following = Session::load(&storage, changing.key()).await.unwrap();

    changing.add_entrant("Bob").unwrap();
    changing.undo().unwrap();
    changing.save(&storage).await.unwrap();

    assert_eq!(following.catch_up(&storage).await.unwrap(), 2);
    assert_eq!(names(&following), names(&changing));
}

#[wasm_bindgen_test]
async fn competitions_are_listed_in_the_order_they_were_first_saved() {
    let storage = database().await;
    let mut sessions: Vec<_> = ["league", "cup", "open"].map(Session::new).into();
    for session in &mut sessions {
        session.save(&storage).await.unwrap();
    }
    sessions[0].add_entrant("Alice").unwrap();
    sessions[0].save(&storage).await.unwrap();

    let listed = storage.list().await.unwrap();

    let expected: Vec<_> = sessions
        .iter()
        .map(|session| Listing {
            key: session.key(),
            name: session.name().to_owned(),
        })
        .collect();
    assert_eq!(listed, expected);
}

/// A GUI writes changes while the operator goes on, so a write may finish after a later one.
#[wasm_bindgen_test]
async fn changes_written_out_of_order_are_all_saved() {
    let storage = database().await;
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();
    let first = session.unsaved_changes();
    session.add_entrant("Bob").unwrap();
    let second = session.unsaved_changes();

    storage.write(&second).await.unwrap();
    storage.write(&first).await.unwrap();

    let loaded = Session::load(&storage, session.key()).await.unwrap();
    assert_eq!(names(&loaded), names(&session));
}

#[wasm_bindgen_test]
async fn changes_that_leave_a_gap_in_the_log_store_nothing() {
    let (storage, elsewhere) = (database().await, database().await);
    let mut session = Session::new("cup");
    session.save(&elsewhere).await.unwrap();
    session.add_entrant("Alice").unwrap();

    let written = storage.write(&session.unsaved_changes()).await;

    assert!(matches!(
        written,
        Err(IndexedDbError::Gap {
            first_entry: 1,
            stored: 0
        })
    ));
    assert_eq!(storage.list().await.unwrap(), []);
}

/// The log knows the entrants by id only, so that their names can be deleted without touching the
/// log.
#[wasm_bindgen_test]
async fn names_are_stored_apart_from_the_log() {
    let storage = database().await;
    let mut session = Session::new("cup");
    session.add_entrant("Alice").unwrap();

    session.save(&storage).await.unwrap();

    let stored = storage.read(session.key(), 0).await.unwrap().unwrap();
    assert!(stored.entries.iter().all(|entry| !entry.contains("Alice")));
    assert!(stored.people[0].contains("Alice"));
}

#[wasm_bindgen_test]
async fn competitions_are_stored_apart_from_each_other() {
    let storage = database().await;
    let (mut cup, mut league) = (Session::new("cup"), Session::new("league"));
    cup.add_entrant("Alice").unwrap();
    league.add_entrant("Bob").unwrap();

    cup.save(&storage).await.unwrap();
    league.save(&storage).await.unwrap();

    let loaded = Session::load(&storage, cup.key()).await.unwrap();
    assert_eq!(names(&loaded), [Some("Alice".to_owned())]);
}
