//! End-to-end tests of `tournament-gui`: [`run`] opens the page in headless Chrome, and the test
//! uses it through a [`Gui`], the way an operator does, by typing and clicking; a test about a
//! single competition starts in a new one with [`run_in_new_competition`]. A [`Gui`] compares
//! texts without the [`ISOLATION_MARKS`] the translation puts around names and numbers.
//!
//! The page is served from where `dx build -p tournament-gui --release --debug-symbols false`
//! builds it, as it is hosted. Everything else,
//! from the chromedriver to what a failing test leaves and the environment variables that show a
//! test in a window, is that of [`e2e_harness`].

use std::path::Path;

use e2e_harness::{
    ISOLATION_MARKS, Page, Site,
    thirtyfour::{
        By, WebDriver, components::SelectElement, error::WebDriverResult,
        extensions::query::ElementQueryable,
    },
};
pub use e2e_harness::{failures, thirtyfour, thirtyfour::Key};

/// The name of the competition [`run_in_new_competition`] creates.
pub const COMPETITION: &str = "Cup";

/// Runs `test` on a new competition named [`COMPETITION`], open on the page freshly opened, see
/// [`run`].
///
/// # Panics
/// Panics as [`run`] does.
pub async fn run_in_new_competition(test: impl AsyncFnOnce(Gui) -> WebDriverResult<()>) {
    run(async move |gui| {
        gui.new_competition(COMPETITION).await?;
        test(gui).await
    })
    .await;
}

/// Runs `test` on the page freshly opened, which lists the competitions stored, none yet, see
/// [`e2e_harness::run`].
///
/// # Panics
/// Panics if `test` fails, if the page logs an error, or if it is not accessible at the end.
pub async fn run(test: impl AsyncFnOnce(Gui) -> WebDriverResult<()>) {
    let site = Site {
        // where dx builds the page; the debug build would try to reach `dx serve`
        dist: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/dx/tournament-gui/release/web/public"),
        build: "dx build -p tournament-gui --release --debug-symbols false",
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
    /// Creates the competition `name` from the list of competitions, and waits until it is open.
    pub async fn new_competition(&self, name: &str) -> WebDriverResult<()> {
        let input = self.driver.query(By::Css("form input")).first().await?;
        input.send_keys(name).await?;
        Page::pause().await;
        self.click("Create").await?;
        self.expect_open(name).await
    }

    /// Opens the competition `name` from the list of competitions, and waits until it is open.
    pub async fn open_competition(&self, name: &str) -> WebDriverResult<()> {
        self.follow(name).await?;
        self.expect_open(name).await
    }

    /// Imports the competition of the file at `path` from the list of competitions, which opens it.
    pub async fn import(&self, path: &Path) -> WebDriverResult<()> {
        let input = self.driver.query(By::Id("import")).first().await?;
        input
            .send_keys(path.to_str().expect("the path is a text"))
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Follows the link whose text is `text`, e.g. `"All competitions"`.
    pub async fn follow(&self, text: &str) -> WebDriverResult<()> {
        self.driver
            .query(By::LinkText(text))
            .first()
            .await?
            .click()
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Waits until the competitions listed are `names`, in this order.
    ///
    /// # Panics
    /// Panics if they are not in time, with those listed instead.
    pub async fn expect_competitions(&self, names: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("#competition-list a"), names)
            .await
    }

    /// Waits until the competition `name` is open.
    ///
    /// # Panics
    /// Panics if it is not in time.
    pub async fn expect_open(&self, name: &str) -> WebDriverResult<()> {
        self.page.expect_texts(By::Css("h1"), &[name]).await?;
        // the screen of the competition, before or after the start, rather than that of opening it
        self.driver
            .query(By::Css("#entrants, #matches"))
            .first()
            .await?;
        Ok(())
    }

    /// Waits until the competition open is shown only, since another tab changes it, if
    /// `read_only`, or else until it can be changed here.
    ///
    /// # Panics
    /// Panics if it is not in time.
    pub async fn expect_read_only(&self, read_only: bool) -> WebDriverResult<()> {
        let notices: &[&str] = if read_only {
            &[
                "This competition is open in another tab, so it is only shown here. It follows \
               what is done there, and can be changed here once that tab leaves it.",
            ]
        } else {
            &[]
        };
        self.page.expect_texts(By::Css(".notice"), notices).await
    }

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

    /// Chooses the format labelled `label` in the form of the phase shown, which may be that of a
    /// dialog.
    pub async fn choose_format(&self, label: &str) -> WebDriverResult<()> {
        self.choose_plugin("format", label).await
    }

    /// Chooses the plugin labelled `label` for the field at `path` of the form of the phase, e.g.
    /// `"format"`, or adds it to the list at `path` if it is one whose order matters, e.g.
    /// `"ranking"`.
    pub async fn choose_plugin(&self, path: &str, label: &str) -> WebDriverResult<()> {
        let id = field_id(path);
        let trigger = match self.driver.find(By::Id(format!("{id}-add"))).await {
            Ok(add) => add,
            Err(_) => self.driver.query(By::Id(id)).first().await?,
        };
        trigger.click().await?;
        self.driver
            .query(By::XPath(format!(
                "//*[@role='option'][.//*[@class='option-label'][normalize-space()='{label}']]"
            )))
            .and_displayed()
            .first()
            .await?
            .click()
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Enters `text` into the number field at `path` of the form of the phase, e.g.
    /// `"format.rounds"`, and leaves the field, which takes it.
    pub async fn enter(&self, path: &str, text: &str) -> WebDriverResult<()> {
        let input = self.driver.query(By::Id(field_id(path))).first().await?;
        input.clear().await?;
        input.send_keys(text).await?;
        Page::pause().await;
        input.send_keys(Key::Tab).await?;
        Page::pause().await;
        Ok(())
    }

    /// Checks or unchecks the checkbox labelled `label`, e.g. a plugin of the metrics.
    pub async fn toggle(&self, label: &str) -> WebDriverResult<()> {
        self.driver
            .query(By::XPath(format!(
                "//label[{}='{label}']/input[@type='checkbox']",
                without_isolation_marks("normalize-space()")
            )))
            .and_enabled()
            .first()
            .await?
            .click()
            .await?;
        Page::pause().await;
        Ok(())
    }

    /// Waits until what is wrong with the answers of the form of the phase is `issues`, those of
    /// every field in the order of the fields.
    ///
    /// # Panics
    /// Panics if it is not in time, with what is shown instead.
    pub async fn expect_issues(&self, issues: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css(".phase-form .issues li"), issues)
            .await
    }

    /// Waits until the plugins of the list at `path` of the form of the phase whose order matters
    /// are `labels`, in this order, e.g. `"ranking"`.
    ///
    /// # Panics
    /// Panics if they are not in time, with those shown instead.
    pub async fn expect_list(&self, path: &str, labels: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css(format!("#{} > li > .name", field_id(path))), labels)
            .await
    }

    /// Waits until the plugins checked in the set of plugins at `path` of the form of the phase,
    /// e.g. `"metrics"`, are `labels`, in the order shown, those it implies included. What is
    /// checked is what the browser shows, not what the page last rendered.
    ///
    /// # Panics
    /// Panics if they are not in time, with those checked instead.
    pub async fn expect_checked(&self, path: &str, labels: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(
                By::Css(format!(
                    "#{} > li > label:has(> input:checked)",
                    field_id(path)
                )),
                labels,
            )
            .await
    }

    /// Waits until how the phase is played reads `answers`, the answers of its fields in order,
    /// e.g. `"round robin"`, with the plugins of a list joined by commas.
    ///
    /// # Panics
    /// Panics if it does not in time, with what it reads instead.
    pub async fn expect_phase(&self, answers: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(
                By::Css("section[aria-labelledby=phase] > dl > dd > .answer"),
                answers,
            )
            .await
    }

    /// Waits until the dialog open shows why what was done in it was refused, `text`.
    ///
    /// # Panics
    /// Panics if it does not in time, with what it shows instead.
    pub async fn expect_dialog_error(&self, text: &str) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("[role=dialog] [role=alert]"), &[text])
            .await
    }

    /// Waits until no dialog is open.
    ///
    /// # Panics
    /// Panics if one still is in time.
    pub async fn expect_no_dialog(&self) -> WebDriverResult<()> {
        self.page.expect_texts(By::Css("[role=dialog]"), &[]).await
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

    /// Waits until the matches of the list shown that have a result are those numbered `numbers`,
    /// in this order, whoever won them.
    ///
    /// # Panics
    /// Panics if they are not in time, with the numbers shown instead.
    pub async fn expect_finished(&self, numbers: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(
                By::Css("#match-list tbody tr:has(.result) td:first-child"),
                numbers,
            )
            .await
    }

    /// Waits until the stages of the matches of the round shown are `stages`, in the order of the
    /// matches, e.g. `"Semifinals"`.
    ///
    /// # Panics
    /// Panics if they are not in time, with the stages shown instead.
    pub async fn expect_stages(&self, stages: &[&str]) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css("#match-list tbody td:nth-child(2)"), stages)
            .await
    }

    /// Reports the result `entry` says by typing it, e.g. `"3 1"`, in place of what was typed
    /// before, and pressing Enter.
    pub async fn enter_result(&self, entry: &str) -> WebDriverResult<()> {
        let input = self.driver.query(By::Id("result-entry")).first().await?;
        // selected, so that typing replaces it, as an operator would
        input.send_keys(Key::Control + "a").await?;
        input.send_keys(entry).await?;
        Page::pause().await;
        input.send_keys(Key::Enter).await?;
        Page::pause().await;
        Ok(())
    }

    /// Presses `key` where the focus is, e.g. [`Key::Enter`] on a button.
    pub async fn press(&self, key: Key) -> WebDriverResult<()> {
        self.driver.active_element().await?.send_keys(key).await?;
        Page::pause().await;
        Ok(())
    }

    /// Waits until the element with the focus shows `text`, e.g. a button.
    ///
    /// # Panics
    /// Panics if it does not in time, with what the element with the focus shows instead.
    pub async fn expect_focus(&self, text: &str) -> WebDriverResult<()> {
        self.page.expect_texts(By::Css(":focus"), &[text]).await
    }

    /// Waits until the focus is in the line of the match numbered `number` of the list shown, e.g.
    /// on a button reporting its result.
    ///
    /// # Panics
    /// Panics if it is not in time, with the number of the match it is in instead, if any.
    pub async fn expect_focus_in_match(&self, number: &str) -> WebDriverResult<()> {
        self.page
            .expect_texts(
                By::Css("#match-list tbody tr:has(:focus) td:first-child"),
                &[number],
            )
            .await
    }

    /// Waits until the page announces that the last action did `text`.
    ///
    /// # Panics
    /// Panics if it does not in time, with what it announces instead.
    pub async fn expect_announcement(&self, text: &str) -> WebDriverResult<()> {
        self.page
            .expect_texts(By::Css(".announcement"), &[text])
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

    /// Waits until the warning that the browser may not keep the competitions says `texts`, one
    /// per paragraph, or until it is not shown for none.
    ///
    /// # Panics
    /// Panics if it does not in time, with what it says instead.
    pub async fn expect_storage_warning(&self, texts: &[&str]) -> WebDriverResult<()> {
        self.page.expect_texts(By::Css(".warning p"), texts).await
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

/// The id of the element of the field at `path` of the form of a phase, as the GUI makes it, e.g.
/// `field-metrics-1-points_for_win` for `metrics[1].points_for_win`.
fn field_id(path: &str) -> String {
    let path = path.replace(['.', '[', ']'], "-").replace("--", "-");
    format!("field-{}", path.trim_end_matches('-'))
}

/// The XPath of the string `expression` without the [`ISOLATION_MARKS`] in it.
fn without_isolation_marks(expression: &str) -> String {
    let marks: String = ISOLATION_MARKS.iter().collect();
    format!("translate({expression}, '{marks}', '')")
}
