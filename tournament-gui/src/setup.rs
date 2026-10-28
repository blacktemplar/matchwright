//! What is done before the start: entering the entrants, adding the phase, starting.

use dioxus::prelude::*;
use tournament_app::{i18n::Message as _, session::Session};

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
    // an entrant whose name the one typed looks like, e.g. differing in case only
    let like = ctx.session.read().entrant_like(&name.read());
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
            if count == 0 {
                p { class: "empty", {tr.get("no-entrants")} }
            }
            ol { id: "entrant-list",
                for (id, name) in entrants {
                    li { key: "{id:?}",
                        span { class: "name", "{name}" }
                        button {
                            class: "small",
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
            // below the list, so that the entrant just added shows right above the field, which
            // keeps the focus for the next
            form {
                class: "row",
                onsubmit: move |event| {
                    event.prevent_default();
                    if ctx.act(|session| session.add_entrant(&name.peek())).is_some() {
                        name.set(String::new());
                        scroll_to_field();
                    }
                },
                label { r#for: "entrant-name", class: "visually-hidden", {tr.get("name")} }
                input {
                    id: "entrant-name",
                    value: "{name}",
                    autocomplete: "off",
                    placeholder: tr.get("name"),
                    aria_describedby: "entrant-like",
                    oninput: move |event| name.set(event.value()),
                }
                button {
                    class: "primary",
                    r#type: "submit",
                    disabled: !actions.add_entrant,
                    {tr.get("add-entrant")}
                }
                // said as it is typed, without refusing the name
                p { id: "entrant-like", class: "warnings", role: "status",
                    if let Some(like) = like {
                        {t!(tr, "entrant-like", name = like.as_str())}
                    }
                }
            }
        }
    }
}

/// Scrolls the field of the name into view, once the entrant just added is rendered above it, so
/// that the list growing with the page does not push it out of sight.
fn scroll_to_field() {
    document::eval(
        "requestAnimationFrame(() => document.getElementById('entrant-name')\
         ?.form.scrollIntoView({ block: 'nearest' }))",
    );
}

/// The phase: its form until it is added, then how it is played; and the start.
#[component]
pub fn Phase() -> Element {
    let ctx = use_context::<Ctx>();
    let actions = ctx.actions();
    let has_phase = !ctx.session.read().competition().phases().is_empty();
    // what keeps the competition from starting, said only to the tab that could start it
    let refusal = ctx
        .changes()
        .then(|| ctx.session.read().start_refusal())
        .flatten()
        .map(|refusal| refusal.text());
    let tr = ctx.translator.read();
    rsx! {
        section { class: "card", aria_labelledby: "phase",
            // focused once the phase is added, unless it can start
            h2 { id: "phase", tabindex: "-1", {tr.get("phase-heading")} }
            if has_phase {
                PhaseSummary {}
            } else {
                NewPhase {}
            }
            // apart from the phase above it, the card's primary action once the phase is added
            div { class: "start",
                button {
                    id: "start",
                    class: if has_phase { "primary" },
                    disabled: !actions.start,
                    aria_describedby: refusal.is_some().then_some("start-refusal"),
                    onclick: move |_| _ = ctx.act(Session::start),
                    {tr.get("start")}
                }
                if let Some(refusal) = &refusal {
                    " "
                    span { id: "start-refusal", class: "hint", {tr.sentence(refusal)} }
                }
            }
        }
    }
}
