use dioxus::prelude::*;
use spike_core::{MatchResult, MatchRow, Tournament};

fn main() {
    console_error_panic_hook::set_once();
    dioxus::launch(App);
}

/// The state every component shares. Signals are `Copy` handles, so this is too.
#[derive(Clone, Copy)]
struct Ctx {
    tournament: Signal<Tournament>,
    error: Signal<Option<String>>,
}

impl Ctx {
    /// Changes the tournament, showing why if it refuses.
    fn act(mut self, change: impl FnOnce(&mut Tournament) -> Result<(), String>) {
        let result = change(&mut self.tournament.write());
        self.error.set(result.err());
    }
}

#[component]
fn App() -> Element {
    let ctx = use_context_provider(|| Ctx {
        tournament: Signal::new(Tournament::new()),
        error: Signal::new(None),
    });
    let started = ctx.tournament.read().is_started();
    rsx! {
        header {
            h1 { "Tournament (Dioxus)" }
            button { onclick: move |_| ctx.act(Tournament::undo), "Undo" }
            button { onclick: move |_| ctx.act(Tournament::redo), "Redo" }
        }
        p { class: "error", role: "alert", {ctx.error.cloned()} }
        if started {
            Matches {}
            Standings {}
        } else {
            Setup {}
        }
    }
}

#[component]
fn Setup() -> Element {
    let ctx = use_context::<Ctx>();
    let mut name = use_signal(String::new);
    let mut format = use_signal(|| Tournament::formats()[0].id.to_owned());
    let (entrants, has_phase) = {
        let tournament = ctx.tournament.read();
        (tournament.entrants(), tournament.has_phase())
    };
    rsx! {
        section {
            h2 { "Entrants" }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    ctx.act(|tournament| tournament.add_entrant(&name.peek()));
                    if ctx.error.peek().is_none() {
                        name.set(String::new());
                    }
                },
                label {
                    "Name "
                    input { value: "{name}", oninput: move |event| name.set(event.value()) }
                }
                button { r#type: "submit", "Add" }
            }
            button {
                onclick: move |_| ctx.act(Tournament::add_demo_entrants),
                "Add 8 demo entrants"
            }
            button {
                onclick: move |_| {
                    ctx.act(|tournament| {
                        *tournament = Tournament::large(1000, 2500);
                        Ok(())
                    })
                },
                "Load 1000 entrants, 2500 reported"
            }
            ol {
                for entrant in entrants {
                    li { key: "{entrant}", "{entrant}" }
                }
            }
        }
        section {
            h2 { "Phase" }
            label {
                "Format "
                select {
                    value: "{format}",
                    onchange: move |event| format.set(event.value()),
                    for option in Tournament::formats() {
                        option { value: option.id, "{option.label}" }
                    }
                }
            }
            button {
                disabled: has_phase,
                onclick: move |_| ctx.act(|tournament| tournament.add_phase(&format.peek())),
                "Add phase"
            }
            button {
                disabled: !has_phase,
                onclick: move |_| ctx.act(Tournament::start),
                "Start"
            }
        }
    }
}

#[component]
fn Matches() -> Element {
    let ctx = use_context::<Ctx>();
    let mut open_only = use_signal(|| false);
    let rows: Vec<_> = ctx
        .tournament
        .read()
        .matches()
        .into_iter()
        .filter(|row| !open_only() || row.result.is_none())
        .collect();
    rsx! {
        section {
            h2 { "Matches" }
            label {
                input {
                    r#type: "checkbox",
                    checked: open_only(),
                    onchange: move |event| open_only.set(event.checked()),
                }
                " open only"
            }
            table {
                thead {
                    tr {
                        th { "#" }
                        th { "Round" }
                        th { "Stage" }
                        th { "A" }
                        th { "B" }
                        th { "Result" }
                    }
                }
                tbody {
                    for row in rows {
                        MatchLine { key: "{row.number}", row }
                    }
                }
            }
        }
    }
}

#[component]
fn MatchLine(row: MatchRow) -> Element {
    let ctx = use_context::<Ctx>();
    let id = row.id;
    let report = move |result| move |_| ctx.act(move |tournament| tournament.report(id, result));
    rsx! {
        tr {
            td { "{row.number}" }
            td { "{row.round}" }
            td { {row.stage.clone()} }
            td { "{row.a}" }
            td { "{row.b}" }
            td {
                match row.result {
                    None => rsx! {
                        button { onclick: report(MatchResult::AWins), "A wins" }
                        button { onclick: report(MatchResult::Draw), "Draw" }
                        button { onclick: report(MatchResult::BWins), "B wins" }
                    },
                    Some(result) => rsx! {
                        span { {describe(result, &row.a, &row.b)} }
                        " "
                        button {
                            onclick: move |_| ctx.act(move |tournament| tournament.undo_report(id)),
                            "Reopen"
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn Standings() -> Element {
    let ctx = use_context::<Ctx>();
    let standings = ctx.tournament.read().standings();
    rsx! {
        section {
            h2 { "Standings" }
            table {
                thead {
                    tr {
                        th { "#" }
                        th { "Name" }
                        for header in standings.columns {
                            th { "{header}" }
                        }
                    }
                }
                tbody {
                    for row in standings.rows {
                        tr {
                            td { "{row.rank}" }
                            td { "{row.name}" }
                            for cell in row.cells {
                                td { "{cell}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn describe(result: MatchResult, a: &str, b: &str) -> String {
    match result {
        MatchResult::AWins => format!("{a} won"),
        MatchResult::BWins => format!("{b} won"),
        MatchResult::Draw => "draw".to_owned(),
    }
}
