use leptos::{ev::SubmitEvent, prelude::*};
use spike_core::{MatchResult, MatchRow, StandingRow, Tournament};

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// The state every component shares. Signals are `Copy` handles, so this is too.
#[derive(Clone, Copy)]
struct Ctx {
    /// `LocalStorage`: the engine's plugins are not `Send`, and the browser has one thread anyway.
    tournament: RwSignal<Tournament, LocalStorage>,
    error: RwSignal<Option<String>>,
}

impl Ctx {
    /// Changes the tournament, showing why if it refuses.
    fn act(self, change: impl FnOnce(&mut Tournament) -> Result<(), String>) {
        let result = self
            .tournament
            .try_update(change)
            .expect("the tournament lives as long as the page");
        self.error.set(result.err());
    }
}

#[component]
fn App() -> impl IntoView {
    let ctx = Ctx {
        tournament: RwSignal::new_local(Tournament::new()),
        error: RwSignal::new(None),
    };
    provide_context(ctx);
    let started = Memo::new(move |_| ctx.tournament.with(Tournament::is_started));
    view! {
        <header>
            <h1>"Tournament (Leptos)"</h1>
            <button on:click=move |_| ctx.act(Tournament::undo)>"Undo"</button>
            <button on:click=move |_| ctx.act(Tournament::redo)>"Redo"</button>
        </header>
        <p class="error" role="alert">{move || ctx.error.get()}</p>
        <Show when=move || started.get() fallback=Setup>
            <Matches />
            <StandingsTable />
        </Show>
    }
}

#[component]
fn Setup() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let name = RwSignal::new(String::new());
    let entrants = Memo::new(move |_| ctx.tournament.with(Tournament::entrants));
    let has_phase = Memo::new(move |_| ctx.tournament.with(Tournament::has_phase));
    let formats = Tournament::formats();
    let format = RwSignal::new(formats[0].id.to_owned());
    let add = move |event: SubmitEvent| {
        event.prevent_default();
        ctx.act(|tournament| tournament.add_entrant(&name.get_untracked()));
        if ctx.error.get_untracked().is_none() {
            name.set(String::new());
        }
    };
    view! {
        <section>
            <h2>"Entrants"</h2>
            <form on:submit=add>
                <label>"Name " <input bind:value=name /></label>
                <button type="submit">"Add"</button>
            </form>
            <button on:click=move |_| {
                ctx.act(Tournament::add_demo_entrants)
            }>"Add 8 demo entrants"</button>
            <button on:click=move |_| {
                ctx.act(|tournament| {
                    *tournament = Tournament::large(1000, 2500);
                    Ok(())
                })
            }>"Load 1000 entrants, 2500 reported"</button>
            <ol>
                <For each=move || entrants.get() key=String::clone let:name>
                    <li>{name}</li>
                </For>
            </ol>
        </section>
        <section>
            <h2>"Phase"</h2>
            <label>
                "Format "
                <select
                    prop:value=move || format.get()
                    on:change:target=move |event| format.set(event.target().value())
                >
                    {formats
                        .into_iter()
                        .map(|format| view! { <option value=format.id>{format.label}</option> })
                        .collect_view()}
                </select>
            </label>
            <button
                disabled=move || has_phase.get()
                on:click=move |_| {
                    ctx.act(|tournament| tournament.add_phase(&format.get_untracked()))
                }
            >
                "Add phase"
            </button>
            <button
                disabled=move || !has_phase.get()
                on:click=move |_| ctx.act(Tournament::start)
            >
                "Start"
            </button>
        </section>
    }
}

#[component]
fn Matches() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let open_only = RwSignal::new(false);
    let matches = Memo::new(move |_| ctx.tournament.with(Tournament::matches));
    let shown = Memo::new(move |_| {
        let open_only = open_only.get();
        matches
            .get()
            .into_iter()
            .filter(|row| !open_only || row.result.is_none())
            .collect::<Vec<_>>()
    });
    view! {
        <section>
            <h2>"Matches"</h2>
            <label>
                <input type="checkbox" bind:checked=open_only />
                " open only"
            </label>
            <table>
                <thead>
                    <tr>
                        <th>"#"</th>
                        <th>"Round"</th>
                        <th>"Stage"</th>
                        <th>"A"</th>
                        <th>"B"</th>
                        <th>"Result"</th>
                    </tr>
                </thead>
                <tbody>
                    // keyed by the whole row, so a line is rebuilt exactly when its row changes.
                    // Keying by position with a memo per line, which updates single cells, measured
                    // slower: 5000 memos cloning and comparing their row on every change.
                    <For each=move || shown.get() key=MatchRow::clone let:row>
                        <MatchLine row />
                    </For>
                </tbody>
            </table>
        </section>
    }
}

#[component]
fn MatchLine(row: MatchRow) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let id = row.id;
    let report = move |result| move |_| ctx.act(move |tournament| tournament.report(id, result));
    let result = match row.result {
        None => view! {
            <button on:click=report(MatchResult::AWins)>"A wins"</button>
            <button on:click=report(MatchResult::Draw)>"Draw"</button>
            <button on:click=report(MatchResult::BWins)>"B wins"</button>
        }
        .into_any(),
        Some(result) => view! {
            <span>{describe(result, &row.a, &row.b)}</span>
            " "
            <button on:click=move |_| {
                ctx.act(move |tournament| tournament.undo_report(id))
            }>"Reopen"</button>
        }
        .into_any(),
    };
    view! {
        <tr>
            <td>{row.number}</td>
            <td>{row.round}</td>
            <td>{row.stage}</td>
            <td>{row.a}</td>
            <td>{row.b}</td>
            <td>{result}</td>
        </tr>
    }
}

#[component]
fn StandingsTable() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let standings = Memo::new(move |_| ctx.tournament.with(Tournament::standings));
    view! {
        <section>
            <h2>"Standings"</h2>
            <table>
                <thead>
                    <tr>
                        <th>"#"</th>
                        <th>"Name"</th>
                        {move || {
                            standings
                                .get()
                                .columns
                                .into_iter()
                                .map(|header| view! { <th>{header}</th> })
                                .collect_view()
                        }}
                    </tr>
                </thead>
                <tbody>
                    <For each=move || standings.get().rows key=StandingRow::clone let:row>
                        <tr>
                            <td>{row.rank}</td>
                            <td>{row.name}</td>
                            {row
                                .cells
                                .into_iter()
                                .map(|cell| view! { <td>{cell}</td> })
                                .collect_view()}
                        </tr>
                    </For>
                </tbody>
            </table>
        </section>
    }
}

fn describe(result: MatchResult, a: &str, b: &str) -> String {
    match result {
        MatchResult::AWins => format!("{a} won"),
        MatchResult::BWins => format!("{b} won"),
        MatchResult::Draw => "draw".to_owned(),
    }
}
