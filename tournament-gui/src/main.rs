//! The GUI for running a tournament, in the browser: one screen on a
//! [`Session`] of `tournament-app`, which does everything but
//! laying out what it hands out.
//!
//! The competition is saved to a [`MemoryStorage`] after each change, so it is lost with the page.
//! Everything the screen says goes through a [`Translator`] of the language chosen, see [`i18n`].

mod i18n;
mod play;
mod setup;

use dioxus::prelude::*;
use tournament_app::{
    i18n::{Language, Message, Text, Translator},
    session::Session,
    storage::{MemoryStorage, Storage as _},
    view::EntrantRow,
};

use crate::{
    play::{Rounds, Standings},
    setup::{Entrants, Phase},
};

const STYLE: Asset = asset!("/assets/style.css");

fn main() {
    dioxus::launch(App);
}

/// The state every component shares. Signals are `Copy` handles, so this is too.
#[derive(Clone, Copy)]
struct Ctx {
    session: Signal<Session>,
    storage: Signal<MemoryStorage>,
    /// What the screen says in the language chosen.
    translator: Signal<Translator>,
    /// Why the last action was refused, shown until the next one.
    error: Signal<Option<Text>>,
}

impl Ctx {
    /// Runs `action` on the session and saves what it changed; shows why if it is refused. Returns
    /// what it returned if it was done.
    fn act<T, E: Message>(
        mut self,
        action: impl FnOnce(&mut Session) -> Result<T, E>,
    ) -> Option<T> {
        let result = action(&mut self.session.write());
        match result {
            Ok(done) => {
                self.error.set(None);
                self.save();
                Some(done)
            }
            Err(error) => {
                self.error.set(Some(error.text()));
                None
            }
        }
    }

    /// Writes what changed since the last save, without holding the session across the write.
    fn save(mut self) {
        let changes = self.session.peek().unsaved_changes();
        let storage = self.storage.peek().clone();
        spawn(async move {
            match storage.write(&changes).await {
                Ok(()) => self.session.write().mark_saved(&changes),
                Err(error) => self.error.set(Some(
                    Text::new("error-not-saved").with("reason", error.to_string()),
                )),
            }
        });
    }

    /// Says everything in `language` from now on, and remembers it as the one chosen.
    fn choose_language(mut self, language: Language) {
        self.translator
            .set(Translator::new(language, &[i18n::resources]));
        i18n::remember(language);
    }
}

#[component]
fn App() -> Element {
    let ctx = use_context_provider(|| Ctx {
        session: Signal::new(Session::new("Tournament")),
        storage: Signal::new(MemoryStorage::default()),
        translator: Signal::new(Translator::new(
            i18n::initial_language(),
            &[i18n::resources],
        )),
        error: Signal::new(None),
    });
    // runs again whenever the language changes
    use_effect(move || i18n::set_page_language(ctx.translator.read().language()));
    let (actions, started) = {
        let session = ctx.session.read();
        (session.actions(), session.competition().is_started())
    };
    let tr = ctx.translator.read();
    let language = tr.language();
    rsx! {
        document::Stylesheet { href: STYLE }
        document::Title { {tr.get("app-title")} }
        header {
            h1 { {tr.get("app-title")} }
            label {
                {tr.get("language")}
                " "
                select {
                    id: "language",
                    onchange: move |event| {
                        if let Some(chosen) = Language::from_tag(&event.value()) {
                            ctx.choose_language(chosen);
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
            button {
                disabled: !actions.undo,
                onclick: move |_| _ = ctx.act(Session::undo),
                {tr.get("undo")}
            }
            button {
                disabled: !actions.redo,
                onclick: move |_| _ = ctx.act(Session::redo),
                {tr.get("redo")}
            }
        }
        main {
            p { class: "error", role: "alert",
                if let Some(error) = ctx.error.read().as_ref() {
                    {tr.text(error)}
                }
            }
            if started {
                Rounds {}
                Standings {}
            } else {
                Entrants {}
                Phase {}
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
