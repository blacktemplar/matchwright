//! End-to-end tests of a page built into a directory: [`run`] serves the [`Site`] and opens it in
//! headless Chrome, and the test uses it through a [`Page`], which a crate testing a particular
//! page wraps in what an operator does there.
//!
//! Chrome is driven by the chromedriver matching it, which is downloaded and cached in the user's
//! cache directory, unless the environment variable `E2E_CHROMEDRIVER` names one to use instead.
//!
//! A test fails if the page logs an error, or if it is not accessible at its end, as
//! [`Page::check_accessibility`] finds out. A failing test saves a screenshot of the page and a
//! report on it, see [`failures`].
//!
//! With the environment variable `E2E_HEADED` set to anything but `0`, Chrome opens a window, which
//! shows what a test does; `E2E_LINGER` set to a number of seconds then keeps it open that long
//! after the test, whether it passed or failed, and `E2E_PAUSE` pauses that long after each action
//! of a [`Page`].

use std::{
    fmt::Debug,
    fs, io,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use axum::Router;
use futures_util::{FutureExt, StreamExt};
use sha2::{Digest, Sha256};
pub use thirtyfour;
use thirtyfour::{
    BrowserLogEntry, By, CapabilitiesHelper, ChromiumLikeCapabilities, DesiredCapabilities,
    LoggingPrefsLogLevel, WebDriver,
    bidi::events::UserPromptOpened,
    error::{WebDriverError, WebDriverResult},
    extensions::query::{ElementPollerWithTimeout, ElementQuery, ElementQueryable},
    manager::BrowserKind,
    stringmatch::Needle,
    testing::{
        Artifact, FailureArtifactCollector, FailureArtifactConfig, FailureArtifacts,
        run_browser_test,
    },
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

/// How long a query waits for what it looks for, and how often it looks.
const TIMEOUT: Duration = Duration::from_secs(5);
const INTERVAL: Duration = Duration::from_millis(20);

/// The version of axe-core that checks accessibility, where it is downloaded from, and the SHA-256
/// of what is downloaded.
const AXE_VERSION: &str = "4.14.0";
const AXE_URL: &str = "https://cdn.jsdelivr.net/npm/axe-core@4.14.0/axe.min.js";
const AXE_SHA256: &str = "20c09fe157a8a34a30e241aaa1fcdade657734f08ab379ecfbeb7d45cc46e878";

/// The rules of axe-core that are checked: those of WCAG 2.1, levels A and AA.
const AXE_TAGS: &str = "['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']";

/// A page to be tested.
pub struct Site {
    /// The directory the page is built into, with its `index.html`.
    pub dist: PathBuf,
    /// The command that builds the page, which a test without it tells to run.
    pub build: &'static str,
    /// An element the page shows with some text once it runs.
    pub ready: By,
}

/// Runs `test` on the page of `site` freshly opened in a new Chrome, headless unless `E2E_HEADED`
/// says otherwise, with nothing saved yet. The chromedriver is the one `E2E_CHROMEDRIVER` names,
/// if any, see the [crate docs](crate).
///
/// A failing test leaves a screenshot of the page and a report in [`failures`].
///
/// # Panics
/// Panics if the page is not built, if `test` fails, if the page logs an error, such as a panic of
/// the WebAssembly, or throws one, or if it is not accessible when `test` has passed, see
/// [`Page::check_accessibility`].
pub async fn run(site: Site, test: impl AsyncFnOnce(Page) -> WebDriverResult<()>) {
    let url = serve(&site).await;
    let mut capabilities = DesiredCapabilities::chrome();
    if !headed() {
        capabilities.set_headless().unwrap();
    }
    capabilities
        .set_logging_prefs("browser", LoggingPrefsLogLevel::Severe)
        .unwrap();
    // reports the browser asking whether to leave the page (see `Page::leave`), and leaves the
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
    let failures = failures();
    // what is saved there is always of the last run
    if let Err(error) = fs::remove_dir_all(&failures)
        && error.kind() != io::ErrorKind::NotFound
    {
        panic!("cannot remove {}: {error}", failures.display());
    }
    let result = run_browser_test(session, |driver| async move {
        // reading the browser log empties it, so it is read once below, for the check of errors and
        // for the report alike
        let mut config = FailureArtifactConfig::default();
        config.browser_logs = None;
        let collector = FailureArtifactCollector::with_config(&driver, config);
        let page = Page {
            driver,
            url,
            ready: site.ready,
        };
        // a failing expectation panics, after which the page is worth seeing all the more
        let tested = AssertUnwindSafe(async {
            page.open().await?;
            test(page.clone()).await?;
            page.check_accessibility().await
        })
        .catch_unwind()
        .await;
        let log = page.driver.browser_log().await;
        let failed = !matches!(tested, Ok(Ok(())))
            || log
                .as_ref()
                .map_or(true, |log| !page_errors(log).is_empty());
        if failed {
            save(&collector.capture().await, &log, &failures);
        }
        if let Some(linger) = linger() {
            tokio::time::sleep(linger).await;
        }
        match tested {
            Ok(tested) => tested?,
            Err(panic) => panic::resume_unwind(panic),
        }
        let log = log?;
        let errors = page_errors(&log);
        assert!(
            errors.is_empty(),
            "the page must report no errors, got {errors:#?}"
        );
        Ok::<_, WebDriverError>(())
    })
    .await;
    if let Err(error) = result {
        panic!("{error}");
    }
}

/// Where [`run`] saves what the page showed when the current test failed: the directory
/// `e2e-failures/<test file>/<test>` in the target directory, with `screenshot.png`, and
/// `report.txt` with the page's URL, source and logs.
pub fn failures() -> PathBuf {
    let target = option_env!("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../target"),
        PathBuf::from,
    );
    // the binary of an integration test is named after its file, followed by a hash
    let binary = std::env::args()
        .next()
        .map(PathBuf::from)
        .and_then(|binary| Some(binary.file_name()?.to_str()?.to_owned()))
        .unwrap_or_default();
    let file = binary.rsplit_once('-').map_or(&*binary, |(file, _)| file);
    // the test harness names the thread a test runs on after the test
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .replace("::", "-");
    target.join("e2e-failures").join(file).join(test)
}

/// The errors the page logged or threw, in the browser `log`.
fn page_errors(log: &[BrowserLogEntry]) -> Vec<&str> {
    log.iter()
        // a failed request, such as the favicon the page has none of, is no error of the page
        .filter(|entry| entry.source.as_deref() != Some("network"))
        .map(|entry| entry.message.as_str())
        .collect()
}

/// Saves `artifacts` and the browser `log` into `dir`, see [`failures`]. A failure to save is only
/// reported, so that it does not hide the failure of the test.
fn save(artifacts: &FailureArtifacts, log: &WebDriverResult<Vec<BrowserLogEntry>>, dir: &Path) {
    let mut report = artifacts.to_string();
    match log {
        Ok(log) => {
            report += &format!("browser log: {} entries\n", log.len());
            for entry in log {
                report += &format!("  [{}] {}\n", entry.level, entry.message);
            }
        }
        Err(error) => report += &format!("browser log: unavailable: {error}\n"),
    }
    let saved = fs::create_dir_all(dir)
        .and_then(|()| match &artifacts.screenshot {
            Artifact::Captured(png) => fs::write(dir.join("screenshot.png"), png),
            _ => Ok(()),
        })
        .and_then(|()| fs::write(dir.join("report.txt"), report));
    match saved {
        Ok(()) => eprintln!("what the page showed is saved in {}", dir.display()),
        Err(error) => eprintln!(
            "cannot save what the page showed in {}: {error}",
            dir.display()
        ),
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

/// Serves the built page of `site` on a port of its own, so that each test has its own storage,
/// and returns its URL. The server runs as long as the test's runtime.
async fn serve(site: &Site) -> String {
    assert!(
        site.dist.join("index.html").is_file(),
        "the page must be built first: `{}`",
        site.build
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback_service(ServeDir::new(&site.dist));
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{address}/")
}

/// The source of axe-core, downloaded once and cached in the user's cache directory.
///
/// # Panics
/// Panics if it is neither cached nor can be downloaded, or what is downloaded is not what was
/// expected.
async fn axe_source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    if let Some(source) = SOURCE.get() {
        return source;
    }
    let cache = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("tournament-e2e")
        .join(format!("axe-core-{AXE_VERSION}.min.js"));
    let source = match tokio::fs::read_to_string(&cache).await {
        Ok(source) if sha256(&source) == AXE_SHA256 => source,
        _ => {
            let source = reqwest::get(AXE_URL)
                .await
                .and_then(reqwest::Response::error_for_status)
                .unwrap_or_else(|error| panic!("cannot download axe-core from {AXE_URL}: {error}"))
                .text()
                .await
                .unwrap_or_else(|error| panic!("cannot download axe-core from {AXE_URL}: {error}"));
            assert_eq!(
                sha256(&source),
                AXE_SHA256,
                "axe-core downloaded from {AXE_URL} must be the one expected"
            );
            // written to a file of its own first, since tests running at the same time may write
            // it too
            let partial = cache.with_extension(format!("{}.partial", std::process::id()));
            let cached = async {
                tokio::fs::create_dir_all(cache.parent().expect("a file has a parent")).await?;
                tokio::fs::write(&partial, &source).await?;
                tokio::fs::rename(&partial, &cache).await
            }
            .await;
            if let Err(error) = cached {
                eprintln!("cannot cache axe-core in {}: {error}", cache.display());
            }
            source
        }
    };
    SOURCE.get_or_init(|| source)
}

fn sha256(text: &str) -> String {
    Sha256::digest(text)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The page under test, as a WebDriver session.
///
/// After each of its actions (opening, reloading or leaving the page), a test pauses as long as
/// `E2E_PAUSE` says, see the [crate docs](crate); so should every action of a type wrapping it,
/// with [`Page::pause`].
#[derive(Clone)]
pub struct Page {
    pub driver: WebDriver,
    /// Where the page is served.
    url: String,
    /// See [`Site::ready`].
    ready: By,
}

impl Page {
    /// Opens the page, and waits until it runs.
    pub async fn open(&self) -> WebDriverResult<()> {
        self.driver.goto(&self.url).await?;
        self.ready().await?;
        Self::pause().await;
        Ok(())
    }

    /// Opens the page with `fragment` after the `#` of its URL, and waits until it runs.
    pub async fn open_at(&self, fragment: &str) -> WebDriverResult<()> {
        self.driver.goto(format!("{}#{fragment}", self.url)).await?;
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
    pub async fn pause() {
        if let Some(pause) = pause() {
            tokio::time::sleep(pause).await;
        }
    }

    /// Waits until the page runs, which shows some text in [`Site::ready`].
    async fn ready(&self) -> WebDriverResult<()> {
        self.driver
            .query(self.ready.clone())
            .without_text("")
            .first()
            .await?;
        Ok(())
    }

    /// Checks the page as it is now with axe-core, against the rules of WCAG 2.1 at levels A and
    /// AA. [`run`] checks every page at the end of its test.
    ///
    /// # Panics
    /// Panics with what violates them, if anything does, or if axe-core cannot be downloaded, see
    /// the [crate docs](crate).
    pub async fn check_accessibility(&self) -> WebDriverResult<()> {
        let source = axe_source().await;
        self.driver
            .execute(
                format!("if (typeof axe === 'undefined') {{ {source} }}"),
                Vec::new(),
            )
            .await?;
        let violations = self
            .driver
            .execute(
                format!(
                    "return axe.run(document, {{ runOnly: {{ type: 'tag', values: {AXE_TAGS} }} }})
                        .then(results => results.violations.map(violation => ({{
                            rule: violation.id,
                            help: violation.help,
                            elements: violation.nodes.map(node => node.target.join(' ')),
                        }})))"
                ),
                Vec::new(),
            )
            .await?
            .json()
            .clone();
        let violations = violations
            .as_array()
            .expect("axe-core returns a list of violations");
        assert!(
            violations.is_empty(),
            "the page must be accessible, but violates {violations:#?}"
        );
        Ok(())
    }

    /// Waits until an element `by` finds has `text`.
    ///
    /// # Panics
    /// Panics if none does in time, with what the first one has instead.
    pub async fn expect_text<N>(&self, by: By, text: N) -> WebDriverResult<()>
    where
        N: Needle + Debug + Clone + Send + Sync + 'static,
    {
        let expected = format!("the text {text:?}");
        self.expect(by, |query| query.with_text(text), &expected)
            .await
    }

    /// Waits until the elements `by` finds have `texts`, in this order.
    ///
    /// # Panics
    /// Panics if they do not in time, with the texts they have instead.
    pub async fn expect_texts(&self, by: By, texts: &[&str]) -> WebDriverResult<()> {
        let start = Instant::now();
        loop {
            let mut found = Vec::new();
            for element in self.driver.find_all(by.clone()).await? {
                found.push(element.text().await?);
            }
            if found == texts {
                return Ok(());
            }
            if start.elapsed() > TIMEOUT {
                panic!("{by} must find the texts {texts:?}, but finds {found:?}");
            }
            tokio::time::sleep(INTERVAL).await;
        }
    }

    /// Waits until the query for `by` that `refine` narrows down finds an element, which is what is
    /// `expected`.
    ///
    /// # Panics
    /// Panics if it does not in time, with what the first element `by` finds has instead.
    pub async fn expect(
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
}
