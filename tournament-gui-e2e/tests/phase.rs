use tournament_gui_e2e::{Gui, Key, run_in_new_competition, thirtyfour::error::WebDriverResult};

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
        gui.expect_list("ranking", &["More points", "More Sonneborn-Berger"])
            .await?;

        gui.choose_format("Single elimination").await?;

        gui.expect_list("ranking", &["Later first loss", "Fewer losses"])
            .await?;
        gui.page.check_accessibility().await?;
        gui.click("Add phase").await?;
        gui.expect_phase(&[
            "Single elimination",
            "Later first loss, fewer losses",
            "Round of the first loss, win-draw-loss record",
        ])
        .await
    })
    .await;
}

/// Enter takes the number typed, as leaving the field does, rather than adding the phase.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn enter_in_a_number_takes_it_without_adding_the_phase() {
    run_in_new_competition(async |gui| {
        gui.choose_format("Monster DYP").await?;

        gui.type_into("format.rounds", "0").await?;
        gui.press(Key::Enter).await?;

        gui.expect_issues(&["This takes a whole number of at least 1."])
            .await?;
        gui.type_into("format.rounds", "7").await?;
        gui.press(Key::Enter).await?;
        gui.expect_issues(&[]).await?;
        gui.expect_phase(&[]).await?;
        gui.click("Add phase").await?;
        gui.expect_phase(&[
            "Monster DYP",
            "More points",
            "Win-draw-loss record",
            "Points",
        ])
        .await
    })
    .await;
}

/// Adding a phase that cannot be played shows what to change rather than refusing it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn adding_a_phase_with_issues_moves_the_focus_to_them() {
    run_in_new_competition(async |gui| {
        gui.toggle("Points").await?;

        gui.click("Add phase").await?;

        gui.expect_focus(
            "More points needs one of these: points.\nSonneborn-Berger needs one of these: points.",
        )
        .await?;
        gui.expect_no_error().await?;
        gui.expect_phase(&[]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_number_not_taken_is_shown_with_why() {
    run_in_new_competition(async |gui| {
        gui.choose_format("Monster DYP").await?;

        gui.enter("format.rounds", "0").await?;

        gui.expect_issues(&["This takes a whole number of at least 1."])
            .await?;
        gui.enter("format.rounds", "7").await?;
        gui.expect_issues(&[]).await
    })
    .await;
}

/// Leaving a number with Tab focuses the button of what is said about the next field, whose text
/// shows below it rather than over why the number was not taken.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn what_is_said_about_the_next_field_leaves_the_issue_in_sight() {
    run_in_new_competition(async |gui| {
        gui.choose_format("Monster DYP").await?;

        gui.enter("format.rounds", "0").await?;

        gui.expect_issues(&["This takes a whole number of at least 1."])
            .await?;
        gui.expect_toggletip_clear_of_issues().await
    })
    .await;
}

/// The points can be scored in many ways, so they are not added without being chosen, and the
/// ranking by them stays rather than being dropped unnoticed.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_metric_needed_and_not_chosen_is_an_issue_at_the_metrics() {
    run_in_new_competition(async |gui| {
        gui.toggle("Points").await?;

        gui.expect_issues(&[
            "More points needs one of these: points.",
            "Sonneborn-Berger needs one of these: points.",
        ])
        .await?;
        gui.expect_list("ranking", &["More points", "More Sonneborn-Berger"])
            .await?;
        gui.toggle("Points").await?;
        gui.expect_issues(&[]).await
    })
    .await;
}

/// A metric stays where it is when it is unchecked or checked again, keeping the focus, so that a
/// second click at the same place is at the same metric; the others show as checked still.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_metrics_stay_in_place_when_toggled() {
    const ALL: [&str; 3] = ["Points", "Sonneborn-Berger", "Win-draw-loss record"];
    run_in_new_competition(async |gui| {
        gui.expect_checked("metrics", &ALL).await?;

        gui.toggle("Points").await?;

        gui.expect_checked("metrics", &["Sonneborn-Berger", "Win-draw-loss record"])
            .await?;
        gui.expect_focus_on_checkbox("Points").await?;
        gui.toggle("Sonneborn-Berger").await?;
        gui.expect_checked("metrics", &["Win-draw-loss record"])
            .await?;
        gui.toggle("Points").await?;
        gui.toggle("Sonneborn-Berger").await?;
        gui.expect_checked("metrics", &ALL).await?;
        gui.expect_focus_on_checkbox("Sonneborn-Berger").await
    })
    .await;
}

/// The focus stays with what the operator works on: the list a plugin was added from, the
/// plugin moved, even to the top, and the plugin after the one removed.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_criteria_of_the_ranking_are_added_and_moved() {
    run_in_new_competition(async |gui| {
        gui.choose_plugin("ranking", "More wins").await?;
        gui.expect_list(
            "ranking",
            &["More points", "More Sonneborn-Berger", "More wins"],
        )
        .await?;
        gui.expect_focus_on_choice("ranking").await?;

        gui.click_labelled("Move more wins up").await?;
        gui.click_labelled("Move more wins up").await?;

        gui.expect_list(
            "ranking",
            &["More wins", "More points", "More Sonneborn-Berger"],
        )
        .await?;
        gui.expect_focus_in_list("ranking", "More wins").await?;
        gui.click_labelled("Remove more points").await?;
        gui.expect_list("ranking", &["More wins", "More Sonneborn-Berger"])
            .await?;
        gui.expect_focus_in_list("ranking", "More Sonneborn-Berger")
            .await?;
        gui.click_labelled("Remove more Sonneborn-Berger").await?;
        gui.expect_focus_in_list("ranking", "More wins").await
    })
    .await;
}

/// The format chosen, the focus is back at its choice rather than on the list that closed.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_focus_is_back_at_the_format_once_it_is_chosen() {
    run_in_new_competition(async |gui| {
        gui.choose_format("Single elimination").await?;

        gui.expect_focus_on_choice("format").await
    })
    .await;
}

/// A phase ranked by nothing is added, but the form says what that leads to first.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_ranking_of_nothing_is_warned_about() {
    run_in_new_competition(async |gui| {
        gui.click_labelled("Remove more points").await?;
        gui.click_labelled("Remove more Sonneborn-Berger").await?;

        gui.expect_warnings(&["Ranked by nothing, every entrant is tied in the standings."])
            .await?;
        gui.page.check_accessibility().await?;
        gui.click("Add phase").await?;
        gui.expect_phase(&[
            "Round robin",
            "(none)",
            "Win-draw-loss record",
            "Points",
            "Sonneborn-Berger",
        ])
        .await
    })
    .await;
}

/// Undoing the phase brings back the form as it was when the phase was added, so that adding it
/// again adds the same phase.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undoing_the_phase_brings_back_its_form() {
    run_in_new_competition(async |gui| {
        gui.choose_format("Monster DYP").await?;
        gui.enter("format.rounds", "7").await?;
        gui.click("Add phase").await?;

        gui.click("Undo").await?;

        gui.expect_entered("format.rounds", "7").await?;
        gui.expect_list("ranking", &["More points"]).await
    })
    .await;
}

/// Numbers are read by the form: with a comma before the decimals as well, and what is not read
/// as a number is named.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_number_is_read_with_a_comma_and_what_is_none_is_named() {
    run_in_new_competition(async |gui| {
        gui.enter("metrics[1].points_for_win", "2,5").await?;
        gui.expect_entered("metrics[1].points_for_win", "2.5")
            .await?;
        gui.expect_issues(&[]).await?;

        gui.enter("metrics[1].points_for_win", "abc").await?;

        gui.expect_issues(&["“abc” is no number."]).await?;
        gui.expect_entered("metrics[1].points_for_win", "abc").await
    })
    .await;
}

/// In German, a number is written with a comma before its decimals.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_number_is_shown_with_a_comma_in_german() {
    run_in_new_competition(async |gui| {
        gui.choose_language("Deutsch").await?;

        gui.enter("metrics[1].points_for_win", "2.5").await?;

        gui.expect_entered("metrics[1].points_for_win", "2,5").await
    })
    .await;
}

/// What is said about a list or a plugin in it shows while the pointer is on its button, and from a
/// click on it until the next; Escape hides it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn what_is_said_about_a_plugin_shows_on_hover_and_after_a_click() {
    const POINTS: &str = "scored for every win, every draw and every bye";
    run_in_new_competition(async |gui| {
        gui.expect_toggletip(None).await?;

        gui.hover_labelled("About order of the standings").await?;

        gui.expect_toggletip(Some(
            "best first, every criterion only breaks the ties of the ones before it",
        ))
        .await?;
        gui.press(Key::Escape).await?;
        gui.expect_toggletip(None).await?;
        // a criterion has the description of its metric
        gui.hover_labelled("About more points").await?;
        gui.expect_toggletip(Some(POINTS)).await?;
        gui.hover_labelled("About format").await?;
        gui.expect_toggletip(Some("how the matches are scheduled"))
            .await?;
        gui.click_labelled("About points").await?;
        gui.expect_toggletip(Some(POINTS)).await?;
        gui.page.check_accessibility().await?;
        gui.click_labelled("About points").await?;
        gui.expect_toggletip(None).await?;
        gui.click_labelled("About points").await?;
        gui.expect_toggletip(Some(POINTS)).await?;
        gui.press(Key::Escape).await?;
        gui.expect_toggletip(None).await
    })
    .await;
}

/// The options of a plugin show their labels only, and the description of the one under the
/// pointer below them.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_description_of_an_option_shows_while_the_pointer_is_on_it() {
    run_in_new_competition(async |gui| {
        gui.open_choice("ranking").await?;

        gui.hover_option("Fewer matches played").await?;

        gui.expect_option_description("the same result out of fewer matches ranks higher")
            .await?;
        gui.page.check_accessibility().await?;
        gui.hover_option("Later first loss").await?;
        gui.expect_option_description("by the round of the first loss; never having lost is best")
            .await?;
        // a criterion without a description of its own has that of its metric
        gui.hover_option("More Buchholz").await?;
        gui.expect_option_description("the points of all opponents")
            .await
    })
    .await;
}

/// A touch screen shows the description of an option by a tap on its ⓘ, which does not choose it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_tap_on_the_info_of_an_option_shows_its_description_without_choosing_it() {
    run_in_new_competition(async |gui| {
        gui.open_choice("ranking").await?;

        gui.tap_option_info("Fewer matches played").await?;

        gui.expect_option_description_tapped_open(
            "the same result out of fewer matches ranks higher",
        )
        .await?;
        gui.page.check_accessibility().await?;
        gui.expect_list("ranking", &["More points", "More Sonneborn-Berger"])
            .await?;
        gui.choose_option("Fewer matches played").await?;
        gui.expect_list(
            "ranking",
            &[
                "More points",
                "More Sonneborn-Berger",
                "Fewer matches played",
            ],
        )
        .await
    })
    .await;
}

/// The dialog cuts off what reaches out of it, so what is said about a field at its right and
/// bottom edges shows to the left of its button and above it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn what_is_said_about_a_field_at_the_right_of_the_dialog_shows_whole() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui).await?;
        gui.click("Edit phase").await?;

        gui.hover_labelled("About weight of an opponent played to a draw")
            .await?;

        gui.expect_toggletip(Some("the points of that opponent are multiplied by this"))
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_phase_is_edited_after_the_start_in_a_dialog() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui).await?;
        gui.click_labelled("Alice wins").await?;
        // every match played, the phase is folded away
        gui.unfold_phase().await?;

        gui.click("Edit phase").await?;
        gui.click_labelled("Move more Sonneborn-Berger up").await?;
        gui.page.check_accessibility().await?;
        gui.click("Save changes").await?;

        gui.expect_no_dialog().await?;
        gui.expect_phase(&[
            "Round robin",
            "More Sonneborn-Berger, more points",
            "Win-draw-loss record",
            "Points",
            "Sonneborn-Berger",
        ])
        .await?;
        gui.expect_results(&["Alice won"]).await?;
        gui.click("Undo").await?;
        gui.expect_phase(&[
            "Round robin",
            "More points, more Sonneborn-Berger",
            "Win-draw-loss record",
            "Points",
            "Sonneborn-Berger",
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
        gui.click_labelled("Alice wins").await?;
        gui.unfold_phase().await?;
        gui.click("Edit phase").await?;

        gui.choose_format("Monster DYP").await?;
        gui.click("Save changes").await?;

        gui.expect_dialog_error(
            "The phase cannot be changed this way, since a later change no longer fits it: match \
             does not exist.",
        )
        .await?;
        gui.click("Cancel").await?;
        gui.expect_no_dialog().await?;
        gui.expect_phase(&[
            "Round robin",
            "More points, more Sonneborn-Berger",
            "Win-draw-loss record",
            "Points",
            "Sonneborn-Berger",
        ])
        .await
    })
    .await;
}
