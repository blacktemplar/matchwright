use tournament_gui_e2e::{run, run_in_new_competition};

const LONG_NAME: &str = "Maximilian-Alexander Müller-Lüdenscheidt";

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
