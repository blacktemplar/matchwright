//! The cross table of a phase whose teams stay the same, e.g. a round robin: a row and a column
//! per team, each cell the matches of the row's team against the column's, seen from the row.
//!
//! A match without a result shows its number as a button, which opens a dialog reporting it; the
//! focus then moves on to the next match without a result, as in the list of matches.

use dioxus::prelude::*;
use tournament_app::view::{CrossMatch, MatchRow, Outcome};
use tournament_engine::{MatchId, MatchResult};

use crate::{
    announcement::Announcement,
    competition::Ctx,
    components::dialog::{Dialog, DialogDescription, DialogTitle},
    i18n::t,
    play::{move_focus, use_focus},
    team_name,
};

/// What gets the focus once the page shows a result reported in the table.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    /// The button of the match in the row of the team of it that comes first in the table.
    Report(MatchId),
    /// The heading of the table, once no match is left without a result.
    Heading,
    /// The button of the match in the row of the other team, which is never focused.
    Mirrored(MatchId),
}

/// The cross table, if the phase has one.
#[component]
pub fn CrossTable() -> Element {
    let ctx = use_context::<Ctx>();
    let focus = use_signal(|| None::<Focus>);
    let on_heading_mounted = use_focus(focus, Focus::Heading);
    // the match whose result the dialog reports, while it is open
    let reporting = use_signal(|| None::<MatchRow>);
    let Some(table) = ctx.session.read().cross_table() else {
        return rsx! {};
    };
    let tr = ctx.translator.read();
    // only reported by the tab that changes the competition
    let reports = ctx.changes();
    let names: Vec<String> = table
        .rows
        .iter()
        .map(|row| team_name(&tr, &row.entrants))
        .collect();
    // the matches without a result in the order of the table, each where it comes first
    let mut open: Vec<MatchId> = Vec::new();
    for played in table.rows.iter().flat_map(|row| row.cells.iter().flatten()) {
        if played.outcome.is_none() && !open.contains(&played.row.id) {
            open.push(played.row.id);
        }
    }
    rsx! {
        section { class: "card", aria_labelledby: "cross-table",
            // focused once no match is left without a result, so that the focus is not lost
            h2 { id: "cross-table", tabindex: "-1", onmounted: on_heading_mounted,
                {tr.get("cross-table")}
            }
            if reports && !open.is_empty() {
                p { class: "hint", {tr.get("cross-table-report-hint")} }
            }
            // scrolls sideways if its columns do not fit, the names staying in sight
            div {
                class: "scroll",
                role: "group",
                tabindex: "0",
                aria_labelledby: "cross-table",
                table { id: "cross-table-grid", class: "cross-table",
                    thead {
                        tr {
                            th { class: "name", scope: "col", {tr.get("name")} }
                            // the number of the row, its name said to screen readers
                            for (index , name) in names.iter().enumerate() {
                                th { scope: "col",
                                    span { aria_hidden: "true", "{index + 1}" }
                                    span { class: "visually-hidden", "{name}" }
                                }
                            }
                            th { scope: "col", {tr.get("rank")} }
                        }
                    }
                    tbody {
                        for (i , row) in table.rows.into_iter().enumerate() {
                            tr { key: "{i}",
                                th { class: "name", scope: "row",
                                    span { class: "number", aria_hidden: "true", "{i + 1}" }
                                    " "
                                    "{names[i]}"
                                }
                                for (j , cell) in row.cells.into_iter().enumerate() {
                                    if i == j {
                                        td { key: "{j}", class: "self" }
                                    } else {
                                        td { key: "{j}",
                                            for played in cell {
                                                Cell {
                                                    key: "{played.row.id:?}",
                                                    // the one of the two cells of a match
                                                    // that comes first in the table
                                                    first: i < j,
                                                    played,
                                                    reports,
                                                    focus,
                                                    reporting,
                                                }
                                            }
                                        }
                                    }
                                }
                                td { "{row.rank}" }
                            }
                        }
                    }
                }
            }
            ReportDialog {
                reporting,
                draws: table.draws,
                focus,
                open,
            }
        }
    }
}

/// A match in a cell: how it ended for the row's team, or else its number, a button opening the
/// dialog reporting it for the tab that reports.
#[component]
fn Cell(
    played: CrossMatch,
    first: bool,
    reports: bool,
    focus: Signal<Option<Focus>>,
    reporting: Signal<Option<MatchRow>>,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let row = played.row;
    // the focus moves to the first of the two buttons of a match only
    let target = if first {
        Focus::Report(row.id)
    } else {
        Focus::Mirrored(row.id)
    };
    let on_mounted = use_focus(focus, target);
    let (shown, said) = match played.outcome {
        Some(Outcome::Win) => ("1", "outcome-win"),
        Some(Outcome::Draw) => ("½", "outcome-draw"),
        Some(Outcome::Loss) => ("0", "outcome-loss"),
        None if reports => {
            return rsx! {
                button {
                    class: "small",
                    aria_label: t!(tr, "report-match", number = row.number),
                    onmounted: on_mounted,
                    onclick: move |_| reporting.set(Some(row.clone())),
                    "{row.number}"
                }
            };
        }
        None => {
            return rsx! {
                span { class: "open",
                    span { aria_hidden: "true", "{row.number}" }
                    span { class: "visually-hidden", {t!(tr, "match-open", number = row.number)} }
                }
            };
        }
    };
    rsx! {
        span { class: "outcome",
            span { aria_hidden: "true", "{shown}" }
            span { class: "visually-hidden", {tr.get(said)} }
        }
    }
}

/// The dialog reporting the result of the match `reporting` holds, while it holds one.
#[component]
fn ReportDialog(
    reporting: Signal<Option<MatchRow>>,
    draws: bool,
    focus: Signal<Option<Focus>>,
    /// The matches without a result in the order of the table.
    open: Vec<MatchId>,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let row = reporting();
    let shown = row.is_some();
    let Some(row) = row else {
        return rsx! {
            Dialog { open: Some(false), on_open_change: move |_| {} }
        };
    };
    let id = row.id;
    // the next match without a result after this one, else the first before it
    let next = {
        let position = open.iter().position(|open| *open == id).unwrap_or(0);
        open.get(position + 1..)
            .unwrap_or_default()
            .iter()
            .chain(&open[..position])
            .next()
            .map_or(Focus::Heading, |next| Focus::Report(*next))
    };
    let report = move |result| {
        move |_| {
            reporting.set(None);
            if let Some(reported) = ctx.act(move |session| session.report(id, result)) {
                ctx.announce(Announcement::Reported(reported));
                move_focus(focus, next);
            }
        }
    };
    let (a, b) = (team_name(&tr, &row.a), team_name(&tr, &row.b));
    rsx! {
        Dialog {
            open: Some(shown),
            on_open_change: move |open: bool| {
                if !open {
                    reporting.set(None);
                }
            },
            DialogTitle { {t!(tr, "match-numbered", number = row.number)} }
            DialogDescription { {t!(tr, "match-between", a = a.as_str(), b = b.as_str())} }
            div { class: "dialog-buttons",
                button { onclick: report(MatchResult::AWins), {t!(tr, "wins", team = a.as_str())} }
                if draws {
                    button { onclick: report(MatchResult::Draw), {tr.get("draw")} }
                }
                button { onclick: report(MatchResult::BWins), {t!(tr, "wins", team = b.as_str())} }
                button { onclick: move |_| reporting.set(None), {tr.get("cancel")} }
            }
        }
    }
}
