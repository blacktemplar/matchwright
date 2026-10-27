//! The page as an app: offline, installed, and what it says while the browser may not keep what is
//! stored, which headless Chrome refuses to.

use tournament_gui_e2e::{COMPETITION, run_in_new_competition};

/// The warning, in English, while the browser offers to install the page, as headless Chrome does.
const NOT_PERSISTED: [&str; 2] = [
    "This browser may not keep the competitions stored here, e.g. in a private window or when it \
     runs short of space. Export them to keep a copy.",
    "Installed as an app, the page is usually allowed to keep them.",
];

/// At a venue without a network, a page opened before still opens, with what is stored.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn a_page_opened_before_opens_offline() {
    run_in_new_competition(async |gui| {
        gui.add_entrant("Alice").await?;
        gui.page.expect_kept_offline().await?;

        gui.page.go_offline().await;
        gui.page.reload().await?;

        gui.expect_open(COMPETITION).await?;
        gui.expect_entrants(&["Alice"]).await?;
        gui.add_entrant("Bob").await?;
        gui.expect_entrants(&["Alice", "Bob"]).await
    })
    .await;
}

/// The tab names the competition shown, in plain text without the marks isolating it.
#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_tab_is_titled_by_the_competition() {
    run_in_new_competition(async |gui| {
        gui.page.expect_title("Cup – Tournament").await?;

        gui.follow("All competitions").await?;

        gui.page.expect_title("Tournament").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_page_can_be_installed() {
    run_in_new_competition(async |gui| gui.page.expect_installable().await).await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_warning_that_the_browser_may_not_keep_competitions_offers_to_export() {
    run_in_new_competition(async |gui| {
        gui.expect_storage_warning(&NOT_PERSISTED).await?;

        gui.click("Export this competition").await?;

        gui.page.expect_download("Cup.json").await?;
        Ok(())
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_warning_that_the_browser_may_not_keep_competitions_can_be_dismissed_for_good() {
    run_in_new_competition(async |gui| {
        gui.expect_storage_warning(&NOT_PERSISTED).await?;

        gui.click("Don’t show again").await?;

        gui.expect_storage_warning(&[]).await?;
        gui.page.reload().await?;
        gui.expect_open(COMPETITION).await?;
        gui.expect_storage_warning(&[]).await
    })
    .await;
}
