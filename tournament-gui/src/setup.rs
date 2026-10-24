//! What is done before the start: entering the entrants, adding the phase, starting.

use dioxus::prelude::*;
use tournament_app::{i18n::PluginText, session::Session};

use crate::{competition::Ctx, i18n::t, team_name};

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
    rsx! {
        section { aria_labelledby: "entrants",
            h2 { id: "entrants", {tr.get("entrants")} }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    if ctx.act(|session| session.add_entrant(&name.peek())).is_some() {
                        name.set(String::new());
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
                button { r#type: "submit", disabled: !actions.add_entrant, {tr.get("add-entrant")} }
            }
            ol { id: "entrant-list",
                for (id, name) in entrants {
                    li { key: "{id:?}",
                        span { class: "name", "{name}" }
                        " "
                        button {
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

#[component]
pub fn Phase() -> Element {
    let ctx = use_context::<Ctx>();
    let formats = Session::formats();
    // the id of the format chosen
    let mut format = use_signal(|| formats[0].id.to_owned());
    let actions = ctx.actions();
    let has_phase = !ctx.session.read().competition().phases().is_empty();
    let tr = ctx.translator.read();
    rsx! {
        section { aria_labelledby: "phase",
            h2 { id: "phase", {tr.get("phase")} }
            if !has_phase {
                label {
                    {tr.get("format")}
                    " "
                    select {
                        id: "format",
                        disabled: !actions.add_phase,
                        onchange: move |event| format.set(event.value()),
                        for option in formats {
                            option {
                                value: option.id,
                                selected: option.id == *format.read(),
                                {tr.plugin(option.id, PluginText::Label, option.label)}
                            }
                        }
                    }
                }
                button {
                    disabled: !actions.add_phase,
                    onclick: move |_| {
                        let draft = ctx.act(|session| session.phase_draft(&format.peek()));
                        if let Some(draft) = draft {
                            _ = ctx.act(|session| session.add_phase(&draft));
                        }
                    },
                    {tr.get("add-phase")}
                }
            } else {
                p { {tr.get("phase-added")} }
            }
            button {
                disabled: !actions.start,
                onclick: move |_| _ = ctx.act(Session::start),
                {tr.get("start")}
            }
        }
    }
}
