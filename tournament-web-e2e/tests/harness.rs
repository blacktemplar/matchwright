use tournament_web_e2e::run;

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
#[should_panic(expected = "the page must report no errors")]
async fn an_error_the_page_logs_fails_the_test() {
    run(async |terminal| {
        terminal
            .driver
            .execute("console.error('broken')", Vec::new())
            .await?;
        Ok(())
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
#[should_panic(expected = "must have the text \"Bob\", but has the text \"Alice\"")]
async fn an_expectation_not_met_fails_with_what_the_page_shows() {
    run(async |terminal| {
        terminal.run("add Alice").await?;
        terminal.run("entrants").await?;

        terminal.expect_reply("Bob").await
    })
    .await;
}
