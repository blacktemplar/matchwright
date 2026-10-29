//! What is done after the start: reporting the results of the matches, round by round or those
//! that can be played now, and following the standings.
//!
//! A result can be typed in by the number of its match. After one is reported or reopened by its
//! buttons, which are gone then, the focus moves on to where the next one is reported, so that the
//! results of a round can be entered by keyboard alone.

use std::{num::NonZeroU32, rc::Rc};

use dioxus::prelude::*;
use tournament_app::{
    i18n::{Message as _, PluginText, Text},
    view::{MatchRow, Medal, RoundSummary},
};
use tournament_engine::{MatchId, MatchResult};
use wasm_bindgen::JsCast as _;

use crate::{
    announcement::Announcement, capitalized, competition::Ctx, i18n::t, icons, phase::answer,
    result_text, team_name, toggletip::Toggletip,
};

/// The list of matches shown.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shown {
    /// Those that can be played now, see
    /// [`Session::playable`](tournament_app::session::Session::playable).
    Playable,
    Round(NonZeroU32),
}

/// What gets the focus once the page shows a result reported or reopened by its buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    /// The first button reporting a result of the match.
    Report(MatchId),
    /// The heading of the matches shown, once none of them is left without a result.
    Heading,
}

/// Focuses the element mounted with the handler it returns whenever `focus` asks for `target`,
/// and then asks for nothing.
pub fn use_focus<T: Copy + PartialEq + 'static>(
    mut focus: Signal<Option<T>>,
    target: T,
) -> impl FnMut(MountedEvent) {
    let mut element = use_signal(|| None::<Rc<MountedData>>);
    use_effect(move || {
        if focus() == Some(target)
            && let Some(element) = element()
        {
            focus.set(None);
            if let Some(element) = element
                .downcast::<web_sys::Element>()
                .and_then(|element| element.dyn_ref::<web_sys::HtmlElement>())
            {
                focus_in_sight(element);
            }
        }
    });
    move |event: MountedEvent| element.set(Some(event.data()))
}

/// Focuses `element`, scrolling only as far as it takes to show it, in either direction, e.g. not
/// sideways in a bracket whose next match is in sight already.
fn focus_in_sight(element: &web_sys::HtmlElement) {
    let options = web_sys::FocusOptions::new();
    options.set_prevent_scroll(true);
    // fails only if the element cannot take the focus
    _ = element.focus_with_options(&options);
    let options = web_sys::ScrollIntoViewOptions::new();
    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
    options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
    element.scroll_into_view_with_scroll_into_view_options(&options);
}

/// Asks for `target` to get the focus, see [`use_focus`].
pub fn move_focus<T: 'static>(mut focus: Signal<Option<T>>, target: T) {
    focus.set(Some(target));
}

#[component]
pub fn Rounds() -> Element {
    let ctx = use_context::<Ctx>();
    // the list the operator chose, if any; a round that no longer exists, e.g. after an undo,
    // falls back to the one shown without a choice
    let mut chosen = use_signal(|| None::<Shown>);
    let focus = use_signal(|| None::<Focus>);
    let on_heading_mounted = use_focus(focus, Focus::Heading);
    // whether the entry of results typed in is shown
    let typing = use_signal(result_entry_open);
    // what is typed into it, which while there is any shows the matches it fits instead
    let entry = use_signal(String::new);
    let session = ctx.session.read();
    let rounds = session.rounds();
    let Some(current) = current(&rounds) else {
        return rsx! {};
    };
    let shown = chosen()
        .filter(|shown| match shown {
            Shown::Playable => true,
            Shown::Round(round) => rounds.iter().any(|summary| summary.round == *round),
        })
        .unwrap_or(Shown::Round(current));
    let playable = session.playable();
    let draws = session.round(current).is_some_and(|round| round.draws);
    let entered = entry.read().trim().to_owned();
    let entered = (typing() && !entered.is_empty()).then(|| {
        let read = session.read_entered(&entered);
        (entered, read.found, read.report.ok().map(|row| row.id))
    });
    let (matches, byes) = match (&entered, shown) {
        (Some((_, found, _)), _) => (found.clone(), Vec::new()),
        (None, Shown::Playable) => (playable.clone(), Vec::new()),
        (None, Shown::Round(round)) => session
            .round(round)
            .map(|round| (round.matches, round.byes))
            .unwrap_or_default(),
    };
    drop(session);
    // the match whose result Enter in the entry reports
    let to_report = entered.as_ref().and_then(|(_, _, to_report)| *to_report);
    let with_round = shown == Shown::Playable || entered.is_some();
    let with_stage = matches.iter().any(|row| row.stage.is_some());
    // after the result of a match is reported by its buttons, the focus moves on to the next match
    // shown without one, after it or else before it
    let open: Vec<_> = matches
        .iter()
        .filter(|row| row.result.is_none())
        .map(|row| row.id)
        .collect();
    let after_report = |id| {
        let position = open.iter().position(|open| *open == id).unwrap_or(0);
        open.get(position + 1..)
            .unwrap_or_default()
            .iter()
            .chain(&open[..position])
            .next()
            .map_or(Focus::Heading, |next| Focus::Report(*next))
    };
    let rows: Vec<_> = matches
        .into_iter()
        .map(|row| (after_report(row.id), row))
        .collect();
    // the rounds before and after the one shown, for the buttons stepping through them
    let (previous, next) = match shown {
        Shown::Playable => (None, None),
        Shown::Round(round) => {
            let position = rounds.iter().position(|summary| summary.round == round);
            let at = |offset: isize| {
                position
                    .and_then(|position| position.checked_add_signed(offset))
                    .and_then(|position| rounds.get(position))
                    .map(|summary| summary.round)
            };
            (at(-1), at(1))
        }
    };
    let tr = ctx.translator.read();
    rsx! {
        section { class: "card", aria_labelledby: "matches",
            div { class: "card-heading",
                h2 { id: "matches", {tr.get("matches")} }
                ResultEntryToggle { open: typing }
            }
            if typing() {
                ResultEntry { draws, entry }
            }
            // out of the way while the matches shown are those the entry fits, which they would not
            // change
            nav {
                class: "toolbar",
                aria_label: tr.get("rounds"),
                hidden: entered.is_some(),
                div { class: "segmented",
                    button {
                        aria_pressed: shown == Shown::Playable,
                        onclick: move |_| chosen.set(Some(Shown::Playable)),
                        {t!(tr, "playable-now-count", count = playable.len())}
                    }
                    button {
                        aria_pressed: shown != Shown::Playable,
                        onclick: move |_| chosen.set(Some(Shown::Round(current))),
                        {tr.get("by-round")}
                    }
                }
                if let Shown::Round(round) = shown {
                    div { class: "stepper",
                        button {
                            class: "small",
                            aria_label: tr.get("previous-round"),
                            // focusable at the first round as well, so that the focus stays
                            aria_disabled: previous.is_none().then_some("true"),
                            onclick: move |_| {
                                if let Some(previous) = previous {
                                    chosen.set(Some(Shown::Round(previous)));
                                }
                            },
                            icons::ChevronLeft {}
                        }
                        label { r#for: "round", class: "visually-hidden", {tr.get("rounds")} }
                        select {
                            id: "round",
                            onchange: move |event| {
                                if let Ok(round) = event.value().parse() {
                                    chosen.set(Some(Shown::Round(round)));
                                }
                            },
                            for summary in rounds {
                                option {
                                    key: "{summary.round}",
                                    value: "{summary.round}",
                                    selected: summary.round == round,
                                    if summary.open > 0 {
                                        {
                                            t!(
                                                tr,
                                                "round-with-open",
                                                round = summary.round,
                                                open = summary.open,
                                            )
                                        }
                                    } else {
                                        {t!(tr, "round", round = summary.round)}
                                    }
                                }
                            }
                        }
                        button {
                            class: "small",
                            aria_label: tr.get("next-round"),
                            aria_disabled: next.is_none().then_some("true"),
                            onclick: move |_| {
                                if let Some(next) = next {
                                    chosen.set(Some(Shown::Round(next)));
                                }
                            },
                            icons::ChevronRight {}
                        }
                    }
                }
            }
            // focused once no match shown is left without a result, so that the focus is not lost
            h3 { tabindex: "-1", onmounted: on_heading_mounted,
                match (&entered, shown) {
                    (Some((text, ..)), _) => t!(tr, "entry-found", text = text.as_str()),
                    (None, Shown::Playable) => tr.get("playable-now"),
                    (None, Shown::Round(round)) => t!(tr, "round", round = round),
                }
            }
            if rows.is_empty() && byes.is_empty() {
                if let Some((text, ..)) = &entered {
                    p { class: "empty", {t!(tr, "entry-nothing-found", text = text.as_str())} }
                } else if shown == Shown::Playable {
                    p { class: "empty", {tr.get("nothing-playable")} }
                }
            }
            if !rows.is_empty() {
                ol { id: "match-list",
                    for (after_report, row) in rows {
                        MatchLine {
                            key: "{row.id:?}",
                            with_round,
                            with_stage,
                            draws,
                            focus,
                            after_report,
                            to_report: to_report == Some(row.id),
                            row,
                        }
                    }
                }
            }
            for bye in byes {
                p { class: "bye",
                    {
                        let team = team_name(&tr, &bye.entrants);
                        match bye.stage {
                            Some(stage) => {
                                t!(tr, "bye-in-stage", team = team, stage = tr.stage(& stage))
                            }
                            None => t!(tr, "bye", team = team),
                        }
                    }
                }
            }
        }
    }
}

/// The round shown when the operator chose none: the first with a match without a result, or else
/// the last.
fn current(rounds: &[RoundSummary]) -> Option<NonZeroU32> {
    rounds
        .iter()
        .find(|summary| summary.open > 0)
        .or(rounds.last())
        .map(|summary| summary.round)
}

/// The button in the heading of the matches showing the entry of results typed in, or hiding it
/// again, which shows whether it is open; the browser remembers what the operator left it at.
#[component]
fn ResultEntryToggle(open: Signal<bool>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    rsx! {
        button {
            id: "result-entry-toggle",
            class: "quiet small",
            r#type: "button",
            aria_expanded: open(),
            onclick: move |_| {
                let opened = !open();
                open.set(opened);
                remember_result_entry_open(opened);
            },
            {tr.get("type-results")}
            icons::ChevronDown {}
        }
    }
}

/// Reports a result typed in as `entry`, the number of its match and the result, e.g. "3 1", or
/// part of the winner's name, saying while it is typed what it would report, or why nothing.
#[component]
fn ResultEntry(draws: bool, entry: Signal<String>) -> Element {
    let ctx = use_context::<Ctx>();
    // why what was entered last was not reported, until it is changed
    let mut error = use_signal(|| None::<Text>);
    // only reported by the tab that changes the competition
    let disabled = !ctx.changes();
    // the match and result reported if submitted now, or why none; nothing while nothing is typed
    let preview = (!entry.read().trim().is_empty())
        .then(|| ctx.session.read().read_entered(&entry.read()).report);
    let tr = ctx.translator.read();
    let preview = preview.map(|report| match report {
        Ok(row) => (
            "preview",
            t!(
                tr,
                "result-entry-preview",
                number = row.number,
                result = result_text(&tr, &row).unwrap_or_default(),
            ),
        ),
        Err(why) => ("preview hint", tr.sentence(&why.text())),
    });
    rsx! {
        form {
            class: "result-entry",
            onsubmit: move |event| {
                event.prevent_default();
                let entered = entry.peek().clone();
                match ctx.try_act(|session| session.report_entered(&entered)) {
                    Ok(reported) => {
                        ctx.announce(Announcement::Reported(reported));
                        entry.set(String::new());
                        error.set(None);
                    }
                    Err(refused) => error.set(Some(refused)),
                }
            },
            label { r#for: "result-entry", class: "visually-hidden", {tr.get("enter-result")} }
            input {
                id: "result-entry",
                value: "{entry}",
                autocomplete: "off",
                aria_invalid: error.read().is_some(),
                aria_describedby: "result-entry-error result-entry-preview result-entry-hint",
                disabled,
                oninput: move |event| {
                    entry.set(event.value());
                    error.set(None);
                },
            }
            " "
            button { r#type: "submit", disabled, {tr.get("report")} }
            p { id: "result-entry-error", class: "error", role: "alert",
                if let Some(error) = error() {
                    {tr.sentence(&error)}
                }
            }
            // said by the alert above instead once Enter was refused
            if let Some((class, text)) = preview.filter(|_| error.read().is_none()) {
                p { id: "result-entry-preview", class, {text} }
            }
            p { id: "result-entry-hint", class: "hint",
                {t!(tr, "enter-result-hint", draws = if draws { "yes" } else { "no" })}
            }
        }
    }
}

/// Where the browser remembers that the operator opened the entry of results.
const RESULT_ENTRY_OPEN: &str = "tournamo.result-entry-open";

/// Whether the operator left the entry of results open in this browser.
fn result_entry_open() -> bool {
    local_storage()
        .and_then(|storage| storage.get_item(RESULT_ENTRY_OPEN).ok().flatten())
        .is_some()
}

fn remember_result_entry_open(open: bool) {
    // without local storage, the entry is closed again on the next visit
    if let Some(storage) = local_storage() {
        _ = if open {
            storage.set_item(RESULT_ENTRY_OPEN, "true")
        } else {
            storage.remove_item(RESULT_ENTRY_OPEN)
        };
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[component]
fn MatchLine(
    row: MatchRow,
    with_round: bool,
    with_stage: bool,
    draws: bool,
    focus: Signal<Option<Focus>>,
    /// What gets the focus once a result of the match is reported by its buttons.
    after_report: Focus,
    /// Whether Enter in the entry of results reports its result, which marks it.
    to_report: bool,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let winner = |side| {
        let won = match side {
            Side::A => MatchResult::AWins,
            Side::B => MatchResult::BWins,
        };
        if row.result == Some(won) {
            "winner"
        } else {
            ""
        }
    };
    // one card: the pairing around the number, then each side's button below its name; once it
    // has a result, one line, the button reopening it after the pairing
    rsx! {
        li {
            class: "match",
            class: if row.result.is_some() { "finished" },
            class: if to_report { "to-report" },
            span { class: "visually-hidden", {tr.get("match")} }
            span { class: "number", "{row.number}" }
            span { class: "side a {winner(Side::A)}", {team_name(&tr, &row.a)} }
            span { class: "visually-hidden", {tr.get("versus")} }
            span { class: "side b {winner(Side::B)}", {team_name(&tr, &row.b)} }
            if with_round || with_stage {
                span { class: "meta",
                    if with_round {
                        span { class: "round", {t!(tr, "round", round = row.round)} }
                    }
                    if with_stage {
                        span { class: "stage", {row.stage.as_ref().map(|stage| tr.stage(stage))} }
                    }
                }
            }
            div { class: "report",
                if row.result.is_some() {
                    ReportedResult { row: row.clone(), draws, focus }
                } else {
                    ReportButtons {
                        row: row.clone(),
                        draws,
                        focus,
                        after_report,
                    }
                }
            }
        }
    }
}

/// The side of a match shown on the left, A, or on the right, B.
#[derive(Clone, Copy)]
enum Side {
    A,
    B,
}

/// The buttons reporting a result of the match `row`, which has none yet.
#[component]
fn ReportButtons(
    row: MatchRow,
    draws: bool,
    focus: Signal<Option<Focus>>,
    after_report: Focus,
) -> Element {
    let ctx = use_context::<Ctx>();
    let id = row.id;
    let on_mounted = use_focus(focus, Focus::Report(id));
    // only reported by the tab that changes the competition
    let disabled = !ctx.changes();
    let tr = ctx.translator.read();
    let (a, b) = (team_name(&tr, &row.a), team_name(&tr, &row.b));
    let report = move |result| {
        move |_| {
            if let Some(reported) = ctx.act(move |session| session.report(id, result)) {
                ctx.announce(Announcement::Reported(reported));
                move_focus(focus, after_report);
            }
        }
    };
    // each below its side's name, so that it names the side only for screen readers
    rsx! {
        button {
            class: "win a",
            disabled,
            aria_label: t!(tr, "wins", team = a.as_str()),
            onmounted: on_mounted,
            onclick: report(MatchResult::AWins),
            {tr.get("wins-short")}
        }
        if draws {
            button { class: "draw", disabled, onclick: report(MatchResult::Draw), {tr.get("draw")} }
        }
        button {
            class: "win b",
            disabled,
            aria_label: t!(tr, "wins", team = b.as_str()),
            onclick: report(MatchResult::BWins),
            {tr.get("wins-short")}
        }
    }
}

/// The result of the match `row`, of a round where a match may end in a draw if `draws`, and the
/// button reopening it.
#[component]
fn ReportedResult(row: MatchRow, draws: bool, focus: Signal<Option<Focus>>) -> Element {
    let ctx = use_context::<Ctx>();
    let id = row.id;
    // only reopened by the tab that changes the competition
    let disabled = !ctx.changes();
    let tr = ctx.translator.read();
    // a win is shown by the mark at the winner's name, so said only to screen readers, which may
    // not read the mark; a draw has no such mark
    let shown = row.result == Some(MatchResult::Draw);
    rsx! {
        span { class: if shown { "result" } else { "result visually-hidden" },
            {result_text(&tr, &row)}
        }
        // where a match may end in a draw, a win keeps the draw's room, so that the pairings of
        // every finished match line up
        if draws && !shown {
            span { class: "result-room", aria_hidden: "true", {tr.get("draw")} }
        }
        button {
            class: "small",
            disabled,
            aria_label: t!(tr, "reopen-match", number = row.number),
            onclick: move |_| {
                if let Some(reopened) = ctx.act(move |session| session.undo_report(id)) {
                    ctx.announce(Announcement::Reopened(reopened));
                    move_focus(focus, Focus::Report(id));
                }
            },
            {tr.get("reopen")}
        }
    }
}

/// The rank of a place with a medal, on a disc in the medal's colour, said to screen readers by the
/// medal's name before it.
#[component]
fn MedalOf(medal: Medal, rank: u32) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let (class, name) = match medal {
        Medal::Gold => ("gold", "medal-gold"),
        Medal::Silver => ("silver", "medal-silver"),
        Medal::Bronze => ("bronze", "medal-bronze"),
    };
    rsx! {
        span { class: "place medal {class}",
            span { class: "visually-hidden", {tr.get(name)} }
            "{rank}"
        }
    }
}

#[component]
pub fn Standings() -> Element {
    let ctx = use_context::<Ctx>();
    let Some(standings) = ctx.session.read().standings() else {
        return rsx! {};
    };
    // the settings of the calculators, e.g. the points for a win
    let form = ctx
        .session
        .read()
        .phase_form_to_edit()
        .ok()
        .map(|form| form.view());
    let tr = ctx.translator.read();
    let headers: Vec<_> = standings
        .columns
        .iter()
        .map(|column| {
            let header =
                capitalized(&tr.plugin(column.calculator, PluginText::Column, column.header));
            let label = capitalized(&tr.plugin(column.calculator, PluginText::Label, column.label));
            let description = column
                .description
                .map(|english| tr.plugin(column.calculator, PluginText::Description, english));
            // the metric's whole name where the header abbreviates it, e.g. "SB"
            let about = match description {
                Some(description) if label != header => Some(format!("{label}: {description}")),
                Some(description) => Some(description),
                None => (label != header).then_some(label),
            };
            let settings: Vec<_> = form
                .as_ref()
                .and_then(|form| form.fields_of(column.calculator))
                .unwrap_or_default()
                .iter()
                .map(|field| format!("{}: {}", tr.field_label(field), answer(&tr, &field.input)))
                .collect();
            let about = match about {
                _ if settings.is_empty() => about,
                Some(about) => Some(format!("{about} ({})", settings.join(", "))),
                None => Some(settings.join(", ")),
            };
            // ids of plugins are namespaced by `::`
            let id = format!("standings-about-{}", column.calculator.replace("::", "-"));
            (header, about, id)
        })
        .collect();
    rsx! {
        section { class: "card", aria_labelledby: "standings",
            h2 { id: "standings", {tr.get("standings")} }
            // scrolls sideways if its columns do not fit, the names staying in sight
            div {
                class: "scroll",
                role: "group",
                tabindex: "0",
                aria_labelledby: "standings",
                table { id: "standing-list",
                    thead {
                        tr {
                            th { scope: "col", {tr.get("rank")} }
                            th { class: "name", scope: "col", {tr.get("name")} }
                            th { scope: "col", {tr.get("played")} }
                            // named by the header alone, which the cells are read with, rather
                            // than with the toggletip's button too
                            for (header, about, id) in headers {
                                th { scope: "col", aria_label: "{header}",
                                    "{header}"
                                    if let Some(text) = about {
                                        " "
                                        Toggletip { id, subject: header.clone(), text }
                                    }
                                }
                            }
                        }
                    }
                    tbody {
                        for row in standings.rows {
                            // the places with a medal, once every match is played, stand out
                            tr {
                                class: if row.medal.is_some() { "podium" },
                                "data-rank": row.medal.map(|_| row.rank.get()),
                                td { class: "rank",
                                    if let Some(medal) = row.medal {
                                        MedalOf { medal, rank: row.rank.get() }
                                    } else {
                                        span { class: "place", "{row.rank}" }
                                    }
                                }
                                td { class: "name", {team_name(&tr, &row.entrants)} }
                                td { "{row.played}" }
                                for (column, cell) in standings.columns.iter().zip(row.cells) {
                                    td { {cell.map(|cell| tr.cell(column.calculator, &cell))} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
