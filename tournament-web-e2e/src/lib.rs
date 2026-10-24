//! End-to-end tests of `tournament-web`: [`run`] opens the page in headless Chrome, and the test
//! uses it through a [`Terminal`], the way an operator does, by typing into its input.
//!
//! The page is served from `tournament-web/dist`, which `trunk build` has to have built before.
//! Everything else, from the chromedriver to what a failing test leaves and the environment
//! variables that show a test in a window, is that of [`e2e_harness`].

use std::{fmt::Debug, path::Path};

use e2e_harness::{
    Page, Site,
    thirtyfour::{
        By, Key, WebDriver, WebElement, error::WebDriverResult,
        extensions::query::ElementQueryable, stringmatch::Needle,
    },
};
pub use e2e_harness::{failures, thirtyfour};

/// What a line run shows that is not the line itself, the last one.
const LAST_REPLY: &str = "(//div[@id='output']/div[not(contains(@class, 'line'))])[last()]";

/// Runs `test` on the page freshly opened, with nothing saved yet, see [`e2e_harness::run`].
///
/// # Panics
/// Panics if `test` fails, if the page logs an error, or if it is not accessible at the end.
pub async fn run(test: impl AsyncFnOnce(Terminal) -> WebDriverResult<()>) {
    let site = Site {
        dist: Path::new(env!("CARGO_MANIFEST_DIR")).join("../tournament-web/dist"),
        build: "cd tournament-web && trunk build",
        // shown once the WebAssembly runs
        ready: By::Id("prompt"),
    };
    e2e_harness::run(site, async |page| {
        test(Terminal {
            driver: page.driver.clone(),
            page,
        })
        .await
    })
    .await;
}

/// The command line of the page.
///
/// After each of its actions (opening, reloading or leaving the page, running a line, pressing a
/// key), a test pauses as long as `E2E_PAUSE` says.
#[derive(Clone)]
pub struct Terminal {
    pub driver: WebDriver,
    page: Page,
}

impl Terminal {
    /// Opens the page with `name` after the `#` of its URL, i.e. the competition saved under it,
    /// and waits until it runs.
    pub async fn open_named(&self, name: &str) -> WebDriverResult<()> {
        self.page.open_at(name).await
    }

    /// Reloads the page, and waits until it runs.
    pub async fn reload(&self) -> WebDriverResult<()> {
        self.page.reload().await
    }

    /// Leaves the page by reloading it, and returns whether the browser asked first. Asked, it
    /// stays on the page.
    ///
    /// # Panics
    /// Panics if the page is neither left nor asked about in time.
    pub async fn leave(&self) -> WebDriverResult<bool> {
        self.page.leave().await
    }

    pub async fn input(&self) -> WebDriverResult<WebElement> {
        self.driver.query(By::Id("input")).first().await
    }

    /// Types `line` into the input and presses Enter.
    pub async fn run(&self, line: &str) -> WebDriverResult<()> {
        let input = self.input().await?;
        input.send_keys(line).await?;
        // shows the line typed, which running it removes
        Page::pause().await;
        input.send_keys(Key::Enter).await?;
        // the page empties the input when it runs the line
        self.expect_input("").await?;
        Page::pause().await;
        Ok(())
    }

    /// Presses `key` in the input.
    pub async fn press(&self, key: Key) -> WebDriverResult<()> {
        self.input().await?.send_keys(key).await?;
        Page::pause().await;
        Ok(())
    }

    /// Waits until what the last line run showed (its last reply) is `text`.
    ///
    /// # Panics
    /// Panics if it does not show `text` in time, with what it shows instead.
    pub async fn expect_reply<N>(&self, text: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        self.page.expect_text(By::XPath(LAST_REPLY), text).await
    }

    /// Waits until the last line run failed with `text`.
    ///
    /// # Panics
    /// Panics if it does not show the error `text` in time, with what it shows instead.
    pub async fn expect_error<N>(&self, text: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        let expected = format!("the text {text:?} and class \"error\"");
        self.page
            .expect(
                By::XPath(LAST_REPLY),
                |query| query.with_text(text).with_class("error"),
                &expected,
            )
            .await
    }

    /// Waits until the prompt is `text`, e.g. `"cup*> "`.
    ///
    /// # Panics
    /// Panics if the prompt is not `text` in time, with what it is instead.
    pub async fn expect_prompt<N>(&self, text: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        self.page.expect_text(By::Id("prompt"), text).await
    }

    /// Waits until the input holds `value`.
    ///
    /// # Panics
    /// Panics if it does not hold `value` in time, with what it holds instead.
    pub async fn expect_input<N>(&self, value: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        let expected = format!("the value {value:?}");
        self.page
            .expect(By::Id("input"), |query| query.with_value(value), &expected)
            .await
    }

    /// Waits until the output shows `text` somewhere.
    ///
    /// # Panics
    /// Panics if it does not in time, with what it shows instead.
    pub async fn expect_output<N>(&self, text: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        self.page.expect_text(By::Id("output"), text).await
    }

    /// Answers every question with the answer it offers, i.e. the one in brackets in the prompt,
    /// until no question offers one.
    pub async fn accept_every_question(&self) -> WebDriverResult<()> {
        for _ in 0..100 {
            let prompt = self.driver.find(By::Id("prompt")).await?.text().await?;
            if !prompt.ends_with("]> ") {
                return Ok(());
            }
            self.run("").await?;
        }
        panic!("the questions must end");
    }
}
