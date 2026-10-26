//! What is done before the start: entering the entrants, adding the phase, starting.

use dioxus::prelude::*;
use tournament_app::session::Session;

use crate::{
    competition::Ctx,
    i18n::t,
    phase::{NewPhase, PhaseSummary},
    team_name,
};

#[component]
pub fn Entrants() -> Element {
    let ctx = use_context::<Ctx>();
    let mut name = use_signal(String::new);
    let entrants = ctx.session.read().entrants();
    let actions = ctx.actions();
    let tr = ctx.translator.read();
    let entrants: Vec<_> = entrants
        .into_iter()
        .map(|entrant| (entrant.id, team_name(&tr, &[entrant])))
        .collect();
    let count = entrants.len();
    rsx! {
        section { class: "card", aria_labelledby: "entrants",
            h2 { id: "entrants",
                {tr.get("entrants")}
                if count > 0 {
                    " "
                    span { class: "count", "({count})" }
                }
            }
            form {
                class: "row",
                onsubmit: move |event| {
                    event.prevent_default();
                    if ctx.act(|session| session.add_entrant(&name.peek())).is_some() {
                        name.set(String::new());
                    }
                },
                label { r#for: "entrant-name", class: "visually-hidden", {tr.get("name")} }
                input {
                    id: "entrant-name",
                    value: "{name}",
                    autocomplete: "off",
                    placeholder: tr.get("name"),
                    oninput: move |event| name.set(event.value()),
                }
                button {
                    class: "primary",
                    r#type: "submit",
                    disabled: !actions.add_entrant,
                    {tr.get("add-entrant")}
                }
            }
            if count == 0 {
                p { class: "empty", {tr.get("no-entrants")} }
            }
            ol { id: "entrant-list",
                for (id, name) in entrants {
                    li { key: "{id:?}",
                        span { class: "name", "{name}" }
                        button {
                            class: "quiet small",
                            aria_label: t!(tr, "remove-entrant", name = name.as_str()),
                            disabled: !actions.remove_entrant,
                            onclick: move |_| {
                                _ = ctx.act(|session| session.remove_entrant(id));
                            },
                            {tr.get("remove")}
                        }
                    }
                }
            }
        }
    }
}

/// The phase: its form until it is added, then how it is played; and the start.
#[component]
pub fn Phase() -> Element {
    let ctx = use_context::<Ctx>();
    let actions = ctx.actions();
    let has_phase = !ctx.session.read().competition().phases().is_empty();
    let tr = ctx.translator.read();
    rsx! {
        section { class: "card", aria_labelledby: "phase",
            h2 { id: "phase", {tr.get("phase-heading")} }
            if has_phase {
                PhaseSummary {}
            } else {
                NewPhase {}
            }
            button {
                class: "primary start",
                disabled: !actions.start,
                onclick: move |_| _ = ctx.act(Session::start),
                {tr.get("start")}
            }
        }
    }
}
