use tournament_gui_e2e::{Gui, run_in_new_competition, thirtyfour::error::WebDriverResult};

/// Enters Alice and Bob, adds a round robin with what it recommends, and starts.
async fn start_round_robin(gui: &Gui) -> WebDriverResult<()> {
    gui.add_entrant("Alice").await?;
    gui.add_entrant("Bob").await?;
    gui.click("Add phase").await?;
    gui.click("Start").await
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_format_brings_what_it_recommends() {
    run_in_new_competition(async |gui| {
        gui.expect_list("ranking", &["more points", "more Sonneborn-Berger"])
            .await?;

        gui.choose_format("single elimination").await?;

        gui.expect_list("ranking", &["later first loss", "fewer losses"])
            .await?;
        gui.page.check_accessibility().await?;
        gui.click("Add phase").await?;
        gui.expect_phase(&[
            "single elimination",
            "later first loss, fewer losses",
            "round of the first loss, win-draw-loss record",
        ])
        .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_number_not_taken_is_shown_with_why() {
    run_in_new_competition(async |gui| {
        gui.choose_format("Monster DYP").await?;

        gui.enter("format.rounds", "0").await?;

        gui.expect_issues(&["this takes a whole number of at least 1"])
            .await?;
        gui.enter("format.rounds", "7").await?;
        gui.expect_issues(&[]).await
    })
    .await;
}

/// The points can be scored in many ways, so they are not added without being chosen, and the
/// ranking by them stays rather than being dropped unnoticed.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_metric_needed_and_not_chosen_is_an_issue_at_the_metrics() {
    run_in_new_competition(async |gui| {
        gui.toggle("points").await?;

        gui.expect_issues(&[
            "more points needs one of these: points",
            "Sonneborn-Berger needs one of these: points",
        ])
        .await?;
        gui.expect_list("ranking", &["more points", "more Sonneborn-Berger"])
            .await?;
        gui.toggle("points").await?;
        gui.expect_issues(&[]).await
    })
    .await;
}

/// The metrics after one removed move up, and each shows as checked still.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_metrics_after_one_removed_stay_checked() {
    run_in_new_competition(async |gui| {
        gui.expect_checked(
            "metrics",
            &["win-draw-loss record", "points", "Sonneborn-Berger"],
        )
        .await?;

        gui.toggle("points").await?;

        gui.expect_checked("metrics", &["win-draw-loss record", "Sonneborn-Berger"])
            .await?;
        gui.toggle("Sonneborn-Berger").await?;
        gui.expect_checked("metrics", &["win-draw-loss record"])
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_criteria_of_the_ranking_are_added_and_moved() {
    run_in_new_competition(async |gui| {
        gui.choose_plugin("ranking", "more wins").await?;
        gui.expect_list(
            "ranking",
            &["more points", "more Sonneborn-Berger", "more wins"],
        )
        .await?;

        gui.click_labelled("Move more wins up").await?;

        gui.expect_list(
            "ranking",
            &["more points", "more wins", "more Sonneborn-Berger"],
        )
        .await?;
        gui.click_labelled("Remove more points").await?;
        gui.expect_list("ranking", &["more wins", "more Sonneborn-Berger"])
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_phase_is_edited_after_the_start_in_a_dialog() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui).await?;
        gui.click("Alice wins").await?;

        gui.click("Edit phase").await?;
        gui.click_labelled("Move more Sonneborn-Berger up").await?;
        gui.page.check_accessibility().await?;
        gui.click("Save changes").await?;

        gui.expect_no_dialog().await?;
        gui.expect_phase(&[
            "round robin",
            "more Sonneborn-Berger, more points",
            "win-draw-loss record, points, Sonneborn-Berger",
        ])
        .await?;
        gui.expect_results(&["Alice won"]).await?;
        gui.click("Undo").await?;
        gui.expect_phase(&[
            "round robin",
            "more points, more Sonneborn-Berger",
            "win-draw-loss record, points, Sonneborn-Berger",
        ])
        .await
    })
    .await;
}

/// The teams of a Monster DYP are pairs, so it never schedules the match of single entrants whose
/// result was reported.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn an_edit_a_result_does_not_fit_is_refused_in_the_dialog() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui).await?;
        gui.click("Alice wins").await?;
        gui.click("Edit phase").await?;

        gui.choose_format("Monster DYP").await?;
        gui.click("Save changes").await?;

        gui.expect_dialog_error(
            "the phase cannot be changed this way, since a later change no longer fits it: match \
             does not exist",
        )
        .await?;
        gui.click("Cancel").await?;
        gui.expect_no_dialog().await?;
        gui.expect_phase(&[
            "round robin",
            "more points, more Sonneborn-Berger",
            "win-draw-loss record, points, Sonneborn-Berger",
        ])
        .await
    })
    .await;
}
