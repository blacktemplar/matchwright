//! The list of the competitions stored in the browser, to open one, export it, or start another:
//! a new one or one imported from a file.

use dioxus::{html::FileData, prelude::*};
use futures_util::StreamExt as _;
use tournament_app::{
    i18n::{Message as _, Text},
    session::Session,
    storage::{CompetitionKey, Storage as _},
};
use tournament_browser::channel::Broadcast;

use crate::{Notices, Page, competition::SAVED, download, i18n::t, open};

#[component]
pub fn Competitions() -> Element {
    let page = use_context::<Page>();
    let mut name = use_signal(String::new);
    // counts what other tabs saved, which may be a competition the list does not have yet
    let mut saved_elsewhere = use_signal(|| 0_u32);
    use_future(move || async move {
        let channel = Broadcast::new(SAVED);
        let mut saved = channel.messages();
        while saved.next().await.is_some() {
            saved_elsewhere += 1;
        }
    });
    let listings = use_resource(move || async move {
        saved_elsewhere();
        let storage = page
            .storage
            .peek()
            .clone()
            .expect("the list is shown once the database is open");
        storage.list().await.map_err(|error| {
            Text::new("error-competitions-unreadable").with("reason", error.to_string())
        })
    });
    let tr = page.translator.read();
    let (listings, unreadable) = match &*listings.read() {
        Some(Ok(listings)) => (listings.clone(), None),
        Some(Err(why)) => (Vec::new(), Some(tr.text(why))),
        None => (Vec::new(), None),
    };
    rsx! {
        main {
            Notices {}
            section { aria_labelledby: "competitions",
                h2 { id: "competitions", {tr.get("competitions")} }
                if let Some(why) = unreadable {
                    p { class: "error", "{why}" }
                } else if listings.is_empty() {
                    p { {tr.get("no-competitions")} }
                }
                ul { id: "competition-list",
                    for listing in listings {
                        li { key: "{listing.key}",
                            a { href: "#{listing.key}", "{listing.name}" }
                            " "
                            button {
                                aria_label: t!(tr, "export-competition", name = &*listing.name),
                                onclick: move |_| export(page, listing.key, listing.name.clone()),
                                {tr.get("export")}
                            }
                        }
                    }
                }
            }
            section { aria_labelledby: "new-competition",
                h2 { id: "new-competition", {tr.get("new-competition")} }
                form {
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
                    label {
                        {tr.get("name")}
                        " "
                        input {
                            value: "{name}",
                            autocomplete: "off",
                            oninput: move |event| name.set(event.value()),
                        }
                    }
                    button { r#type: "submit", {tr.get("create")} }
                }
                label {
                    {tr.get("import")}
                    " "
                    input {
                        id: "import",
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
        }
    }
}

/// Imports the competition of `file`, named after the file unless it says its name, and opens it.
async fn import(mut page: Page, file: FileData) {
    let text = match file.read_string().await {
        Ok(text) => text,
        Err(error) => {
            page.error.set(Some(
                Text::new("error-file-not-read").with("reason", error.to_string()),
            ));
            return;
        }
    };
    let name = file.name();
    let name = name.strip_suffix(".json").unwrap_or(&name);
    match Session::import(&text, name) {
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
                Text::new("error-not-saved").with("reason", error.to_string()),
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
                Text::new("error-competition-unreadable").with("reason", error.to_string()),
            )),
        }
    });
}
