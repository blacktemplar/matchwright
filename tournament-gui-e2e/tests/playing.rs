use tournament_gui_e2e::{
    Gui, Key, run_in_new_competition,
    thirtyfour::{By, error::WebDriverResult},
};

/// Enters `names` and starts a round robin of them.
async fn start_round_robin(gui: &Gui, names: &[&str]) -> WebDriverResult<()> {
    for name in names {
        gui.add_entrant(name).await?;
    }
    gui.choose_format("Round robin").await?;
    gui.click("Add phase").await?;
    gui.click("Start").await
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_reported_result_shows_in_the_match_and_the_standings() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.click_labelled("Bob wins").await?;

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
        gui.click_labelled("Bob wins").await?;

        gui.click_labelled("Reopen match 1").await?;

        gui.expect_results(&[]).await?;
        gui.click_labelled("Alice wins").await?;
        gui.expect_results(&["Alice won"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undo_takes_back_a_result_and_redo_brings_it_back() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.click_labelled("Alice wins").await?;

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

        gui.choose_round("Round 2 (2 open)").await?;

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
async fn the_matches_shown_are_those_of_the_entrants_found() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.find_entrant("carol").await?;

        gui.expect_matches(&[]).await?;
        gui.page
            .expect_texts(
                By::Css(".empty"),
                &["No entrant here has a name containing “carol”."],
            )
            .await?;
        gui.page.check_accessibility().await?;
        gui.find_entrant("ALI").await?;
        gui.expect_matches(&["1"]).await
    })
    .await;
}

/// Of three, one has a bye in every round, and is found as well.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn an_entrant_found_with_a_bye_is_not_said_to_be_missing() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob", "Carol"]).await?;

        for name in ["Alice", "Bob", "Carol"] {
            gui.find_entrant(name).await?;
            gui.page.expect_texts(By::Css(".empty"), &[]).await?;
        }
        Ok(())
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_result_is_typed_in_by_the_number_of_its_match() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.enter_result("2 1").await?;
        gui.expect_result_entry_error("There is no match 2.")
            .await?;
        gui.enter_result("1 x").await?;

        gui.expect_no_error().await?;
        gui.expect_results(&["Draw"]).await?;
        gui.expect_announcement("Match 1: Draw. Every match has a result.")
            .await
    })
    .await;
}

/// A column of the standings says what its metric is, by its whole name where its header
/// abbreviates it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_column_of_the_standings_says_what_it_shows() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.click_labelled("About SB").await?;

        gui.expect_toggletip(Some(
            "Sonneborn-Berger: the points of the opponents that were beaten or played to a draw",
        ))
        .await?;
        gui.page.check_accessibility().await?;
        gui.click_labelled("About Points").await?;
        gui.expect_toggletip(Some("scored for every win, every draw and every bye"))
            .await
    })
    .await;
}

/// The entry of results typed in is closed until opened, and stays as the operator left it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_entry_of_results_stays_as_it_was_left() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.expect_result_entry_open(false).await?;

        gui.toggle_result_entry().await?;

        gui.expect_result_entry_open(true).await?;
        gui.page.check_accessibility().await?;
        gui.page.reload().await?;
        gui.expect_result_entry_open(true).await?;
        gui.toggle_result_entry().await?;
        gui.page.reload().await?;
        gui.expect_result_entry_open(false).await
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

/// The final and the match for third place wait for the semifinals in the bracket, and take
/// their entrants as those are decided; they show the numbers they are named by before that.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_bracket_shows_who_meets_whom() {
    run_in_new_competition(async |gui| {
        for name in ["Alice", "Bob", "Carol", "Dave"] {
            gui.add_entrant(name).await?;
        }
        gui.choose_format("Single elimination").await?;
        gui.click("Add phase").await?;
        gui.click("Start").await?;

        gui.click("Bracket").await?;
        gui.expect_bracket_stages(&["Semifinals", "Final", "Third place"])
            .await?;
        gui.expect_bracket_waiting(&["Winner of 1", "Winner of 2", "Loser of 2", "Loser of 1"])
            .await?;
        gui.expect_bracket_numbers(&["1", "2", "3", "4"]).await?;
        gui.page.check_accessibility().await?;

        gui.click("Matches & standings").await?;
        gui.enter_result("1 1").await?;

        gui.click("Bracket").await?;
        gui.expect_bracket_waiting(&["Winner of 2", "Loser of 2"])
            .await
    })
    .await;
}

/// A click on a side's name in the bracket reports that it won, and the focus moves on to the
/// next match without a result, the final once both semifinals have one.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn results_are_reported_in_the_bracket() {
    run_in_new_competition(async |gui| {
        for name in ["Alice", "Bob", "Carol", "Dave"] {
            gui.add_entrant(name).await?;
        }
        gui.choose_format("Single elimination").await?;
        gui.click("Add phase").await?;
        gui.click("Start").await?;
        gui.click("Bracket").await?;

        gui.win_in_bracket("1").await?;

        gui.expect_bracket_waiting(&["Winner of 2", "Loser of 2"])
            .await?;
        gui.expect_focus_in_bracket("2").await?;

        gui.win_in_bracket("2").await?;

        gui.expect_bracket_waiting(&[]).await?;
        gui.expect_focus_in_bracket("3").await?;
        gui.page.check_accessibility().await
    })
    .await;
}

/// A semifinal whose winner played the final is reopened only after the final, which the refusal
/// names.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn reopening_a_result_a_later_one_depends_on_names_that_one() {
    run_in_new_competition(async |gui| {
        for name in ["Alice", "Bob", "Carol", "Dave"] {
            gui.add_entrant(name).await?;
        }
        gui.choose_format("Single elimination").await?;
        gui.toggle("Match for third place").await?;
        gui.click("Add phase").await?;
        gui.click("Start").await?;
        for entry in ["1 1", "2 1", "3 1"] {
            gui.enter_result(entry).await?;
        }
        gui.choose_round("Round 1").await?;

        gui.click_labelled("Reopen match 1").await?;

        gui.expect_error("The result of match 3 depends on this one; reopen that match first.")
            .await
    })
    .await;
}

/// The bracket, once chosen, is shown first on the next visit.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_bracket_chosen_is_shown_again_after_a_reload() {
    run_in_new_competition(async |gui| {
        for name in ["Alice", "Bob"] {
            gui.add_entrant(name).await?;
        }
        gui.choose_format("Single elimination").await?;
        gui.click("Add phase").await?;
        gui.click("Start").await?;

        gui.click("Bracket").await?;
        gui.page.reload().await?;

        gui.expect_view("Bracket").await?;
        gui.click("Matches & standings").await?;
        gui.page.reload().await?;
        gui.expect_view("Matches & standings").await
    })
    .await;
}

/// Once every match is played, the screen says who won, the first places stand out, the phase
/// folds away, and the list of competitions names the winner too; undoing the last result takes
/// it all back.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_winner_is_shown_once_every_match_is_played() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;
        gui.expect_winners(None).await?;
        gui.expect_podium(&[]).await?;

        gui.click_labelled("Alice wins").await?;

        gui.expect_winners(Some("Winner: Alice")).await?;
        gui.expect_podium(&["Alice", "Bob"]).await?;
        gui.page.check_accessibility().await?;
        gui.click("Undo").await?;
        gui.expect_winners(None).await?;
        gui.click("Redo").await?;
        gui.follow("All competitions").await?;
        gui.expect_progress(&["Finished · winner: Alice"]).await
    })
    .await;
}

/// Teams tied for first are all named, in German as well.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn teams_tied_for_first_are_named_together() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.click("Draw").await?;

        gui.expect_winners(Some("Tied for first: Alice, Bob"))
            .await?;
        gui.choose_language("Deutsch").await?;
        gui.expect_winners(Some("Geteilter erster Platz: Alice, Bob"))
            .await
    })
    .await;
}

/// Undoing the start leaves play, back to the setup, so the operator is asked first.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn undoing_the_start_is_asked_about_first() {
    run_in_new_competition(async |gui| {
        start_round_robin(&gui, &["Alice", "Bob"]).await?;

        gui.click("Undo").await?;
        gui.page.check_accessibility().await?;
        gui.click("Cancel").await?;

        gui.expect_no_dialog().await?;
        gui.expect_matches(&["1"]).await?;
        gui.click("Undo").await?;
        gui.click("Undo the start").await?;
        gui.expect_no_dialog().await?;
        gui.expect_entrants(&["Alice", "Bob"]).await?;
        gui.expect_announcement("Undone: the start.").await
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
