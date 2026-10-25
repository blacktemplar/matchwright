use tournament_gui_e2e::{Gui, Key, run_in_new_competition, thirtyfour::error::WebDriverResult};

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
    run_in_new_competition(async |gui| {
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
    run_in_new_competition(async |gui| {
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
    run_in_new_competition(async |gui| {
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
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob", "Carol", "Dave"]).await?;
        gui.expect_matches(&["1", "2"]).await?;
        gui.page.check_accessibility().await?;

        gui.click("Round 2 (2 open)").await?;

        gui.expect_matches(&["3", "4"]).await
    })
    .await;
}

/// In a round robin of four, every match of the second round has an entrant of each match of the
/// first.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_matches_that_can_be_played_now_are_listed() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob", "Carol", "Dave"]).await?;

        gui.click("Playable now (2)").await?;
        gui.expect_matches(&["1", "2"]).await?;
        gui.click("Draw").await?;
        gui.expect_matches(&["2"]).await?;
        gui.click("Draw").await?;

        gui.expect_matches(&["3", "4"]).await?;
        gui.page.check_accessibility().await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_result_is_typed_in_by_the_number_of_its_match() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.enter_result("2 1").await?;
        gui.expect_error("there is no match 2").await?;
        gui.enter_result("1 x").await?;

        gui.expect_results(&["Draw"]).await?;
        gui.expect_announcement("Match 1: Draw. Every match has a result.")
            .await
    })
    .await;
}

/// After a result is reported by its button, the focus moves on to the next match without one,
/// and to the heading of the round once there is none.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn results_are_reported_by_keyboard_one_match_after_the_other() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob", "Carol", "Dave"]).await?;

        gui.click("Draw").await?;
        gui.expect_focus_in_match("2").await?;
        gui.press(Key::Enter).await?;

        gui.expect_focus("Round 2").await?;
        gui.expect_matches(&["3", "4"]).await?;
        gui.press(Key::Tab).await?;
        gui.press(Key::Enter).await?;
        gui.expect_finished(&["3"]).await?;
        gui.expect_focus_in_match("4").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_reopened_match_has_the_focus_and_is_announced() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.click("Draw").await?;

        gui.click_labelled("Reopen match 1").await?;

        gui.expect_announcement("Match 1 is open again.").await?;
        gui.expect_focus_in_match("1").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undo_and_redo_announce_what_they_did() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.click("Draw").await?;

        gui.click("Undo").await?;
        gui.expect_announcement("Undone: the result of match 1 (Draw).")
            .await?;
        gui.click("Redo").await?;
        gui.expect_announcement("Redone: the result of match 1 (Draw).")
            .await
    })
    .await;
}
