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
    view::{Action, Actions, EntrantRow},
};
use tournament_browser::{
    channel::Broadcast,
    indexed_db::IndexedDb,
    lock::{self, Lock},
};

use crate::{
    Notices, Page,
    announcement::Announcement,
    bracket::Bracket,
    capitalized,
    components::dialog::{Dialog, DialogDescription, DialogTitle},
    cross_table::CrossTable,
    download, icons,
    phase::PhaseSummary,
    play::{Rounds, Standings},
    setup::{Entrants, Phase},
    winners_text,
};

/// The channel on which a tab tells the others that it saved, with the key of the competition.
pub const SAVED: &str = "matchwright.saved";

/// The message telling that the competition is changed in another tab.
const READ_ONLY: &str = "read-only";

/// The name of the lock of the competition `key`, held by the tab that changes it.
pub fn lock_name(key: CompetitionKey) -> String {
    format!("matchwright.competition.{key}")
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
    /// What the last action did, shown until the next one.
    announcement: Signal<Option<Announcement>>,
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
    /// competition. Either way the announcement of the action before is gone.
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
                self.announcement.set(None);
                self.error.set(Some(error));
                None
            }
        }
    }

    /// Runs `action` on the session and saves what it changed, as [`Self::act`] does, but returns
    /// why it was refused rather than showing it, e.g. for a dialog to show it. If it was done,
    /// the announcement of the action before is gone.
    ///
    /// # Errors
    /// Returns what the error of `action` says, and [`READ_ONLY`] while another tab changes the
    /// competition.
    pub fn try_act<T, E: Message>(
        mut self,
        action: impl FnOnce(&mut Session) -> Result<T, E>,
    ) -> Result<T, Text> {
        if self.lock.peek().is_none() {
            return Err(Text::new(READ_ONLY));
        }
        let mut session = self.session;
        let done = action(&mut session.write()).map_err(|error| error.text())?;
        self.announcement.set(None);
        self.save();
        Ok(done)
    }

    /// Tells the operator what the action just done did, until the next action.
    pub fn announce(mut self, announcement: Announcement) {
        self.announcement.set(Some(announcement));
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
                    Text::new("error-not-saved").with_detail(error.to_string()),
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
            announcement: Signal::new(None),
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
    // whether the operator is asked to confirm undoing the start
    let mut undoing_start = use_signal(|| false);
    let tr = ctx.translator.read();
    let export = move || {
        let session = ctx.session.peek();
        download(page, session.name(), &session.export());
    };
    rsx! {
        // in the bar the header starts
        nav { class: "bar bar-actions", aria_label: tr.get("competition-actions"),
            // focusable while there is nothing to undo or redo, so that the focus stays on the
            // button that undid or redid the last action
            button {
                class: "quiet",
                aria_disabled: (!actions.undo).then_some("true"),
                onclick: move |_| {
                    if !actions.undo {
                        return;
                    }
                    // leaving play, which is asked about first
                    if ctx.session.peek().next_undo() == Some(&Action::Start) {
                        undoing_start.set(true);
                    } else {
                        undo(ctx);
                    }
                },
                icons::Undo {}
                span { class: "bar-label", {tr.get("undo")} }
            }
            button {
                class: "quiet",
                aria_disabled: (!actions.redo).then_some("true"),
                onclick: move |_| {
                    if !actions.redo {
                        return;
                    }
                    if let Some(action) = ctx.act(Session::redo) {
                        ctx.announce(Announcement::Redone(action));
                    }
                },
                icons::Redo {}
                span { class: "bar-label", {tr.get("redo")} }
            }
            button { class: "quiet", onclick: move |_| export(),
                icons::Export {}
                span { class: "bar-label", {tr.get("export")} }
            }
        }
        main {
            Notices { export: move |()| export() }
            if !changes {
                p { class: "notice", role: "status", {tr.get(READ_ONLY)} }
            }
            // always on the page, so that a screen reader notices when its text changes
            p { class: "announcement", role: "status",
                if let Some(announcement) = ctx.announcement.read().as_ref() {
                    {announcement.text(&tr)}
                }
            }
            if started {
                Play {}
            } else {
                div { class: "columns setup",
                    Entrants {}
                    Phase {}
                }
            }
        }
        Dialog {
            open: Some(undoing_start()),
            on_open_change: move |open: bool| undoing_start.set(open),
            DialogTitle { {tr.get("undo-start-title")} }
            DialogDescription { {tr.get("undo-start-description")} }
            div { class: "dialog-buttons",
                button {
                    class: "primary",
                    onclick: move |_| {
                        undoing_start.set(false);
                        undo(ctx);
                    },
                    {tr.get("undo-start")}
                }
                button { onclick: move |_| undoing_start.set(false), {tr.get("cancel")} }
            }
        }
    }
}

/// Undoes the latest action of the session, and says what it undid.
fn undo(ctx: Ctx) {
    if let Some(action) = ctx.act(Session::undo) {
        ctx.announce(Announcement::Undone(action));
    }
}

/// What the screen after the start shows: the matches or the standings, both side by side where
/// there is room for two columns, the chart of the phase, or how the phase is played.
#[derive(Clone, Copy, PartialEq)]
enum Column {
    Matches,
    Standings,
    Chart,
    Phase,
}

/// What a phase shows of all its matches at once, if anything.
#[derive(Clone, Copy, PartialEq)]
enum Chart {
    /// That of a knockout.
    Bracket,
    /// That of teams that stay the same, e.g. in a round robin.
    CrossTable,
}

/// The screen after the start: the matches and next to them the standings, or in their place the
/// chart of the phase (its bracket or its cross table) or how the phase is played, on the full
/// width. Buttons on top choose what is shown: the matches, the standings, the chart or the phase
/// where there is room for one column only, else the two columns, the chart or the phase.
/// Everything stays on the page, hidden while not shown.
#[component]
fn Play() -> Element {
    let ctx = use_context::<Ctx>();
    let mut shown = use_signal(|| {
        if chart_remembered() {
            Column::Chart
        } else {
            Column::Matches
        }
    });
    let chart = if ctx.session.read().bracket().is_some() {
        Some(Chart::Bracket)
    } else if ctx.session.read().cross_table().is_some() {
        Some(Chart::CrossTable)
    } else {
        None
    };
    // a phase without a chart shows the matches, whatever was chosen for another phase
    let current = match shown() {
        Column::Chart if chart.is_none() => Column::Matches,
        column => column,
    };
    let mut show = move |column| {
        shown.set(column);
        // the phase is looked at now and then, so the next visit shows what was played on
        if column != Column::Phase {
            remember_chart(column == Column::Chart);
        }
    };
    let tr = ctx.translator.read();
    let column = |column| {
        if current == column {
            "column"
        } else {
            "column hidden-narrow"
        }
    };
    let columns_shown = matches!(current, Column::Matches | Column::Standings);
    let winners = ctx.session.read().winners();
    rsx! {
        if let Some(winners) = &winners {
            Finished { winners: winners.clone() }
        }
        div {
            class: "segmented view-switch",
            role: "group",
            aria_label: tr.get("view"),
            button {
                class: "narrow-only",
                aria_pressed: current == Column::Matches,
                onclick: move |_| show(Column::Matches),
                {tr.get("matches")}
            }
            button {
                class: "narrow-only",
                aria_pressed: current == Column::Standings,
                onclick: move |_| show(Column::Standings),
                {tr.get("standings")}
            }
            button {
                class: "wide-only",
                aria_pressed: columns_shown,
                onclick: move |_| show(Column::Matches),
                {tr.get("matches-and-standings")}
            }
            if let Some(chart) = chart {
                button {
                    aria_pressed: current == Column::Chart,
                    onclick: move |_| show(Column::Chart),
                    match chart {
                        Chart::Bracket => tr.get("bracket"),
                        Chart::CrossTable => tr.get("cross-table"),
                    }
                }
            }
            button {
                aria_pressed: current == Column::Phase,
                onclick: move |_| show(Column::Phase),
                {tr.get("phase-heading")}
            }
        }
        div { class: "columns play",
            div { class: column(Column::Matches), hidden: !columns_shown, Rounds {} }
            div { class: column(Column::Standings), hidden: !columns_shown, Standings {} }
            if let Some(chart) = chart {
                div { class: "column wide", hidden: current != Column::Chart,
                    match chart {
                        Chart::Bracket => rsx! {
                            Bracket {}
                        },
                        Chart::CrossTable => rsx! {
                            CrossTable {}
                        },
                    }
                }
            }
            div { class: "column wide", hidden: current != Column::Phase,
                section { class: "card", aria_labelledby: "phase",
                    h2 { id: "phase", {tr.get("phase-heading")} }
                    PhaseSummary {}
                }
            }
        }
    }
}

/// Where the browser remembers that the operator chose the chart, which a phase with one shows
/// first then.
const CHART_SHOWN: &str = "matchwright.chart-shown";

/// Whether the operator chose the chart last in this browser.
fn chart_remembered() -> bool {
    local_storage()
        .and_then(|storage| storage.get_item(CHART_SHOWN).ok().flatten())
        .is_some()
}

fn remember_chart(shown: bool) {
    // without local storage, the matches are shown first on the next visit
    if let Some(storage) = local_storage() {
        _ = if shown {
            storage.set_item(CHART_SHOWN, "true")
        } else {
            storage.remove_item(CHART_SHOWN)
        };
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// What the screen says once every match is played: who won, or who is tied for first.
#[component]
fn Finished(winners: Vec<Vec<EntrantRow>>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let names: Vec<Vec<Option<String>>> = winners
        .into_iter()
        .map(|team| team.into_iter().map(|entrant| entrant.name).collect())
        .collect();
    rsx! {
        section { class: "card winners-banner", aria_labelledby: "finished",
            icons::Trophy {}
            div {
                h2 { id: "finished", {tr.get("finished")} }
                p { {capitalized(&winners_text(&tr, &names))} }
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
