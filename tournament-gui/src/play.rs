//! What is done after the start: reporting the results of the matches, round by round or those
//! that can be played now, and following the standings.
//!
//! A result can be typed in by the number of its match. After one is reported or reopened by its
//! buttons, which are gone then, the focus moves on to where the next one is reported, so that the
//! results of a round can be entered by keyboard alone.

use std::{num::NonZeroU32, rc::Rc};

use dioxus::prelude::*;
use tournament_app::{
    i18n::PluginText,
    view::{MatchRow, RoundSummary},
};
use tournament_engine::{MatchId, MatchResult};

use crate::{announcement::Announcement, competition::Ctx, i18n::t, result_text, team_name};

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
fn use_focus(mut focus: Signal<Option<Focus>>, target: Focus) -> impl FnMut(MountedEvent) {
    let mut element = use_signal(|| None::<Rc<MountedData>>);
    use_effect(move || {
        if focus() == Some(target)
            && let Some(element) = element()
        {
            focus.set(None);
            spawn(async move {
                // fails only if the element is gone already
                _ = element.set_focus(true).await;
            });
        }
    });
    move |event: MountedEvent| element.set(Some(event.data()))
}

/// Asks for `target` to get the focus, see [`use_focus`].
fn move_focus(mut focus: Signal<Option<Focus>>, target: Focus) {
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
    let (matches, byes) = match shown {
        Shown::Playable => (playable.clone(), Vec::new()),
        Shown::Round(round) => session
            .round(round)
            .map(|round| (round.matches, round.byes))
            .unwrap_or_default(),
    };
    drop(session);
    let with_round = shown == Shown::Playable;
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
    let tr = ctx.translator.read();
    rsx! {
        section { aria_labelledby: "matches",
            h2 { id: "matches", {tr.get("matches")} }
            ResultEntry { draws }
            nav { aria_label: tr.get("rounds"),
                ul { class: "rounds",
                    li {
                        button {
                            aria_pressed: shown == Shown::Playable,
                            onclick: move |_| chosen.set(Some(Shown::Playable)),
                            {t!(tr, "playable-now-count", count = playable.len())}
                        }
                    }
                    for summary in rounds {
                        li { key: "{summary.round}",
                            button {
                                aria_pressed: shown == Shown::Round(summary.round),
                                onclick: move |_| chosen.set(Some(Shown::Round(summary.round))),
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
                }
            }
            // focused once no match shown is left without a result, so that the focus is not lost
            h3 { tabindex: "-1", onmounted: on_heading_mounted,
                match shown {
                    Shown::Playable => tr.get("playable-now"),
                    Shown::Round(round) => t!(tr, "round", round = round),
                }
            }
            if rows.is_empty() {
                if shown == Shown::Playable {
                    p { {tr.get("nothing-playable")} }
                }
            } else {
                table { id: "match-list",
                    thead {
                        tr {
                            th { scope: "col", {tr.get("match-number")} }
                            if with_round {
                                th { scope: "col", {tr.get("round-column")} }
                            }
                            if with_stage {
                                th { scope: "col", {tr.get("stage")} }
                            }
                            th { scope: "col", {tr.get("side-a")} }
                            th { scope: "col", {tr.get("side-b")} }
                            th { scope: "col", {tr.get("result")} }
                        }
                    }
                    tbody {
                        for (after_report, row) in rows {
                            MatchLine {
                                key: "{row.id:?}",
                                with_round,
                                with_stage,
                                draws,
                                focus,
                                after_report,
                                row,
                            }
                        }
                    }
                }
            }
            for bye in byes {
                p {
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

/// Reports a result typed in as the number of its match and the result, e.g. "3 1".
#[component]
fn ResultEntry(draws: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let mut entry = use_signal(String::new);
    // only reported by the tab that changes the competition
    let disabled = !ctx.changes();
    let tr = ctx.translator.read();
    rsx! {
        form {
            class: "result-entry",
            onsubmit: move |event| {
                event.prevent_default();
                if let Some(reported) = ctx.act(|session| session.report_entered(&entry.peek())) {
                    ctx.announce(Announcement::Reported(reported));
                    entry.set(String::new());
                }
            },
            label { r#for: "result-entry", {tr.get("enter-result")} }
            " "
            input {
                id: "result-entry",
                value: "{entry}",
                autocomplete: "off",
                aria_describedby: "result-entry-hint",
                disabled,
                oninput: move |event| entry.set(event.value()),
            }
            " "
            button { r#type: "submit", disabled, {tr.get("report")} }
            p { id: "result-entry-hint", class: "hint",
                {t!(tr, "enter-result-hint", draws = if draws { "yes" } else { "no" })}
            }
        }
    }
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
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    rsx! {
        tr {
            td { "{row.number}" }
            if with_round {
                td { "{row.round}" }
            }
            if with_stage {
                td { {row.stage.as_ref().map(|stage| tr.stage(stage))} }
            }
            td { {team_name(&tr, &row.a)} }
            td { {team_name(&tr, &row.b)} }
            td {
                if row.result.is_some() {
                    ReportedResult { row: row.clone(), focus }
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
    rsx! {
        button {
            disabled,
            onmounted: on_mounted,
            onclick: report(MatchResult::AWins),
            {t!(tr, "wins", team = a.as_str())}
        }
        if draws {
            button { disabled, onclick: report(MatchResult::Draw), {tr.get("draw")} }
        }
        button { disabled, onclick: report(MatchResult::BWins),
            {t!(tr, "wins", team = b.as_str())}
        }
    }
}

/// The result of the match `row`, and the button reopening it.
#[component]
fn ReportedResult(row: MatchRow, focus: Signal<Option<Focus>>) -> Element {
    let ctx = use_context::<Ctx>();
    let id = row.id;
    // only reopened by the tab that changes the competition
    let disabled = !ctx.changes();
    let tr = ctx.translator.read();
    rsx! {
        span { class: "result", {result_text(&tr, &row)} }
        " "
        button {
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

#[component]
pub fn Standings() -> Element {
    let ctx = use_context::<Ctx>();
    let Some(standings) = ctx.session.read().standings() else {
        return rsx! {};
    };
    let tr = ctx.translator.read();
    rsx! {
        section { aria_labelledby: "standings",
            h2 { id: "standings", {tr.get("standings")} }
            table { id: "standing-list",
                thead {
                    tr {
                        th { scope: "col", {tr.get("rank")} }
                        th { scope: "col", {tr.get("name")} }
                        for column in standings.columns {
                            th { scope: "col",
                                {tr.plugin(column.calculator, PluginText::Column, column.header)}
                            }
                        }
                    }
                }
                tbody {
                    for row in standings.rows {
                        tr {
                            td { "{row.rank}" }
                            td { {team_name(&tr, &row.entrants)} }
                            for cell in row.cells {
                                td { {cell} }
                            }
                        }
                    }
                }
            }
        }
    }
}
