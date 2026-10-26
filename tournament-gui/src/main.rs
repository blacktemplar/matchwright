//! The GUI for running a tournament, in the browser, on a
//! [`Session`](tournament_app::session::Session) of `tournament-app`, which does everything but
//! laying out what it hands out.
//!
//! The competitions are stored in the browser's IndexedDB, see [`tournament_browser`]. The page
//! starts with the list of them ([`competitions`]); the one open is named after the `#` of the URL
//! and saved after each change ([`competition`]). Everything the page says goes through a
//! [`Translator`] of the language chosen, see [`i18n`].
//!
//! The page is an app that works offline and can be installed: `public/` holds its manifest, the
//! service worker `sw.js` that keeps it, and `pwa.js`, which registers the worker and keeps the
//! browser's offer to install it (see [`install`]).

mod announcement;
mod competition;
mod competitions;
mod components;
mod i18n;
mod icons;
mod install;
mod notices;
mod phase;
mod play;
mod setup;
mod theme;
mod toggletip;

use dioxus::prelude::*;
use futures_util::StreamExt as _;
use tournament_app::{
    i18n::{Language, Text, Translator},
    storage::CompetitionKey,
    view::{EntrantRow, MatchRow},
};
use tournament_browser::{channel::Events, indexed_db::IndexedDb, persistence};
use tournament_engine::{MatchResult, uuid::Uuid};

use crate::{
    competition::Competition, competitions::Competitions, i18n::t, notices::Notices, theme::Theme,
};

/// The variables the styles of the [`components`] are built from.
const COMPONENTS_THEME: Asset = asset!("/assets/dx-components-theme.css");
const STYLE: Asset = asset!("/assets/style.css");

/// The database of IndexedDB the competitions are stored in.
const DATABASE: &str = "tournament-gui";

fn main() {
    dioxus::launch(App);
}

/// The state every screen shares. Signals are `Copy` handles, so this is too.
#[derive(Clone, Copy)]
struct Page {
    /// Where the competitions are stored, once the database is open.
    storage: Signal<Option<IndexedDb>>,
    /// What the page says in the language chosen.
    translator: Signal<Translator>,
    /// Whether the page is light or dark.
    theme: Signal<Theme>,
    /// Why the last action was refused, shown until the next one.
    error: Signal<Option<Text>>,
    /// Whether the browser keeps what is stored even when short of space; `None` until asked.
    persisted: Signal<Option<bool>>,
    /// Whether the operator chose not to see the warning that the browser may not keep what is
    /// stored.
    warning_dismissed: Signal<bool>,
    /// Whether the browser offers to install the page as an app.
    installable: Signal<bool>,
    /// The name of the competition shown, which the page is headed with.
    shown: Signal<Option<String>>,
}

impl Page {
    /// Says everything in `language` from now on, and remembers it as the one chosen.
    fn choose_language(mut self, language: Language) {
        self.translator
            .set(Translator::new(language, &[i18n::resources]));
        i18n::remember(language);
    }

    /// Shows the page in the colours of `theme` from now on, and remembers it as the one chosen.
    fn choose_theme(mut self, theme: Theme) {
        self.theme.set(theme);
        theme::remember(theme);
    }

    /// Asks the browser to keep what is stored, unless it was asked already.
    fn ask_to_persist(self) {
        if self.persisted.peek().is_none() {
            self.ask_again_to_persist();
        }
    }

    /// Asks the browser to keep what is stored, e.g. once the page is installed, after which it
    /// usually does.
    fn ask_again_to_persist(mut self) {
        spawn(async move { self.persisted.set(Some(persistence::persist().await)) });
    }
}

#[component]
fn App() -> Element {
    let mut page = use_context_provider(|| Page {
        storage: Signal::new(None),
        translator: Signal::new(Translator::new(
            i18n::initial_language(),
            &[i18n::resources],
        )),
        theme: Signal::new(theme::initial()),
        error: Signal::new(None),
        persisted: Signal::new(None),
        warning_dismissed: Signal::new(notices::warning_dismissed()),
        installable: Signal::new(install::offered()),
        shown: Signal::new(None),
    });
    toggletip::use_toggletips();
    let mut open = use_signal(opened_in_url);
    // runs again whenever the language changes
    use_effect(move || i18n::set_page_language(page.translator.read().language()));
    // and whenever the theme does
    use_effect(move || theme::apply(*page.theme.read()));
    use_future(move || async move {
        match IndexedDb::open(DATABASE).await {
            Ok(storage) => page.storage.set(Some(storage)),
            Err(error) => page.error.set(Some(
                Text::new("error-no-storage").with("reason", error.to_string()),
            )),
        }
    });
    use_future(move || async move {
        let window = web_sys::window().expect("the page runs in a window");
        let mut changes = Events::new(&window, "hashchange");
        while changes.next().await.is_some() {
            page.error.set(None);
            open.set(opened_in_url());
        }
    });
    use_future(move || async move {
        let window = web_sys::window().expect("the page runs in a window");
        let mut offers = Events::new(&window, install::OFFERED);
        while offers.next().await.is_some() {
            page.installable.set(true);
        }
    });
    use_future(move || async move {
        let window = web_sys::window().expect("the page runs in a window");
        let mut installed = Events::new(&window, install::INSTALLED);
        while installed.next().await.is_some() {
            page.installable.set(false);
            if page.persisted.peek().is_some() {
                page.ask_again_to_persist();
            }
        }
    });
    let stored = page.storage.read().is_some();
    rsx! {
        // in this order, so that the page's own style comes last
        document::Stylesheet { href: COMPONENTS_THEME }
        document::Stylesheet { href: STYLE }
        // the same for every screen, so that its elements stay while the screens change
        Header {}
        if !stored {
            main { Notices {} }
        } else if let Some(key) = open() {
            // made anew for another competition
            Competition { competition: key, key: "{key}" }
        } else {
            Competitions {}
        }
        Footer {}
    }
}

/// The competition the URL names after its `#`, if any.
fn opened_in_url() -> Option<CompetitionKey> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let key = Uuid::parse_str(hash.strip_prefix('#')?).ok()?;
    Some(CompetitionKey::new(key))
}

/// Opens the competition `key`, or the list of competitions for `None`.
fn open(key: Option<CompetitionKey>) {
    if let Some(window) = web_sys::window() {
        // an entry of the history, so that going back returns; the page follows the change
        _ = window
            .location()
            .set_hash(&key.map(|key| key.to_string()).unwrap_or_default());
    }
}

/// The top of every screen: its heading, the name of the competition shown or else the
/// application's, and while a competition is shown the way back to all of them. The screen of a
/// competition follows it with its actions, in the same bar.
#[component]
fn Header() -> Element {
    let page = use_context::<Page>();
    let tr = page.translator.read();
    let shown = page.shown.read().clone();
    let (title, heading) = match &shown {
        Some(name) => (
            t!(tr, "competition-title", name = name.as_str()),
            name.clone(),
        ),
        None => (tr.get("app-title"), tr.get("app-title")),
    };
    rsx! {
        document::Title { "{title}" }
        header { class: "bar",
            if shown.is_some() {
                a { class: "back", href: "#",
                    icons::ChevronLeft {}
                    span { class: "bar-label", {tr.get("all-competitions")} }
                }
            }
            h1 { "{heading}" }
        }
    }
}

/// The bottom of every screen: the choice of the language and of the colours.
#[component]
fn Footer() -> Element {
    let page = use_context::<Page>();
    let tr = page.translator.read();
    let language = tr.language();
    rsx! {
        footer {
            label {
                {tr.get("language")}
                " "
                select {
                    id: "language",
                    onchange: move |event| {
                        if let Some(chosen) = Language::from_tag(&event.value()) {
                            page.choose_language(chosen);
                        }
                    },
                    for option in Language::ALL {
                        option {
                            value: option.tag(),
                            lang: option.tag(),
                            selected: option == language,
                            {option.name()}
                        }
                    }
                }
            }
            label {
                {tr.get("theme")}
                " "
                select {
                    id: "theme",
                    onchange: move |event| {
                        if let Some(chosen) = Theme::from_tag(&event.value()) {
                            page.choose_theme(chosen);
                        }
                    },
                    for option in Theme::ALL {
                        option {
                            value: option.tag(),
                            selected: option == *page.theme.read(),
                            {tr.get(option.message())}
                        }
                    }
                }
            }
        }
    }
}

/// What a screen shows for `entrants`, a team or a single entrant.
fn team_name(tr: &Translator, entrants: &[EntrantRow]) -> String {
    let names: Vec<_> = entrants
        .iter()
        .map(|entrant| entrant.name.clone().unwrap_or_else(|| tr.get("no-name")))
        .collect();
    names.join(&tr.get("team-separator"))
}

/// What a screen shows for the result of the match `row`, e.g. "Alice won"; `None` while it has
/// none.
fn result_text(tr: &Translator, row: &MatchRow) -> Option<String> {
    Some(match row.result? {
        MatchResult::AWins => t!(tr, "won", team = team_name(tr, &row.a)),
        MatchResult::BWins => t!(tr, "won", team = team_name(tr, &row.b)),
        MatchResult::Draw => tr.get("draw"),
    })
}

/// Has the browser download `file`, a competition's [file](tournament_app::file), named after the
/// competition `name`; shows why if it cannot.
fn download(mut page: Page, name: &str, file: &str) {
    if let Err(error) =
        tournament_browser::file::download(&format!("{name}.json"), "application/json", file)
    {
        page.error.set(Some(
            Text::new("error-not-exported").with("reason", format!("{error:?}")),
        ));
    }
}
