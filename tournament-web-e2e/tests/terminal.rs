use tournament_web_e2e::{
    run,
    thirtyfour::{By, Key, extensions::query::ElementQueryable, stringmatch::StringMatch},
};

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_line_typed_shows_after_the_prompt_followed_by_what_it_shows() {
    run(async |terminal| {
        terminal.run("add Alice Bob").await?;

        terminal
            .driver
            .query(By::Css("#output .line"))
            .with_text("tournament> add Alice Bob")
            .first()
            .await?;
        terminal
            .expect_reply("2 entrant(s) added, 2 in total")
            .await?;
        terminal.expect_prompt("tournament*> ").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_failing_command_shows_its_error() {
    run(async |terminal| {
        terminal.run("remove Alice").await?;

        terminal
            .expect_error("error: there is no entrant named \"Alice\"")
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn the_arrow_keys_walk_through_the_lines_typed_so_far() {
    run(async |terminal| {
        terminal.run("add Alice").await?;
        terminal.run("entrants").await?;

        terminal.press(Key::Up).await?;
        terminal.expect_input("entrants").await?;
        terminal.press(Key::Up).await?;
        terminal.expect_input("add Alice").await?;
        terminal.press(Key::Up).await?;
        terminal.expect_input("add Alice").await?;
        terminal.press(Key::Down).await?;
        terminal.expect_input("entrants").await?;
        terminal.press(Key::Down).await?;
        terminal.expect_input("").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_tournament_is_played_from_start_to_ranking() {
    run(async |terminal| {
        terminal.run("add Alice Bob Carol Dave").await?;
        terminal.run("add-phase round-robin").await?;
        terminal.run("start").await?;
        terminal
            .expect_reply("competition started, 6 match(es) scheduled")
            .await?;

        terminal.run("report 1 a").await?;
        terminal
            .expect_reply(StringMatch::new("beat").partial())
            .await?;

        terminal.run("ranking").await?;
        terminal
            .expect_reply(StringMatch::new("1-0-0").partial())
            .await
    })
    .await;
}
