//! The help, which the footer of every screen links to beside the source code.

use tournament_gui_e2e::{
    run, run_in_new_competition,
    thirtyfour::{By, extensions::query::ElementQueryable as _},
};

const REPOSITORY: &str = "https://github.com/blacktemplar/matchwright";

/// The help says that the competitions stay in the browser, leads to the source code and names the
/// media owner (the one the gui-e2e hook builds the page with); it is a page of its own, kept by a
/// reload, with the way back to the competitions.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_help_says_where_the_competitions_are_kept() {
    run_in_new_competition(async |gui| {
        gui.follow("Help").await?;

        gui.page.expect_title("Help – Matchwright").await?;
        gui.page
            .expect_texts(
                By::Css("main h2"),
                &[
                    "Your data stays on your device",
                    "Offline use and installation",
                    "Open source",
                ],
            )
            .await?;
        let source = gui.driver.query(By::Css("main a")).first().await?;
        assert_eq!(source.attr("href").await?.as_deref(), Some(REPOSITORY));
        gui.page
            .expect_texts(
                By::Css("main section:last-child p:last-child"),
                &["Media owner (disclosure under § 25 MedienG): Jane Doe, Vienna, Austria."],
            )
            .await?;
        gui.page.check_accessibility().await?;

        gui.page.reload().await?;
        gui.page.expect_title("Help – Matchwright").await?;

        gui.follow("All competitions").await?;
        gui.expect_competitions(&["Cup"]).await
    })
    .await;
}

/// Every screen's footer links to the source code.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_footer_links_to_the_source_code() {
    run(async |gui| {
        let source = gui
            .driver
            .query(By::LinkText("Source code"))
            .first()
            .await?;
        assert_eq!(source.attr("href").await?.as_deref(), Some(REPOSITORY));
        Ok(())
    })
    .await;
}
