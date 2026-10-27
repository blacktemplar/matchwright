use tournament_gui_e2e::{Gui, run, run_in_new_competition, thirtyfour::error::WebDriverResult};

const LONG_NAME: &str = "Maximilian-Alexander Müller-Lüdenscheidt";

/// Enters `names`, starts a knockout of them in `format`, and shows its bracket.
async fn show_bracket(gui: &Gui, format: &str, names: &[&str]) -> WebDriverResult<()> {
    for name in names {
        gui.add_entrant(name).await?;
    }
    gui.choose_format(format).await?;
    gui.click("Add phase").await?;
    gui.click("Start").await?;
    gui.click("Bracket").await
}

/// Long names wrap within their pairing, which keeps its column's width, so the lines meet.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_lines_of_the_bracket_meet_whatever_the_names() {
    run_in_new_competition(async |gui| {
        let names = [
            LONG_NAME, "Bo", "Carol", "Dave", "Erin", "Frank", "Grace", "Heidi",
        ];
        show_bracket(&gui, "single elimination", &names).await?;
        gui.win_in_bracket("1").await?;

        gui.expect_bracket_lines_meet().await?;
        gui.page.resize(390, 844).await?;
        gui.expect_bracket_lines_meet().await?;
        gui.page.expect_room_for_words(".pairing .side").await?;
        gui.page.expect_no_sideways_scroll().await
    })
    .await;
}

/// A double elimination of 6 has byes, and a loser bracket whose rounds alternate between
/// dropping losers in and halving.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_lines_of_a_double_elimination_meet() {
    run_in_new_competition(async |gui| {
        let names = ["Alice", "Bob", "Carol", "Dave", "Erin", LONG_NAME];
        show_bracket(&gui, "double elimination", &names).await?;

        gui.expect_bracket_lines_meet().await
    })
    .await;
}

/// A knockout of 16 has four rounds, which fit next to each other on a tablet held sideways.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_bracket_of_sixteen_fits_on_a_tablet() {
    run_in_new_competition(async |gui| {
        let names: Vec<String> = (1..=16).map(|number| format!("Player {number}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        gui.page.resize(820, 1180).await?;
        show_bracket(&gui, "single elimination", &names).await?;

        gui.expect_bracket_lines_meet().await?;
        gui.expect_bracket_fits().await
    })
    .await;
}

/// Two byes meeting in the loser bracket leave their place empty, and the lines still meet.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_pairing_never_played_keeps_its_place() {
    run_in_new_competition(async |gui| {
        let names = ["Alice", "Bob", "Carol", "Dave", "Erin"];
        show_bracket(&gui, "double elimination", &names).await?;

        gui.expect_bracket_lines_meet().await?;
        gui.page.check_accessibility().await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_standings_scroll_inside_their_card_on_a_phone() {
    run_in_new_competition(async |gui| {
        gui.page.resize(360, 740).await?;
        gui.choose_language("Deutsch").await?;
        for name in [LONG_NAME, "Bob", "Carol"] {
            gui.add_entrant(name).await?;
        }
        gui.choose_format("Jeder gegen jeden").await?;
        gui.click("Phase hinzufügen").await?;
        gui.click("Starten").await?;

        gui.click("Tabelle").await?;

        gui.page.expect_no_sideways_scroll().await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn entrants_break_no_word_before_the_start_on_a_tablet() {
    run_in_new_competition(async |gui| {
        gui.page.resize(768, 1024).await?;

        gui.add_entrant(LONG_NAME).await?;
        gui.add_entrant("Zoë Ångström").await?;

        gui.page
            .expect_room_for_words("#entrant-list .name")
            .await?;
        gui.page.expect_no_sideways_scroll().await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_bar_keeps_room_for_the_competition_in_german() {
    run(async |gui| {
        gui.new_competition("Stadtmeisterschaft 2026").await?;
        gui.choose_language("Deutsch").await?;

        // a phone's bar leaves too little room for a long word, which ends in "…" there
        for width in [700, 940, 980] {
            gui.page.resize(width, 900).await?;
            gui.page.expect_room_for_words(".bar h1").await?;
            gui.page.expect_no_sideways_scroll().await?;
        }
        gui.page.resize(360, 900).await?;
        gui.page.expect_no_sideways_scroll().await
    })
    .await;
}
