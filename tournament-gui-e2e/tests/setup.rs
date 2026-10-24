use tournament_gui_e2e::run_in_new_competition;

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

        gui.expect_error("an entrant name must not be empty")
            .await?;
        gui.expect_entrants(&[]).await
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

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn starting_needs_a_phase() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;
        gui.expect_disabled("Start").await?;

        gui.choose_format("single elimination").await?;
        gui.click("Add phase").await?;

        gui.click("Start").await
    })
    .await;
}
