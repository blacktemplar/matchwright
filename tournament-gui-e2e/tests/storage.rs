use tournament_gui_e2e::{
    COMPETITION, Gui, run, run_in_new_competition,
    thirtyfour::{By, error::WebDriverResult, stringmatch::StringMatch},
};

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_competition_is_kept_in_the_browser() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;

        gui.page.reload().await?;

        gui.expect_open(COMPETITION).await?;
        gui.expect_entrants(&["Alice"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_competitions_stored_are_listed_to_open() {
    run(async |gui| {
        gui.expect_competitions(&[]).await?;
        gui.new_competition("Cup").await?;
        gui.add_entrant("Alice").await?;
        gui.follow("All competitions").await?;
        gui.new_competition("League").await?;
        gui.follow("All competitions").await?;

        // the latest first
        gui.expect_competitions(&["League", "Cup"]).await?;
        gui.expect_progress(&["Not started · 0 entrants", "Not started · one entrant"])
            .await?;
        gui.open_competition("Cup").await?;
        gui.expect_entrants(&["Alice"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_competition_is_renamed_and_deleted_from_the_list() {
    run(async |gui| {
        gui.new_competition("Cup").await?;
        gui.add_entrant("Alice").await?;
        gui.follow("All competitions").await?;
        gui.new_competition("League").await?;
        gui.follow("All competitions").await?;

        gui.rename_competition("Cup", "Cup 2026").await?;

        gui.expect_no_dialog().await?;
        gui.expect_competitions(&["League", "Cup 2026"]).await?;
        gui.delete_competition("League").await?;
        gui.expect_competitions(&["Cup 2026"]).await?;
        gui.page.check_accessibility().await?;
        gui.open_competition("Cup 2026").await?;
        gui.expect_entrants(&["Alice"]).await
    })
    .await;
}

/// A tab showing a competition would go on saving it, so it is not deleted meanwhile.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_competition_open_in_another_tab_is_not_deleted() {
    run_in_new_competition(async |gui| {
        let showing = gui.page.tab().await?;
        gui.page.open_tab().await?;
        gui.follow("All competitions").await?;

        gui.delete_competition(COMPETITION).await?;

        gui.expect_dialog_error("The competition is open in another tab; close it there first.")
            .await?;
        gui.click("Cancel").await?;
        gui.expect_competitions(&[COMPETITION]).await?;
        gui.page.close_tab(&showing).await?;
        gui.expect_open(COMPETITION).await
    })
    .await;
}

/// Two tabs changing a competition would fork its log, so only one of them does.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_competition_open_in_another_tab_is_only_shown_and_followed() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        let first = gui.page.tab().await?;

        let second = gui.page.open_tab().await?;

        gui.expect_read_only(true).await?;
        gui.expect_entrants(&["Alice"]).await?;
        gui.expect_disabled("Add entrant").await?;
        gui.page.switch_to(&first).await?;
        gui.expect_read_only(false).await?;
        gui.add_entrant("Bob").await?;
        gui.page.switch_to(&second).await?;
        gui.expect_entrants(&["Alice", "Bob"]).await?;
        gui.page.check_accessibility().await?;

        gui.page.switch_to(&first).await?;
        gui.page.close_tab(&second).await?;

        gui.expect_read_only(false).await?;
        gui.add_entrant("Carol").await?;
        gui.expect_entrants(&["Alice", "Bob", "Carol"]).await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn an_exported_competition_is_imported_as_another_one() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.click("Export").await?;
        let file = gui.page.expect_download("Cup.json").await?;
        gui.follow("All competitions").await?;

        gui.import(&file).await?;

        gui.expect_open("Cup (2)").await?;
        gui.expect_entrants(&["Alice"]).await?;
        gui.follow("All competitions").await?;
        // the copy told apart from it, and listed first, as the latest
        gui.expect_competitions(&["Cup (2)", COMPETITION]).await
    })
    .await;
}

/// A file that is no competition is refused in the operator's language, with where reading it
/// stopped folded away below.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_file_that_is_no_competition_is_refused_with_its_detail_apart() {
    run(async |gui| {
        let file =
            std::env::temp_dir().join(format!("not-a-competition-{}.json", std::process::id()));
        std::fs::write(&file, "<html>").unwrap();

        gui.import(&file).await?;

        gui.expect_error("The file is no competition this version can open.")
            .await?;
        gui.expect_error_detail("line 1 column 1").await?;
        gui.page.check_accessibility().await?;
        std::fs::remove_file(file).unwrap();
        Ok(())
    })
    .await;
}

/// After an update, say, a log may hold what the page no longer knows; the operator can still keep
/// a copy of it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_competition_that_no_longer_loads_is_still_exported() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.follow("All competitions").await?;
        append_unknown_entry(&gui).await?;

        gui.follow(COMPETITION).await?;

        gui.page
            .expect(
                By::Css("[role=alert]"),
                |query| query.with_text(StringMatch::new("of the log cannot be read").partial()),
                "why the competition does not load",
            )
            .await?;
        gui.follow("All competitions").await?;
        gui.click_labelled("Export Cup").await?;
        let file = gui.page.expect_download("Cup.json").await?;
        let file = std::fs::read_to_string(file).unwrap();
        assert!(file.contains("Alice") && file.contains("unknown"));
        Ok(())
    })
    .await;
}

/// Appends an entry no page knows to the log of the only competition stored.
async fn append_unknown_entry(gui: &Gui) -> WebDriverResult<()> {
    gui.driver
        .execute_async(
            r#"
            const done = arguments[arguments.length - 1];
            const request = indexedDB.open("tournament-gui");
            request.onsuccess = () => {
                const database = request.result;
                const transaction = database.transaction("entries", "readwrite");
                const entries = transaction.objectStore("entries");
                entries.getAllKeys().onsuccess = (event) => {
                    const keys = event.target.result;
                    entries.put('{"unknown": true}', [keys[0][0], keys.length]);
                };
                transaction.oncomplete = () => {
                    database.close();
                    done();
                };
            };
            "#,
            Vec::new(),
        )
        .await?;
    Ok(())
}
