use std::{fs, panic::AssertUnwindSafe};

use futures_util::FutureExt;
use tournament_web_e2e::{failures, run};

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_failing_test_leaves_a_screenshot_and_a_report_on_the_page() {
    let failed = AssertUnwindSafe(run(async |terminal| {
        terminal.run("add Alice").await?;
        panic!("the test fails");
    }))
    .catch_unwind()
    .await;

    assert!(failed.is_err());
    let screenshot = fs::read(failures().join("screenshot.png")).unwrap();
    assert!(screenshot.starts_with(b"\x89PNG"));
    let report = fs::read_to_string(failures().join("report.txt")).unwrap();
    assert!(report.contains("1 entrant(s) added"), "{report}");
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn the_report_holds_the_error_the_page_logged() {
    let failed = AssertUnwindSafe(run(async |terminal| {
        terminal
            .driver
            .execute("console.error('broken')", Vec::new())
            .await?;
        Ok(())
    }))
    .catch_unwind()
    .await;

    assert!(failed.is_err());
    let report = fs::read_to_string(failures().join("report.txt")).unwrap();
    assert!(report.contains("broken"), "{report}");
}

#[tokio::test]
#[ignore = "needs Chrome and the page built by trunk; run by the web-e2e hook"]
async fn a_passing_test_leaves_nothing() {
    run(async |terminal| terminal.run("add Alice").await).await;

    assert!(!failures().exists());
}

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
#[should_panic(expected = "the page must be accessible, but violates")]
async fn an_inaccessible_page_fails_the_test() {
    run(async |terminal| {
        // an image without a text alternative
        terminal
            .driver
            .execute(
                "document.body.append(document.createElement('img'))",
                Vec::new(),
            )
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
