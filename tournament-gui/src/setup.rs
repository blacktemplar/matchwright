//! What is done before the start: entering the entrants, adding the phase, starting.

use dioxus::prelude::*;
use tournament_app::session::Session;

use crate::{Ctx, team_name};

#[component]
pub fn Entrants() -> Element {
    let ctx = use_context::<Ctx>();
    let mut name = use_signal(String::new);
    let (entrants, actions) = {
        let session = ctx.session.read();
        (session.entrants(), session.actions())
    };
    rsx! {
        section { aria_labelledby: "entrants",
            h2 { id: "entrants", "Entrants" }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    if ctx.act(|session| session.add_entrant(&name.peek())) {
                        name.set(String::new());
                    }
                },
                label {
                    "Name "
                    input {
                        value: "{name}",
                        autocomplete: "off",
                        oninput: move |event| name.set(event.value()),
                    }
                }
                button { r#type: "submit", disabled: !actions.add_entrant, "Add entrant" }
            }
            ol { id: "entrant-list",
                for entrant in entrants {
                    li { key: "{entrant.id:?}",
                        span { class: "name", {team_name(std::slice::from_ref(&entrant))} }
                        " "
                        button {
                            aria_label: "Remove {team_name(std::slice::from_ref(&entrant))}",
                            disabled: !actions.remove_entrant,
                            onclick: move |_| {
                                _ = ctx.act(|session| session.remove_entrant(entrant.id));
                            },
                            "Remove"
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Phase() -> Element {
    let ctx = use_context::<Ctx>();
    let formats = Session::formats();
    // the id of the format chosen
    let mut format = use_signal(|| formats[0].id.to_owned());
    let actions = ctx.session.read().actions();
    rsx! {
        section { aria_labelledby: "phase",
            h2 { id: "phase", "Phase" }
            if actions.add_phase {
                label {
                    "Format "
                    select {
                        onchange: move |event| format.set(event.value()),
                        for option in formats {
                            option {
                                value: option.id,
                                selected: option.id == *format.read(),
                                "{option.label}"
                            }
                        }
                    }
                }
                button {
                    onclick: move |_| {
                        _ = ctx.act(|session| {
                            let draft = session
                                .phase_draft(&format.peek())
                                .map_err(|error| error.to_string())?;
                            session.add_phase(&draft).map_err(|error| error.to_string())
                        });
                    },
                    "Add phase"
                }
            } else {
                p { "The phase is added, played with the format's defaults." }
            }
            button {
                disabled: !actions.start,
                onclick: move |_| _ = ctx.act(Session::start),
                "Start"
            }
        }
    }
}
