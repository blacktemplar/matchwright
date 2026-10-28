//! The help, at `#help`: that the competitions are kept in the browser and nowhere else, what
//! follows from that, and where the source code is.

use dioxus::prelude::*;

use crate::{Page, REPOSITORY, icons};

#[component]
pub fn Help() -> Element {
    let page = use_context::<Page>();
    let tr = page.translator.read();
    rsx! {
        main { class: "narrow help",
            section { class: "card", aria_labelledby: "help-stored",
                h2 { id: "help-stored", {tr.get("help-stored")} }
                p { {tr.get("help-stored-here")} }
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
            }
        }
    }
}
