//! The help, at `#help`: that the competitions are kept in the browser and nowhere else, what
//! follows from that, where the source code is, and who owns the page.

use dioxus::prelude::*;

use crate::{Page, REPOSITORY, i18n::t, icons};

/// The media owner the help names, name and town (e.g. `Jane Doe, Vienna`), from the environment
/// variable `MATCHWRIGHT_MEDIA_OWNER` at build time; without it (or with it empty) the help names
/// none.
const MEDIA_OWNER: Option<&str> = option_env!("MATCHWRIGHT_MEDIA_OWNER");

#[component]
pub fn Help() -> Element {
    let page = use_context::<Page>();
    let tr = page.translator.read();
    rsx! {
        main { class: "narrow help",
            section { class: "card", aria_labelledby: "help-stored",
                h2 { id: "help-stored", {tr.get("help-stored")} }
                p { {tr.get("help-stored-here")} }
                p { {tr.get("help-stored-hosting")} }
                p { {tr.get("help-stored-lost")} }
                p { {tr.get("help-stored-export")} }
            }
            section { class: "card", aria_labelledby: "help-offline",
                h2 { id: "help-offline", {tr.get("help-offline")} }
                p { {tr.get("help-offline-text")} }
                p { {tr.get("help-tabs")} }
            }
            section { class: "card", aria_labelledby: "help-open-source",
                h2 { id: "help-open-source", {tr.get("help-open-source")} }
                p { {tr.get("help-open-source-text")} }
                p {
                    a { href: REPOSITORY,
                        icons::GitHub {}
                        {tr.get("source-code")}
                    }
                }
                if let Some(owner) = MEDIA_OWNER.filter(|owner| !owner.is_empty()) {
                    p { {t!(tr, "help-media-owner", owner = owner)} }
                }
            }
        }
    }
}
