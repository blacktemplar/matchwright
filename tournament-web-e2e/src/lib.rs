//! End-to-end tests of `tournament-web`: [`run`] opens the page in headless Chrome, and the test
//! uses it through a [`Terminal`], the way an operator does, by typing into its input.
//!
//! The page is served from `tournament-web/dist`, which `trunk build` has to have built before.
//! Chrome is driven by the chromedriver matching it, which is downloaded and cached in the user's
//! cache directory, unless the environment variable `E2E_CHROMEDRIVER` names one to use instead.
//!
//! With the environment variable `E2E_HEADED` set to anything but `0`, Chrome opens a window, which
//! shows what a test does; `E2E_LINGER` set to a number of seconds then keeps it open that long
//! after the test, whether it passed or failed, and `E2E_PAUSE` pauses that long after each action
//! of a [`Terminal`].

use std::{
    fmt::Debug,
    panic::{self, AssertUnwindSafe},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::Router;
use futures_util::{FutureExt, StreamExt};
pub use thirtyfour;
use thirtyfour::{
    By, CapabilitiesHelper, ChromiumLikeCapabilities, DesiredCapabilities, Key,
    LoggingPrefsLogLevel, WebDriver, WebElement,
    bidi::events::UserPromptOpened,
    error::WebDriverResult,
    extensions::query::{ElementPollerWithTimeout, ElementQuery, ElementQueryable},
    manager::BrowserKind,
    stringmatch::Needle,
    testing::run_browser_test,
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

/// How long a query waits for what it looks for, and how often it looks.
const TIMEOUT: Duration = Duration::from_secs(5);
const INTERVAL: Duration = Duration::from_millis(20);

/// What a line run shows that is not the line itself, the last one.
const LAST_REPLY: &str = "(//div[@id='output']/div[not(contains(@class, 'line'))])[last()]";

/// Runs `test` on the page freshly opened in a new Chrome, headless unless `E2E_HEADED` says
/// otherwise, with nothing saved yet. The chromedriver is the one `E2E_CHROMEDRIVER` names, if
/// any, see the [crate docs](crate).
///
/// # Panics
/// Panics if `test` fails, or if the page logs an error, such as a panic of the WebAssembly, or
/// throws one.
pub async fn run(test: impl AsyncFnOnce(Terminal) -> WebDriverResult<()>) {
    let url = serve().await;
    let mut capabilities = DesiredCapabilities::chrome();
    if !headed() {
        capabilities.set_headless().unwrap();
    }
    capabilities
        .set_logging_prefs("browser", LoggingPrefsLogLevel::Severe)
        .unwrap();
    // reports the browser asking whether to leave the page (see `Terminal::leave`), and leaves the
    // question open rather than accepting it
    capabilities.enable_bidi().unwrap();
    capabilities
        .set(
            "unhandledPromptBehavior",
            serde_json::json!({ "beforeUnload": "ignore" }),
        )
        .unwrap();
    let mut session = WebDriver::managed(capabilities)
        .poller(Arc::new(ElementPollerWithTimeout::new(TIMEOUT, INTERVAL)));
    if let Some(chromedriver) = std::env::var_os("E2E_CHROMEDRIVER") {
        session = session.driver_binary(BrowserKind::Chrome, chromedriver);
    }
    let result = run_browser_test(session, |driver| async move {
        let terminal = Terminal { driver, url };
        terminal.open().await?;
        // a failing expectation panics, after which the window is worth seeing all the more
        let tested = AssertUnwindSafe(test(terminal.clone()))
            .catch_unwind()
            .await;
        if let Some(linger) = linger() {
            tokio::time::sleep(linger).await;
        }
        match tested {
            Ok(tested) => tested?,
            Err(panic) => panic::resume_unwind(panic),
        }
        terminal.assert_no_errors().await
    })
    .await;
    if let Err(error) = result {
        panic!("{error}");
    }
}

/// Whether Chrome is to open a window, as the environment variable `E2E_HEADED` says.
fn headed() -> bool {
    std::env::var_os("E2E_HEADED").is_some_and(|headed| !headed.is_empty() && headed != "0")
}

/// How long the window stays open after a test, as the environment variable `E2E_LINGER` says.
///
/// # Panics
/// Panics if `E2E_LINGER` is no number of seconds.
fn linger() -> Option<Duration> {
    watching_for("E2E_LINGER")
}

/// How long a test pauses after each action, as the environment variable `E2E_PAUSE` says.
///
/// # Panics
/// Panics if `E2E_PAUSE` is no number of seconds.
fn pause() -> Option<Duration> {
    watching_for("E2E_PAUSE")
}

/// The number of seconds the environment variable `name` says, which only a window opened by
/// `E2E_HEADED` is watched for.
///
/// # Panics
/// Panics if `name` is no number of seconds.
fn watching_for(name: &str) -> Option<Duration> {
    if !headed() {
        return None;
    }
    let seconds = std::env::var(name).ok()?;
    let seconds = seconds
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be a number of seconds, not {seconds:?}"));
    Some(Duration::from_secs_f64(seconds))
}

/// Serves the built page on a port of its own, so that each test has its own local storage, and
/// returns its URL. The server runs as long as the test's runtime.
async fn serve() -> String {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tournament-web/dist");
    assert!(
        dist.join("index.html").is_file(),
        "the page must be built first: `cd tournament-web && trunk build`"
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback_service(ServeDir::new(dist));
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{address}/")
}

/// The command line of the page.
///
/// After each of its actions (opening, reloading or leaving the page, running a line, pressing a
/// key), a test pauses as long as `E2E_PAUSE` says, see the [crate docs](crate).
#[derive(Clone)]
pub struct Terminal {
    pub driver: WebDriver,
    /// Where the page is served.
    url: String,
}

impl Terminal {
    /// Opens the page, and waits until it runs.
    pub async fn open(&self) -> WebDriverResult<()> {
        self.driver.goto(&self.url).await?;
        self.ready().await?;
        Self::pause().await;
        Ok(())
    }

    /// Opens the page with `name` after the `#` of its URL, i.e. the competition saved under it,
    /// and waits until it runs.
    pub async fn open_named(&self, name: &str) -> WebDriverResult<()> {
        self.driver.goto(format!("{}#{name}", self.url)).await?;
        // going to a URL that differs only after the `#` stays on the page, which reads it only on
        // load
        self.reload().await
    }

    /// Reloads the page, and waits until it runs.
    pub async fn reload(&self) -> WebDriverResult<()> {
        self.driver.refresh().await?;
        self.ready().await?;
        Self::pause().await;
        Ok(())
    }

    /// Leaves the page by reloading it, and returns whether the browser asked first. Asked, it
    /// stays on the page.
    ///
    /// # Panics
    /// Panics if the page is neither left nor asked about in time.
    pub async fn leave(&self) -> WebDriverResult<bool> {
        let bidi = self.driver.bidi().await?;
        let mut asked = bidi.subscribe::<UserPromptOpened>().await?;
        // marks the page, which the reloaded one is not; reloading from a timer returns before the
        // browser asks
        self.driver
            .execute(
                "window.leaving = true; setTimeout(() => location.reload())",
                Vec::new(),
            )
            .await?;
        let start = Instant::now();
        while start.elapsed() < TIMEOUT {
            if let Ok(Some(prompt)) = tokio::time::timeout(INTERVAL, asked.next()).await
                && prompt.prompt_type == "beforeunload"
            {
                // shows the question, then stays
                Self::pause().await;
                bidi.browsing_context()
                    .handle_user_prompt(prompt.context, Some(false), None)
                    .await?;
                Self::pause().await;
                return Ok(true);
            }
            // fails while the browser asks, which the next round finds out
            if let Ok(left) = self
                .driver
                .execute("return window.leaving === undefined", Vec::new())
                .await
                && left.json().as_bool() == Some(true)
            {
                self.ready().await?;
                Self::pause().await;
                return Ok(false);
            }
        }
        panic!("the page must be left, or the browser ask first");
    }

    /// Pauses as long as `E2E_PAUSE` says, after an action.
    async fn pause() {
        if let Some(pause) = pause() {
            tokio::time::sleep(pause).await;
        }
    }

    /// Waits until the WebAssembly runs, which shows the prompt.
    async fn ready(&self) -> WebDriverResult<()> {
        self.driver
            .query(By::Id("prompt"))
            .without_text("")
            .first()
            .await?;
        Ok(())
    }

    pub async fn input(&self) -> WebDriverResult<WebElement> {
        self.driver.query(By::Id("input")).first().await
    }

    /// Types `line` into the input and presses Enter.
    pub async fn run(&self, line: &str) -> WebDriverResult<()> {
        let input = self.input().await?;
        input.send_keys(line).await?;
        // shows the line typed, which running it removes
        Self::pause().await;
        input.send_keys(Key::Enter).await?;
        // the page empties the input when it runs the line
        self.expect_input("").await?;
        Self::pause().await;
        Ok(())
    }

    /// Presses `key` in the input.
    pub async fn press(&self, key: Key) -> WebDriverResult<()> {
        self.input().await?.send_keys(key).await?;
        Self::pause().await;
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
        self.expect_text(By::XPath(LAST_REPLY), text).await
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
        self.expect(
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
        self.expect_text(By::Id("prompt"), text).await
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
        self.expect(By::Id("input"), |query| query.with_value(value), &expected)
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
        self.expect_text(By::Id("output"), text).await
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

    async fn expect_text<N>(&self, by: By, text: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        let expected = format!("the text {text:?}");
        self.expect(by, |query| query.with_text(text), &expected)
            .await
    }

    /// Waits until the query for `by` that `refine` narrows down finds an element, which is what is
    /// `expected`, and panics with what `by` finds instead if it does not in time.
    async fn expect(
        &self,
        by: By,
        refine: impl FnOnce(ElementQuery) -> ElementQuery,
        expected: &str,
    ) -> WebDriverResult<()> {
        if refine(self.driver.query(by.clone())).first().await.is_ok() {
            return Ok(());
        }
        let found = match self.driver.find(by.clone()).await {
            Ok(element) => format!(
                "the text {:?}, class {:?} and value {:?}",
                element.text().await?,
                element.class_name().await?,
                element.prop("value").await?,
            ),
            Err(_) => "nothing".to_owned(),
        };
        panic!("{by} must have {expected}, but has {found}");
    }

    /// Fails if the page logged an error or threw one since the last call.
    async fn assert_no_errors(&self) -> WebDriverResult<()> {
        let errors: Vec<_> = self
            .driver
            .browser_log()
            .await?
            .into_iter()
            // a failed request, such as the favicon the page has none of, is no error of the page
            .filter(|entry| entry.source.as_deref() != Some("network"))
            .map(|entry| entry.message)
            .collect();
        assert!(
            errors.is_empty(),
            "the page must report no errors, got {errors:#?}"
        );
        Ok(())
    }
}
