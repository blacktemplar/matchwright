use tournament_web_e2e::{run, thirtyfour::stringmatch::StringMatch};

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_phase_without_a_format_is_configured_by_answering_its_questions() {
    run(async |terminal| {
        terminal.run("add-phase").await?;

        terminal.expect_prompt("number [1]> ").await?;
        terminal
            .expect_output(StringMatch::new("1. round robin").partial())
            .await?;

        terminal.accept_every_question().await?;

        terminal
            .expect_reply(
                "phase added: round robin, ranked by more points, then more Sonneborn-Berger",
            )
            .await?;
        terminal.expect_prompt("tournament*> ").await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn editing_the_phase_changes_the_answer_given() {
    run(async |terminal| {
        terminal.run("add-phase random-rounds rounds=3").await?;
        terminal.run("edit-phase").await?;

        // the format, then its rounds
        terminal.run("").await?;
        terminal.expect_prompt("whole number [3]> ").await?;
        terminal.run("5").await?;
        terminal.accept_every_question().await?;

        terminal
            .expect_reply(StringMatch::new("phase changed: random rounds (rounds: 5)").partial())
            .await
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn cancel_stops_the_questions_and_leaves_the_competition_unchanged() {
    run(async |terminal| {
        terminal.run("add-phase").await?;

        terminal.run("cancel").await?;

        terminal.expect_reply("cancelled, no phase added").await?;
        terminal.expect_prompt("tournament> ").await
    })
    .await;
}
