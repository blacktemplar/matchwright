//! What every screen tells first: why the last action was refused, and that the browser may not
//! keep what is stored.

use dioxus::prelude::*;
use tournament_app::i18n::Text;

use crate::{Page, install};

/// Where the browser remembers that the operator does not want to see the warning again.
const WARNING_DISMISSED: &str = "tournament-gui.storage-warning-dismissed";

/// Why the last action was refused, and the [warning](StorageWarning) while the browser may not
/// keep what is stored. `export` exports the competition shown, for the warning to offer.
#[component]
pub fn Notices(export: Option<EventHandler>) -> Element {
    let page = use_context::<Page>();
    let tr = page.translator.read();
    let error = page.error.read();
    let detail = error.as_ref().and_then(Text::detail).map(str::to_owned);
    rsx! {
        if *page.persisted.read() == Some(false) && !*page.warning_dismissed.read() {
            StorageWarning { export }
        }
        p { class: "error", role: "alert",
            if let Some(error) = error.as_ref() {
                {tr.sentence(error)}
            }
        }
        if let Some(detail) = detail {
            Detail { detail }
        }
    }
}

/// The [detail](Text::detail) of an error, folded away, for whoever looks into it. It is not part
/// of the alert, so it is not announced with the error.
#[component]
pub fn Detail(detail: String) -> Element {
    let page = use_context::<Page>();
    let tr = page.translator.read();
    rsx! {
        details { class: "error-detail",
            summary { {tr.get("error-detail")} }
            // as a library or the browser says it
            p { lang: "en", "{detail}" }
        }
    }
}

/// The browser refused to keep what is stored even when short of space, or it is a private window,
/// whose storage goes with it (which the page cannot tell apart). Nothing on the page changes the
/// browser's answer, so the warning offers what helps: exporting, installing the page as an app
/// (after which Chromium usually keeps it), and not showing it again.
#[component]
fn StorageWarning(export: Option<EventHandler>) -> Element {
    let mut page = use_context::<Page>();
    let tr = page.translator.read();
    rsx! {
        div { class: "warning", role: "status",
            p { {tr.get("not-persisted")} }
            if *page.installable.read() {
                p { {tr.get("not-persisted-install")} }
            }
            div { class: "warning-buttons",
                if let Some(export) = export {
                    button { onclick: move |_| export(()), {tr.get("export-this-competition")} }
                }
                if *page.installable.read() {
                    button {
                        onclick: move |_| {
                            // the offer is shown once; another one is announced
                            page.installable.set(false);
                            install::install();
                        },
                        {tr.get("install")}
                    }
                }
                button {
                    onclick: move |_| {
                        remember_dismissed();
                        page.warning_dismissed.set(true);
                    },
                    {tr.get("dont-show-again")}
                }
            }
        }
    }
}

/// Whether the operator chose not to see the warning again in this browser.
pub fn warning_dismissed() -> bool {
    local_storage()
        .and_then(|storage| storage.get_item(WARNING_DISMISSED).ok().flatten())
        .is_some()
}

fn remember_dismissed() {
    // without local storage, the warning is shown again on the next visit
    if let Some(storage) = local_storage() {
        _ = storage.set_item(WARNING_DISMISSED, "true");
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
