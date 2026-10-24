use tournament_gui_e2e::{Gui, run, thirtyfour::error::WebDriverResult};

/// Enters `names` and starts a round robin of them.
async fn start_round_robin(gui: &Gui, names: &[&str]) -> WebDriverResult<()> {
    for name in names {
        gui.add_entrant(name).await?;
    }
    gui.choose_format("round robin").await?;
    gui.click("Add phase").await?;
    gui.click("Start").await
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_reported_result_shows_in_the_match_and_the_standings() {
    run(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.click("Bob wins").await?;

        gui.expect_results(&["Bob won"]).await?;
        gui.expect_standings(&["Bob", "Alice"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_result_is_reopened() {
    run(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.click("Bob wins").await?;

        gui.click_labelled("Reopen match 1").await?;

        gui.expect_results(&[]).await?;
        gui.click("Alice wins").await?;
        gui.expect_results(&["Alice won"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undo_takes_back_a_result_and_redo_brings_it_back() {
    run(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.click("Alice wins").await?;

        gui.click("Undo").await?;
        gui.expect_results(&[]).await?;
        gui.click("Redo").await?;
        gui.expect_results(&["Alice won"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_matches_are_shown_one_round_at_a_time() {
    run(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob", "Carol", "Dave"]).await?;
        gui.expect_matches(&["1", "2"]).await?;
        gui.page.check_accessibility().await?;

        gui.click("Round 2 (2 open)").await?;

        gui.expect_matches(&["3", "4"]).await
    })
    .await;
}
