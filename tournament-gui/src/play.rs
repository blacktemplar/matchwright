//! What is done after the start: reporting the results of the matches, round by round, and
//! following the standings.

use std::num::NonZeroU32;

use dioxus::prelude::*;
use tournament_app::{
    i18n::PluginText,
    view::{MatchRow, RoundSummary},
};
use tournament_engine::MatchResult;

use crate::{competition::Ctx, i18n::t, team_name};

#[component]
pub fn Rounds() -> Element {
    let ctx = use_context::<Ctx>();
    // the round the operator chose, if any; a round that no longer exists, e.g. after an undo,
    // falls back to the one shown without a choice
    let mut chosen = use_signal(|| None::<NonZeroU32>);
    let session = ctx.session.read();
    let rounds = session.rounds();
    let Some(shown) = chosen()
        .filter(|round| rounds.iter().any(|summary| summary.round == *round))
        .or_else(|| current(&rounds))
    else {
        return rsx! {};
    };
    let round = session.round(shown);
    drop(session);
    let tr = ctx.translator.read();
    rsx! {
        section { aria_labelledby: "matches",
            h2 { id: "matches", {tr.get("matches")} }
            nav { aria_label: tr.get("rounds"),
                ul { class: "rounds",
                    for summary in rounds {
                        li { key: "{summary.round}",
                            button {
                                aria_pressed: summary.round == shown,
                                onclick: move |_| chosen.set(Some(summary.round)),
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
            if let Some(round) = round {
                h3 { {t!(tr, "round", round = round.round)} }
                if !round.matches.is_empty() {
                    table { id: "match-list",
                        thead {
                            tr {
                                th { scope: "col", {tr.get("match-number")} }
                                if round.matches.iter().any(|row| row.stage.is_some()) {
                                    th { scope: "col", {tr.get("stage")} }
                                }
                                th { scope: "col", {tr.get("side-a")} }
                                th { scope: "col", {tr.get("side-b")} }
                                th { scope: "col", {tr.get("result")} }
                            }
                        }
                        tbody {
                            for row in round.matches.iter().cloned() {
                                MatchLine {
                                    key: "{row.id:?}",
                                    with_stage: round.matches.iter().any(|row| row.stage.is_some()),
                                    row,
                                }
                            }
                        }
                    }
                }
                for bye in round.byes {
                    p { {t!(tr, "bye", team = team_name(&tr, &bye.entrants))} }
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

#[component]
fn MatchLine(row: MatchRow, with_stage: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let id = row.id;
    // only reported or reopened by the tab that changes the competition
    let disabled = !ctx.changes();
    let tr = ctx.translator.read();
    let (a, b) = (team_name(&tr, &row.a), team_name(&tr, &row.b));
    let report = move |result| move |_| _ = ctx.act(move |session| session.report(id, result));
    rsx! {
        tr {
            td { "{row.number}" }
            if with_stage {
                td { {row.stage.clone()} }
            }
            td { "{a}" }
            td { "{b}" }
            td {
                match row.result {
                    None => rsx! {
                        button { disabled, onclick: report(MatchResult::AWins),
                            {t!(tr, "wins", team = a.as_str())}
                        }
                        button { disabled, onclick: report(MatchResult::Draw), {tr.get("draw")} }
                        button { disabled, onclick: report(MatchResult::BWins),
                            {t!(tr, "wins", team = b.as_str())}
                        }
                    },
                    Some(result) => rsx! {
                        span { class: "result",
                            match result {
                                MatchResult::AWins => t!(tr, "won", team = a.as_str()),
                                MatchResult::BWins => t!(tr, "won", team = b.as_str()),
                                MatchResult::Draw => tr.get("draw"),
                            }
                        }
                        " "
                        button {
                            disabled,
                            aria_label: t!(tr, "reopen-match", number = row.number),
                            onclick: move |_| _ = ctx.act(move |session| session.undo_report(id)),
                            {tr.get("reopen")}
                        }
                    },
                }
            }
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
