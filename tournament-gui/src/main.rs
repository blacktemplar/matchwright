//! The GUI for running a tournament, in the browser: one screen on a
//! [`Session`] of `tournament-app`, which does everything but
//! laying out what it hands out.
//!
//! The competition is saved to a [`MemoryStorage`] after each change, so it is lost with the page.

mod play;
mod setup;

use std::fmt::Display;

use dioxus::prelude::*;
use tournament_app::{
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
    /// Why the last action was refused, shown until the next one.
    error: Signal<Option<String>>,
}

impl Ctx {
    /// Runs `action` on the session and saves what it changed; shows why if it is refused. Returns
    /// whether it was done.
    fn act<T, E: Display>(mut self, action: impl FnOnce(&mut Session) -> Result<T, E>) -> bool {
        let result = action(&mut self.session.write());
        match result {
            Ok(_) => {
                self.error.set(None);
                self.save();
                true
            }
            Err(error) => {
                self.error.set(Some(error.to_string()));
                false
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
                Err(error) => self
                    .error
                    .set(Some(format!("the changes could not be saved: {error}"))),
            }
        });
    }
}

#[component]
fn App() -> Element {
    let ctx = use_context_provider(|| Ctx {
        session: Signal::new(Session::new("Tournament")),
        storage: Signal::new(MemoryStorage::default()),
        error: Signal::new(None),
    });
    let (actions, started) = {
        let session = ctx.session.read();
        (session.actions(), session.competition().is_started())
    };
    rsx! {
        document::Stylesheet { href: STYLE }
        header {
            h1 { "Tournament" }
            button {
                disabled: !actions.undo,
                onclick: move |_| _ = ctx.act(Session::undo),
                "Undo"
            }
            button {
                disabled: !actions.redo,
                onclick: move |_| _ = ctx.act(Session::redo),
                "Redo"
            }
        }
        main {
            p { class: "error", role: "alert", {ctx.error.cloned()} }
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
fn team_name(entrants: &[EntrantRow]) -> String {
    let names: Vec<_> = entrants
        .iter()
        .map(|entrant| entrant.name.as_deref().unwrap_or("(no name)"))
        .collect();
    names.join(" & ")
}
