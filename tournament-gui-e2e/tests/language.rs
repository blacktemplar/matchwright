use tournament_gui_e2e::{
    run, run_in_new_competition,
    thirtyfour::{By, stringmatch::StringMatch},
};

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_page_speaks_the_language_the_browser_prefers() {
    run(async |gui| {
        gui.expect_page_language("en").await?;
        gui.page.expect_text(By::Css("h1"), "Tournamo").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn everything_is_said_in_the_language_chosen() {
    run_in_new_competition(async |gui| {
        gui.click("Add entrant").await?;
        gui.expect_error("An entrant name must not be empty.")
            .await?;

        gui.choose_language("Deutsch").await?;

        gui.expect_page_language("de").await?;
        gui.expect_error("Der Name eines Teilnehmers darf nicht leer sein.")
            .await?;
        gui.add_entrant("Alice").await?;
        gui.add_entrant("Bob").await?;
        gui.choose_format("Jeder gegen jeden").await?;
        gui.click("Phase hinzufügen").await?;
        gui.click("Starten").await?;
        gui.click_labelled("Alice gewinnt").await?;
        gui.expect_results(&["Alice hat gewonnen"]).await?;
        gui.page
            .expect_text(By::Css("#standing-list th"), "Punkte")
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_stages_are_said_in_the_language_chosen() {
    run_in_new_competition(async |gui| {
        gui.choose_language("Deutsch").await?;
        for name in ["Alice", "Bob", "Carol", "Dave"] {
            gui.add_entrant(name).await?;
        }
        gui.choose_format("K.-o.-System").await?;
        gui.click("Phase hinzufügen").await?;

        gui.click("Starten").await?;

        gui.expect_stages(&["Halbfinale", "Halbfinale"]).await?;
        // the round of the loss, as the standings show it
        gui.click_labelled("Alice gewinnt").await?;
        gui.page
            .expect(
                By::Css("#standing-list tbody"),
                |query| query.with_text(StringMatch::new("Runde").partial()),
                "the round lost in, in German",
            )
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_language_chosen_is_kept_for_the_next_visit() {
    run(async |gui| {
        gui.choose_language("Deutsch").await?;

        gui.page.reload().await?;

        gui.expect_page_language("de").await?;
        gui.page
            .expect_text(By::Id("new-competition"), "Neues Turnier")
            .await
    })
    .await;
}
