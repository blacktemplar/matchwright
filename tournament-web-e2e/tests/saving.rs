use tournament_web_e2e::run;

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn saving_names_the_competition_in_the_url_so_reloading_opens_it_again() {
    run(async |terminal| {
        terminal.run("add Alice").await?;
        terminal.run("save cup").await?;

        terminal.expect_prompt("cup> ").await?;
        assert_eq!(terminal.driver.current_url().await?.fragment(), Some("cup"));

        terminal.reload().await?;
        terminal.expect_prompt("cup> ").await?;
        terminal.run("entrants").await?;
        terminal.expect_reply("Alice").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_name_in_the_url_nothing_is_saved_under_starts_a_competition_saved_under_it() {
    run(async |terminal| {
        terminal.open_named("league").await?;
        terminal.expect_prompt("league> ").await?;

        terminal.run("add Alice").await?;
        terminal.run("save").await?;

        terminal.open_named("league").await?;
        terminal.run("entrants").await?;
        terminal.expect_reply("Alice").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn leaving_the_page_with_unsaved_changes_asks_first() {
    run(async |terminal| {
        terminal.run("add Alice").await?;

        assert!(terminal.leave().await?);

        // staying keeps the changes
        terminal.run("entrants").await?;
        terminal.expect_reply("Alice").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn leaving_the_page_after_saving_does_not_ask() {
    run(async |terminal| {
        terminal.run("add Alice").await?;
        terminal.run("save cup").await?;

        assert!(!terminal.leave().await?);
        Ok(())
    })
    .await;
}
