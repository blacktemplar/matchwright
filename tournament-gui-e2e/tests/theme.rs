//! The colours of the page: the system's, or light or dark as chosen.

use tournament_gui_e2e::run;

#[tokio::test]
#[ignore = "needs Chrome and the page built by dx; run by the gui-e2e hook"]
async fn the_colours_chosen_are_kept_for_the_next_visit() {
    run(async |gui| {
        gui.expect_theme(None).await?;

        gui.choose_theme("Dark").await?;

        gui.expect_theme(Some("dark")).await?;
        gui.page.check_accessibility().await?;
        gui.page.reload().await?;
        gui.expect_theme(Some("dark")).await?;
        gui.choose_theme("Like the system").await?;
        gui.expect_theme(None).await
    })
    .await;
}
