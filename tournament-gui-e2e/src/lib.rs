//! End-to-end tests of `tournament-gui`: [`run`] opens the page in headless Chrome, and the test
//! uses it through a [`Gui`], the way an operator does, by typing and clicking. A [`Gui`] compares
//! texts without the [`ISOLATION_MARKS`] the translation puts around names and numbers.
//!
//! The page is served from where `dx build -p tournament-gui --release` builds it. Everything else,
//! from the chromedriver to what a failing test leaves and the environment variables that show a
//! test in a window, is that of [`e2e_harness`].

use std::path::Path;

use e2e_harness::{
    ISOLATION_MARKS, Page, Site,
    thirtyfour::{
        By, Key, WebDriver, components::SelectElement, error::WebDriverResult,
        extensions::query::ElementQueryable,
    },
};
pub use e2e_harness::{failures, thirtyfour};

/// Runs `test` on the page freshly opened, see [`e2e_harness::run`].
///
/// # Panics
/// Panics if `test` fails, if the page logs an error, or if it is not accessible at the end.
pub async fn run(test: impl AsyncFnOnce(Gui) -> WebDriverResult<()>) {
    let site = Site {
        // where dx builds the page; the debug build would try to reach `dx serve`
        dist: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/dx/tournament-gui/release/web/public"),
        build: "dx build -p tournament-gui --release",
        // rendered by the WebAssembly
        ready: By::Css("h1"),
    };
    e2e_harness::run(site, async |page| {
        test(Gui {
            driver: page.driver.clone(),
            page,
        })
        .await
    })
    .await;
}

/// The screen of the page.
///
/// After each of its actions (typing, clicking, choosing), a test pauses as long as `E2E_PAUSE`
/// says.
#[derive(Clone)]
pub struct Gui {
    pub driver: WebDriver,
    pub page: Page,
}

impl Gui {
    /// Adds the entrant `name`, and waits until it is listed.
    pub async fn add_entrant(&self, name: &str) -> WebDriverResult<()> {
        let input = self.driver.query(By::Css("form input")).first().await?;
        input.send_keys(name).await?;
        Page::pause().await;
        input.send_keys(Key::Enter).await?;
        self.page
            .expect_text(By::Css("#entrant-list .name"), name.to_owned())
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Chooses the format labelled `label` for the phase.
    pub async fn choose_format(&self, label: &str) -> WebDriverResult<()> {
        self.choose("format", label).await
    }

    /// Chooses the language named `name` in itself, e.g. `"Deutsch"`.
    pub async fn choose_language(&self, name: &str) -> WebDriverResult<()> {
        self.choose("language", name).await
    }

    /// Waits until the language of the page, its `<html lang>`, is `tag`.
    ///
    /// # Panics
    /// Panics if it is not in time.
    pub async fn expect_page_language(&self, tag: &str) -> WebDriverResult<()> {
        self.page
            .expect(
                By::Css("html"),
                |query| query.with_attribute("lang", tag.to_owned()),
                &format!("the language {tag:?}"),
            )
            .await
    }

    /// Clicks the button whose text is `text`, once it is enabled.
    pub async fn click(&self, text: &str) -> WebDriverResult<()> {
        self.button(text)
            .and_enabled()
            .first()
            .await?
            .click()
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Clicks the button whose accessible name, its `aria-label`, is `label`, once it is enabled.
    pub async fn click_labelled(&self, label: &str) -> WebDriverResult<()> {
        self.driver
            .query(By::XPath(format!(
                "//button[{}='{label}']",
                without_isolation_marks("@aria-label")
            )))
            .and_enabled()
            .first()
            .await?
            .click()
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Waits until the button whose text is `text` is disabled.
    ///
    /// # Panics
    /// Panics if it is not in time.
    pub async fn expect_disabled(&self, text: &str) -> WebDriverResult<()> {
        self.button(text).and_not_enabled().first().await?;
        Ok(())
    }

    /// Waits until the screen shows why the last action was refused, `text`.
    ///
    /// # Panics
    /// Panics if it does not in time, with what it shows instead.
    pub async fn expect_error(&self, text: &str) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("[role=alert]"), &[text])
            .await
    }

    /// Waits until the entrants listed are `names`, in this order.
    ///
    /// # Panics
    /// Panics if they are not in time, with those listed instead.
    pub async fn expect_entrants(&self, names: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("#entrant-list .name"), names)
            .await
    }

    /// Waits until the matches of the round shown are those numbered `numbers`, in this order.
    ///
    /// # Panics
    /// Panics if they are not in time, with the numbers shown instead.
    pub async fn expect_matches(&self, numbers: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("#match-list tbody td:first-child"), numbers)
            .await
    }

    /// Waits until the results of the matches of the round shown that have one are `results`, in
    /// the order of the matches, e.g. `"Alice won"`.
    ///
    /// # Panics
    /// Panics if they are not in time, with the results shown instead.
    pub async fn expect_results(&self, results: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("#match-list .result"), results)
            .await
    }

    /// Waits until the standings name `names`, best first.
    ///
    /// # Panics
    /// Panics if they do not in time, with those they name instead.
    pub async fn expect_standings(&self, names: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("#standing-list tbody td:nth-child(2)"), names)
            .await
    }

    /// Chooses the option with the text `text` of the select whose id is `id`.
    async fn choose(&self, id: &str, text: &str) -> WebDriverResult<()> {
        let select = self.driver.query(By::Id(id)).first().await?;
        SelectElement::new(&select)
            .await?
            .select_by_visible_text(text)
            .await?;
        Page::pause().await;
        Ok(())
    }

    fn button(&self, text: &str) -> e2e_harness::thirtyfour::extensions::query::ElementQuery {
        self.driver.query(By::XPath(format!(
            "//button[{}='{text}']",
            without_isolation_marks("normalize-space()")
        )))
    }
}

/// The XPath of the string `expression` without the [`ISOLATION_MARKS`] in it.
fn without_isolation_marks(expression: &str) -> String {
    let marks: String = ISOLATION_MARKS.iter().collect();
    format!("translate({expression}, '{marks}', '')")
}
