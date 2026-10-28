use tournament_gui_e2e::{Key, run_in_new_competition};

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn entrants_are_listed_in_the_order_they_were_added() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;

        gui.expect_entrants(&["Alice", "Bob"]).await
    })
    .await;
}

/// A name differing from another only in case is said to look like it while it is typed, and is
/// taken all the same.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_name_like_one_entered_is_pointed_out_but_taken() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;

        gui.type_entrant_name("ALICE").await?;

        gui.expect_entrant_like(Some("“Alice” is entered already."))
            .await?;
        gui.press(Key::Enter).await?;
        gui.expect_entrants(&["Alice", "ALICE"]).await?;
        gui.expect_entrant_like(None).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn an_entrant_is_removed() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;

        gui.click_labelled("Remove Alice").await?;

        gui.expect_entrants(&["Bob"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn an_entrant_without_a_name_is_refused_with_why() {
    run_in_new_competition(async |gui| {
        gui.click("Add entrant").await?;

        gui.expect_error("An entrant name must not be empty.")
            .await?;
        gui.expect_entrants(&[]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_start_says_why_it_is_not_possible_yet() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.expect_start_refusal(Some("The competition has no phase yet."))
            .await?;
        gui.click("Add phase").await?;

        gui.expect_start_refusal(Some("A competition needs at least two entrants."))
            .await?;
        gui.expect_disabled("Start").await?;
        gui.add_entrant("Bob").await?;
        gui.expect_start_refusal(None).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undo_and_redo_walk_through_the_actions() {
    run_in_new_competition(async |gui| {
        gui.expect_disabled("Undo").await?;
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;

        gui.click("Undo").await?;
        gui.expect_entrants(&["Alice"]).await?;
        gui.click("Redo").await?;
        gui.expect_entrants(&["Alice", "Bob"]).await?;
        gui.expect_disabled("Redo").await
    })
    .await;
}

/// Undo keeps the focus once there is nothing left to undo, and pressing it then does nothing.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undo_keeps_the_focus_when_nothing_is_left_to_undo() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;

        gui.click("Undo").await?;

        gui.expect_entrants(&[]).await?;
        gui.expect_disabled("Undo").await?;
        gui.expect_focus("Undo").await?;
        gui.press(Key::Enter).await?;
        gui.expect_no_error().await
    })
    .await;
}

/// The phase added, the focus moves on to the start, rather than being lost with the form.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn adding_the_phase_moves_the_focus_to_the_start() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;

        gui.click("Add phase").await?;

        gui.expect_focus("Start").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn starting_needs_a_phase() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;
        gui.expect_disabled("Start").await?;

        gui.choose_format("Single elimination").await?;
        gui.click("Add phase").await?;

        gui.click("Start").await
    })
    .await;
}
