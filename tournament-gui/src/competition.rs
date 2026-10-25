//! The screen of the competition open: entering it before the start, playing it after.
//!
//! One tab at a time changes a competition, the one holding its lock, see
//! [`tournament_browser::lock`]. It saves after each change and then tells the other tabs on the
//! channel [`SAVED`]. Another tab shows it read-only, catches up whenever told, and takes over
//! once the lock is released, e.g. when the tab changing it is closed.

use std::{cell::RefCell, rc::Rc};

use dioxus::{core::spawn_forever, prelude::*};
use futures_util::{
    StreamExt as _,
    future::{Either, select},
};
use tournament_app::{
    error::LoadError,
    i18n::{Message, Text, Translator},
    session::Session,
    storage::{CompetitionKey, Storage as _},
    view::Actions,
};
use tournament_browser::{
    channel::Broadcast,
    indexed_db::IndexedDb,
    lock::{self, Lock},
};

use crate::{
    Notices, Page, download,
    phase::PhaseSummary,
    play::{Rounds, Standings},
    setup::{Entrants, Phase},
};

/// The channel on which a tab tells the others that it saved, with the key of the competition.
pub const SAVED: &str = "tournament-gui.saved";

/// The message telling that the competition is changed in another tab.
const READ_ONLY: &str = "read-only";

/// The name of the lock of the competition `key`, held by the tab that changes it.
fn lock_name(key: CompetitionKey) -> String {
    format!("tournament-gui.competition.{key}")
}

/// The state the components of the screen share. Signals are `Copy` handles, so this is too.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub session: Signal<Session>,
    /// The lock that lets this tab change the competition, `None` while another tab holds it. Each
    /// save under way holds it too, so that it is released only once they are written.
    lock: Signal<Option<Rc<Lock>>>,
    storage: Signal<Option<IndexedDb>>,
    pub translator: Signal<Translator>,
    error: Signal<Option<Text>>,
}

impl Ctx {
    /// Whether this tab changes the competition, rather than showing what another tab does.
    pub fn changes(&self) -> bool {
        self.lock.read().is_some()
    }

    /// What the operator can do now in this tab: nothing while another tab changes the competition.
    pub fn actions(&self) -> Actions {
        if self.changes() {
            self.session.read().actions()
        } else {
            Actions::default()
        }
    }

    /// Runs `action` on the session and saves what it changed; shows why if it is refused. Returns
    /// what it returned if it was done, and `None` as well while another tab changes the
    /// competition.
    pub fn act<T, E: Message>(
        mut self,
        action: impl FnOnce(&mut Session) -> Result<T, E>,
    ) -> Option<T> {
        if self.lock.peek().is_none() {
            return None;
        }
        match self.try_act(action) {
            Ok(done) => {
                self.error.set(None);
                Some(done)
            }
            Err(error) => {
                self.error.set(Some(error));
                None
            }
        }
    }

    /// Runs `action` on the session and saves what it changed, as [`Self::act`] does, but returns
    /// why it was refused rather than showing it, e.g. for a dialog to show it.
    ///
    /// # Errors
    /// Returns what the error of `action` says, and [`READ_ONLY`] while another tab changes the
    /// competition.
    pub fn try_act<T, E: Message>(
        self,
        action: impl FnOnce(&mut Session) -> Result<T, E>,
    ) -> Result<T, Text> {
        if self.lock.peek().is_none() {
            return Err(Text::new(READ_ONLY));
        }
        let mut session = self.session;
        let done = action(&mut session.write()).map_err(|error| error.text())?;
        self.save();
        Ok(done)
    }

    /// Writes what changed since the last save, without holding the session across the write, and
    /// tells the other tabs once it is written.
    fn save(mut self) {
        if !self.session.peek().has_unsaved_changes() {
            return;
        }
        let changes = self.session.peek().unsaved_changes();
        let storage = self
            .storage
            .peek()
            .clone()
            .expect("a competition is open once the database is");
        let lock = self.lock.peek().clone();
        // not ended with the screen, so that leaving it right after a change still saves the change
        spawn_forever(async move {
            match storage.write(&changes).await {
                Ok(()) => {
                    // the screen may be gone, and its session with it
                    if let Ok(mut session) = self.session.try_write() {
                        session.mark_saved(&changes);
                    }
                    Broadcast::new(SAVED).post(&changes.listing().key.to_string());
                }
                Err(error) => self.error.set(Some(
                    Text::new("error-not-saved").with("reason", error.to_string()),
                )),
            }
            drop(lock);
        });
    }
}

/// A competition loaded, and the lock if this tab took it.
type Opened = (Session, Option<Lock>);

/// A competition loaded, handed over to its screen once.
#[derive(Clone)]
struct Loaded(Rc<RefCell<Option<Opened>>>);

impl PartialEq for Loaded {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq)]
enum Opening {
    Loading,
    Failed,
    Loaded(Loaded),
}

/// Opens the competition `competition`: shows it once loaded, or else why not.
#[component]
pub fn Competition(competition: CompetitionKey) -> Element {
    let mut page = use_context::<Page>();
    let mut opening = use_signal(|| Opening::Loading);
    use_hook(move || page.ask_to_persist());
    use_future(move || async move {
        let storage = page
            .storage
            .peek()
            .clone()
            .expect("a competition is opened once the database is open");
        let lock = lock::try_lock(&lock_name(competition)).await;
        match Session::load(&storage, competition).await {
            Ok(session) => opening.set(Opening::Loaded(Loaded(Rc::new(RefCell::new(Some((
                session, lock,
            ))))))),
            Err(error) => {
                page.error.set(Some(error.text()));
                opening.set(Opening::Failed);
            }
        }
    });
    let tr = page.translator.read();
    match opening() {
        Opening::Loaded(loaded) => rsx! {
            Screen { competition, loaded }
        },
        opening => rsx! {
            main {
                Notices {}
                if opening == Opening::Loading {
                    p { {tr.get("opening")} }
                }
                p {
                    a { href: "#", {tr.get("all-competitions")} }
                }
            }
        },
    }
}

#[component]
fn Screen(competition: CompetitionKey, loaded: Loaded) -> Element {
    let page = use_context::<Page>();
    let ctx = use_context_provider(|| {
        let (session, lock) = loaded
            .0
            .borrow_mut()
            .take()
            .expect("a competition loaded is shown once");
        Ctx {
            session: Signal::new(session),
            lock: Signal::new(lock.map(Rc::new)),
            storage: page.storage,
            translator: page.translator,
            error: page.error,
        }
    });
    use_future(move || follow(ctx, competition));
    let mut shown = page.shown;
    use_effect(move || {
        let session = ctx.session.read();
        if shown.peek().as_deref() != Some(session.name()) {
            shown.set(Some(session.name().to_owned()));
        }
    });
    use_drop(move || shown.set(None));
    let changes = ctx.changes();
    let actions = ctx.actions();
    let started = ctx.session.read().competition().is_started();
    let tr = ctx.translator.read();
    rsx! {
        nav { aria_label: tr.get("competition-actions"),
            a { href: "#", {tr.get("all-competitions")} }
            " "
            button {
                onclick: move |_| {
                    let session = ctx.session.peek();
                    download(page, session.name(), &session.export());
                },
                {tr.get("export")}
            }
            " "
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
            Notices {}
            if !changes {
                p { class: "notice", role: "status", {tr.get(READ_ONLY)} }
            }
            if started {
                Rounds {}
                Standings {}
                section { aria_labelledby: "phase",
                    h2 { id: "phase", {tr.get("phase")} }
                    PhaseSummary {}
                }
            } else {
                Entrants {}
                Phase {}
            }
        }
    }
}

/// While another tab changes the competition `key`, catches up whenever that tab saved it, and
/// takes over once the lock is released.
async fn follow(mut ctx: Ctx, key: CompetitionKey) {
    if ctx.lock.peek().is_some() {
        return;
    }
    let channel = Broadcast::new(SAVED);
    let mut saved = channel.messages();
    let name = lock_name(key);
    let mut taken = Box::pin(lock::lock(&name));
    let lock = loop {
        match select(saved.next(), taken).await {
            Either::Left((message, waiting)) => {
                taken = waiting;
                if message.as_deref() == Some(&*key.to_string()) {
                    catch_up(ctx, key).await;
                }
            }
            Either::Right((lock, _)) => break lock,
        }
    };
    // what the other tab saved last
    catch_up(ctx, key).await;
    ctx.lock.set(Some(Rc::new(lock)));
}

/// Appends what was stored of the competition `key` since the session loaded or caught up last,
/// without holding the session across the read.
async fn catch_up(mut ctx: Ctx, key: CompetitionKey) {
    let storage = ctx
        .storage
        .peek()
        .clone()
        .expect("a competition is open once the database is");
    let first_entry = ctx.session.peek().next_entry();
    let caught_up = match storage.read(key, first_entry).await {
        Ok(Some(stored)) => ctx.session.write().catch_up_with(&stored, first_entry),
        Ok(None) => Err(LoadError::NotFound),
        Err(error) => Err(LoadError::Storage(error)),
    };
    if let Err(error) = caught_up {
        ctx.error.set(Some(error.text()));
    }
}
