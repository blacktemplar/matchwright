//! What is done after the start: reporting the results of the matches, round by round, and
//! following the standings.

use std::num::NonZeroU32;

use dioxus::prelude::*;
use tournament_app::view::{MatchRow, RoundSummary};
use tournament_engine::MatchResult;

use crate::{Ctx, team_name};

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
    rsx! {
        section { aria_labelledby: "matches",
            h2 { id: "matches", "Matches" }
            nav { aria_label: "Rounds",
                ul { class: "rounds",
                    for summary in rounds {
                        li { key: "{summary.round}",
                            button {
                                aria_pressed: summary.round == shown,
                                onclick: move |_| chosen.set(Some(summary.round)),
                                "Round {summary.round}"
                                if summary.open > 0 {
                                    " ({summary.open} open)"
                                }
                            }
                        }
                    }
                }
            }
            if let Some(round) = round {
                h3 { "Round {round.round}" }
                if !round.matches.is_empty() {
                    table { id: "match-list",
                        thead {
                            tr {
                                th { scope: "col", "#" }
                                if round.matches.iter().any(|row| row.stage.is_some()) {
                                    th { scope: "col", "Stage" }
                                }
                                th { scope: "col", "Side A" }
                                th { scope: "col", "Side B" }
                                th { scope: "col", "Result" }
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
                    p { "Bye: {team_name(&bye.entrants)}" }
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
    let (a, b) = (team_name(&row.a), team_name(&row.b));
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
                        button { onclick: report(MatchResult::AWins), "{a} wins" }
                        button { onclick: report(MatchResult::Draw), "Draw" }
                        button { onclick: report(MatchResult::BWins), "{b} wins" }
                    },
                    Some(result) => rsx! {
                        span { class: "result",
                            match result {
                                MatchResult::AWins => format!("{a} won"),
                                MatchResult::BWins => format!("{b} won"),
                                MatchResult::Draw => "Draw".to_owned(),
                            }
                        }
                        " "
                        button {
                            aria_label: "Reopen match {row.number}",
                            onclick: move |_| _ = ctx.act(move |session| session.undo_report(id)),
                            "Reopen"
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
    rsx! {
        section { aria_labelledby: "standings",
            h2 { id: "standings", "Standings" }
            table { id: "standing-list",
                thead {
                    tr {
                        th { scope: "col", "Rank" }
                        th { scope: "col", "Name" }
                        for header in standings.columns {
                            th { scope: "col", "{header}" }
                        }
                    }
                }
                tbody {
                    for row in standings.rows {
                        tr {
                            td { "{row.rank}" }
                            td { {team_name(&row.entrants)} }
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
