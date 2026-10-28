//! The list of the competitions stored in the browser, to open one, export it, or start another:
//! a new one or one imported from a file.

use dioxus::{html::FileData, prelude::*};
use futures_util::StreamExt as _;
use tournament_app::{
    i18n::{Message as _, Text, Translator},
    session::Session,
    storage::{CompetitionKey, Progress, Storage as _},
};
use tournament_browser::{channel::Broadcast, lock};

use crate::{
    Notices, Page,
    competition::{SAVED, lock_name},
    components::dialog::{Dialog, DialogDescription, DialogTitle},
    download,
    i18n::t,
    icons,
    notices::Detail,
    open, winners_text,
};

#[component]
pub fn Competitions() -> Element {
    let page = use_context::<Page>();
    let mut name = use_signal(String::new);
    // counts what other tabs saved, which may be a competition the list does not have yet
    let mut saved_elsewhere = use_signal(|| 0_u32);
    // counts the competitions renamed or deleted here
    let changed = use_signal(|| 0_u32);
    let mut asked = use_signal(|| None::<Ask>);
    // a click asking `what` in the dialog
    let ask = move |what: Ask| move |_| asked.set(Some(what.clone()));
    use_future(move || async move {
        let channel = Broadcast::new(SAVED);
        let mut saved = channel.messages();
        while saved.next().await.is_some() {
            saved_elsewhere += 1;
        }
    });
    let listings = use_resource(move || async move {
        saved_elsewhere();
        changed();
        let storage = page
            .storage
            .peek()
            .clone()
            .expect("the list is shown once the database is open");
        storage.list().await.map_err(|error| {
            Text::new("error-competitions-unreadable").with_detail(error.to_string())
        })
    });
    let tr = page.translator.read();
    let (listings, unreadable) = match &*listings.read() {
        Some(Ok(listings)) => (listings.clone(), None),
        Some(Err(why)) => (Vec::new(), Some(why.clone())),
        None => (Vec::new(), None),
    };
    rsx! {
        main { class: "narrow",
            Notices {}
            section { class: "card", aria_labelledby: "new-competition",
                h2 { id: "new-competition", {tr.get("new-competition")} }
                form {
                    class: "row",
                    onsubmit: move |event| {
                        event.prevent_default();
                        let chosen = name.peek().trim().to_owned();
                        if chosen.is_empty() {
                            let mut error = page.error;
                            error.set(Some(Text::new("error-empty-competition-name")));
                        } else {
                            store(page, Session::new(chosen));
                        }
                    },
                    label { r#for: "competition-name", class: "visually-hidden", {tr.get("name")} }
                    input {
                        id: "competition-name",
                        value: "{name}",
                        autocomplete: "off",
                        placeholder: tr.get("name"),
                        oninput: move |event| name.set(event.value()),
                    }
                    button { class: "primary", r#type: "submit", {tr.get("create")} }
                }
                // looks like a button; the input in it takes the focus and the file
                label { class: "button file-button",
                    {tr.get("import")}
                    input {
                        id: "import",
                        class: "visually-hidden",
                        r#type: "file",
                        accept: ".json,application/json",
                        onchange: move |event| {
                            if let Some(file) = event.files().into_iter().next() {
                                spawn(async move { import(page, file).await });
                            }
                        },
                    }
                }
            }
            section { class: "card", aria_labelledby: "competitions",
                h2 { id: "competitions", {tr.get("competitions")} }
                if let Some(why) = unreadable {
                    p { class: "error", {tr.sentence(&why)} }
                    if let Some(detail) = why.detail() {
                        Detail { detail }
                    }
                } else if listings.is_empty() {
                    p { class: "empty", {tr.get("no-competitions")} }
                }
                // the latest first, which is the one most likely to be played
                ul { id: "competition-list",
                    for listing in listings.into_iter().rev() {
                        li { key: "{listing.key}",
                            div { class: "listing",
                                a { class: "name", href: "#{listing.key}", "{listing.name}" }
                                if let Some(progress) = &listing.progress {
                                    span { class: "progress", {progress_text(&tr, progress)} }
                                }
                            }
                            div { class: "listing-buttons",
                                button {
                                    class: "small icon-only",
                                    aria_label: t!(tr, "rename-competition", name = &*listing.name),
                                    onclick: ask(Ask::Rename {
                                        key: listing.key,
                                        name: listing.name.clone(),
                                    }),
                                    icons::Pencil {}
                                }
                                button {
                                    class: "small icon-only",
                                    aria_label: t!(tr, "export-competition", name = &*listing.name),
                                    onclick: {
                                        let (key, name) = (listing.key, listing.name.clone());
                                        move |_| export(page, key, name.clone())
                                    },
                                    icons::Export {}
                                }
                                button {
                                    class: "small icon-only",
                                    aria_label: t!(tr, "delete-competition", name = &*listing.name),
                                    onclick: ask(Ask::Delete {
                                        key: listing.key,
                                        name: listing.name.clone(),
                                    }),
                                    icons::Trash {}
                                }
                            }
                        }
                    }
                }
            }
            AskDialog { asked, changed }
        }
    }
}

/// What a dialog asks the operator about a competition of the list.
#[derive(Clone, PartialEq)]
enum Ask {
    /// Its new name, in place of `name`.
    Rename { key: CompetitionKey, name: String },
    /// Whether to delete it.
    Delete { key: CompetitionKey, name: String },
}

/// The dialog asking what `asked` asks, if anything; once done, `changed` counts on, which reads
/// the list anew.
#[component]
fn AskDialog(asked: Signal<Option<Ask>>, changed: Signal<u32>) -> Element {
    let page = use_context::<Page>();
    let mut error = use_signal(|| None::<Text>);
    let mut name = use_signal(String::new);
    // the name shown, from the competition asked about until the operator types
    use_effect(move || {
        if let Some(Ask::Rename { name: current, .. }) = asked() {
            name.set(current);
        }
        error.set(None);
    });
    let tr = page.translator.read();
    let open = asked.read().is_some();
    let mut close = move || asked.set(None);
    rsx! {
        Dialog {
            open: Some(open),
            on_open_change: move |open: bool| {
                if !open {
                    asked.set(None);
                }
            },
            match asked() {
                Some(Ask::Rename { key, name: current }) => rsx! {
                    DialogTitle { {t!(tr, "rename-title", name = current.as_str())} }
                    form {
                        class: "dialog-form",
                        onsubmit: move |event| {
                            event.prevent_default();
                            let chosen = name.peek().trim().to_owned();
                            if chosen.is_empty() {
                                error.set(Some(Text::new("error-empty-competition-name")));
                                return;
                            }
                            spawn(rename(page, key, chosen, asked, changed, error));
                        },
                        label { r#for: "new-name", {tr.get("name")} }
                        input {
                            id: "new-name",
                            value: "{name}",
                            autocomplete: "off",
                            oninput: move |event| name.set(event.value()),
                        }
                        p { class: "error", role: "alert",
                            if let Some(error) = error() {
                                {tr.sentence(&error)}
                            }
                        }
                        div { class: "dialog-buttons",
                            button { class: "primary", r#type: "submit", {tr.get("rename")} }
                            button { r#type: "button", onclick: move |_| close(),
                                {tr.get("cancel")}
                            }
                        }
                    }
                },
                Some(Ask::Delete { key, name: current }) => rsx! {
                    DialogTitle { {t!(tr, "delete-title", name = current.as_str())} }
                    DialogDescription { {tr.get("delete-description")} }
                    p { class: "error", role: "alert",
                        if let Some(error) = error() {
                            {tr.sentence(&error)}
                        }
                    }
                    div { class: "dialog-buttons",
                        button {
                            class: "primary danger",
                            onclick: move |_| _ = spawn(delete(page, key, asked, changed, error)),
                            {tr.get("delete")}
                        }
                        button { r#type: "button", onclick: move |_| close(), {tr.get("cancel")} }
                    }
                },
                None => rsx! {},
            }
        }
    }
}

/// Names the competition `key` `name`, and closes the dialog, or shows in it why not.
async fn rename(
    page: Page,
    key: CompetitionKey,
    name: String,
    mut asked: Signal<Option<Ask>>,
    mut changed: Signal<u32>,
    mut error: Signal<Option<Text>>,
) {
    let Some(storage) = page.storage.peek().clone() else {
        return;
    };
    match storage.rename(key, &name).await {
        Ok(()) => {
            asked.set(None);
            changed += 1;
        }
        Err(why) => error.set(Some(
            Text::new("error-not-saved").with_detail(why.to_string()),
        )),
    }
}

/// Deletes the competition `key`, unless another tab has it open, and closes the dialog, or shows
/// in it why not.
async fn delete(
    page: Page,
    key: CompetitionKey,
    mut asked: Signal<Option<Ask>>,
    mut changed: Signal<u32>,
    mut error: Signal<Option<Text>>,
) {
    let Some(storage) = page.storage.peek().clone() else {
        return;
    };
    // held while deleting, so that no tab opens it meanwhile
    let Some(lock) = lock::try_lock(&lock_name(key)).await else {
        error.set(Some(Text::new("error-competition-open-elsewhere")));
        return;
    };
    let deleted = storage.delete(key).await;
    drop(lock);
    match deleted {
        Ok(()) => {
            asked.set(None);
            changed += 1;
        }
        Err(why) => error.set(Some(
            Text::new("error-not-deleted").with_detail(why.to_string()),
        )),
    }
}

/// What the list says of how far a competition is, e.g. "Round 3 · 2 matches open".
fn progress_text(tr: &Translator, progress: &Progress) -> String {
    match progress {
        Progress::Setup { entrants } => t!(tr, "progress-setup", entrants = *entrants),
        Progress::Playing { round, open } => {
            t!(tr, "progress-playing", round = round.get(), open = *open)
        }
        Progress::Finished => tr.get("progress-finished"),
        Progress::Won { winners } => t!(
            tr,
            "progress-won",
            winners = winners_text(tr, winners).as_str()
        ),
    }
}

/// Imports the competition of `file`, named after the file unless it says its name, and opens it.
async fn import(mut page: Page, file: FileData) {
    let text = match file.read_string().await {
        Ok(text) => text,
        Err(error) => {
            page.error.set(Some(
                Text::new("error-file-not-read").with_detail(error.to_string()),
            ));
            return;
        }
    };
    let name = file.name();
    let name = name.strip_suffix(".json").unwrap_or(&name);
    let Some(storage) = page.storage.peek().clone() else {
        return;
    };
    // so that a copy of one stored is told apart from it
    let stored = match storage.list().await {
        Ok(stored) => stored,
        Err(error) => {
            page.error.set(Some(
                Text::new("error-competitions-unreadable").with_detail(error.to_string()),
            ));
            return;
        }
    };
    match Session::import(&text, name, &stored) {
        Ok(session) => store(page, session),
        Err(error) => page.error.set(Some(error.text())),
    }
}

/// Stores the competition of `session`, which has nothing saved yet, and opens it.
fn store(mut page: Page, mut session: Session) {
    let Some(storage) = page.storage.peek().clone() else {
        return;
    };
    spawn(async move {
        match session.save(&storage).await {
            Ok(()) => open(Some(session.key())),
            Err(error) => page.error.set(Some(
                Text::new("error-not-saved").with_detail(error.to_string()),
            )),
        }
    });
}

/// Has the browser download the file of the competition `key`, named `name`, as it is stored,
/// whether or not it still loads.
fn export(mut page: Page, key: CompetitionKey, name: String) {
    let Some(storage) = page.storage.peek().clone() else {
        return;
    };
    spawn(async move {
        match storage.read(key, 0).await {
            Ok(Some(stored)) => download(page, &name, &stored.export()),
            Ok(None) => page
                .error
                .set(Some(Text::new("error-competition-not-found"))),
            Err(error) => page.error.set(Some(
                Text::new("error-competition-unreadable").with_detail(error.to_string()),
            )),
        }
    });
}
