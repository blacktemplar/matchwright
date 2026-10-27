//! The bracket of a knockout phase, every match in it from the start, drawn as columns of stages
//! joined by lines. Read in order, it is the list of the matches of each stage, which is what a
//! screen reader gets.
//!
//! The result of a match is reported by a click on the side that won, whose name is a button
//! until then; the focus moves on to the next match without a result, as in the list of matches.

use dioxus::prelude::*;
use tournament_app::{
    i18n::Translator,
    view::{
        self, BracketColumn, BracketEntry, BracketLine, BracketPairing, BracketPart,
        BracketSection, BracketSource, MatchRow, StageName, Unplayed,
    },
};
use tournament_engine::{MatchId, MatchResult};

use crate::{
    announcement::Announcement,
    competition::Ctx,
    i18n::t,
    play::{move_focus, use_focus},
    result_text, team_name,
};

/// What gets the focus once the page shows a result reported in the bracket.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    /// The side A of the match, reporting that it won.
    Report(MatchId),
    /// The heading of the bracket, once no match of it is left without a result.
    Heading,
}

/// The bracket, if the format of the phase plays one.
#[component]
pub fn Bracket() -> Element {
    let ctx = use_context::<Ctx>();
    let focus = use_signal(|| None::<Focus>);
    let on_heading_mounted = use_focus(focus, Focus::Heading);
    let Some(bracket) = ctx.session.read().bracket() else {
        return rsx! {};
    };
    let tr = ctx.translator.read();
    // said only to the tab that reports, while there is something to report
    let reports = ctx.actions().report;
    rsx! {
        section { class: "card", aria_labelledby: "bracket",
            // focused once no match is left without a result, so that the focus is not lost
            h2 { id: "bracket", tabindex: "-1", onmounted: on_heading_mounted,
                {tr.get("bracket")}
            }
            if reports {
                p { class: "hint", {tr.get("bracket-report-hint")} }
            }
            // every section in one, scrolling sideways together if its columns do not fit
            div {
                class: "scroll",
                role: "group",
                tabindex: "0",
                aria_labelledby: "bracket",
                for (index , section) in bracket.sections.into_iter().enumerate() {
                    Section {
                        key: "{index}",
                        index,
                        section,
                        focus,
                    }
                }
            }
        }
    }
}

/// The match without a result that follows `reported` in the order of the bracket, else the
/// first one before it, else the heading.
fn after_report(bracket: &view::Bracket, reported: MatchId) -> Focus {
    let rows: Vec<&MatchRow> = bracket
        .sections
        .iter()
        .flat_map(|section| &section.columns)
        .flat_map(|column| {
            let below = column.below.iter().flat_map(|below| &below.pairings);
            column.pairings.iter().chain(below)
        })
        .filter_map(|pairing| pairing.r#match.as_ref())
        .collect();
    let position = rows.iter().position(|row| row.id == reported).unwrap_or(0);
    rows[position..]
        .iter()
        .chain(&rows[..position])
        .find(|row| row.result.is_none())
        .map_or(Focus::Heading, |row| Focus::Report(row.id))
}

#[component]
fn Section(index: usize, section: BracketSection, focus: Signal<Option<Focus>>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    // a single elimination is one bracket, its match for third place below the final
    let heading = match section.part {
        BracketPart::Knockout => None,
        BracketPart::Winners => Some(tr.get("bracket-winners")),
        BracketPart::Losers => Some(tr.get("bracket-losers")),
        BracketPart::GrandFinal => Some(tr.get("bracket-grand-final")),
    };
    let nested = heading.is_some();
    rsx! {
        div { class: "bracket-section",
            if let Some(heading) = heading {
                h3 { id: "bracket-{index}", {heading} }
            }
            div { class: "bracket",
                for (index , column) in section.columns.into_iter().enumerate() {
                    Column {
                        key: "{index}",
                        column,
                        nested,
                        focus,
                    }
                }
            }
        }
    }
}

/// A stage, headed by its name, a level below the section's heading if it has one, and the stage
/// drawn below it, if any.
#[component]
fn Column(column: BracketColumn, nested: bool, focus: Signal<Option<Focus>>) -> Element {
    rsx! {
        div { class: "bracket-column",
            Stage { stage: column.stage, nested }
            div { class: "bracket-slots",
                ol {
                    for (index , pairing) in column.pairings.into_iter().enumerate() {
                        Pairing { key: "{index}", pairing, focus }
                    }
                }
                if let Some(below) = column.below {
                    div { class: "below",
                        Stage { stage: below.stage, nested }
                        ol {
                            for (index , pairing) in below.pairings.into_iter().enumerate() {
                                Pairing { key: "{index}", pairing, focus }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The heading of a stage.
#[component]
fn Stage(stage: Option<StageName>, nested: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let stage = stage.as_ref().map(|stage| tr.stage(stage));
    rsx! {
        if nested {
            h4 { class: "bracket-stage", {stage} }
        } else {
            h3 { class: "bracket-stage", {stage} }
        }
    }
}

/// A pairing in its slot of the column, which the lines to and from it are drawn in.
#[component]
fn Pairing(pairing: BracketPairing, focus: Signal<Option<Focus>>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    // its slot kept, so that the others stay where the lines meet them
    match pairing.unplayed {
        Some(Unplayed::Byes) => {
            return rsx! {
                li { class: "slot", aria_hidden: "true",
                    div { class: "pairing unplayed" }
                }
            };
        }
        Some(Unplayed::NotNeeded) => {
            return rsx! {
                li { class: "slot",
                    div { class: "pairing unplayed",
                        span { class: "if-needed", {tr.get("bracket-not-needed")} }
                    }
                }
            };
        }
        None => {}
    }
    let line_out = match pairing.line_out {
        None => "",
        Some(BracketLine::Straight) => "line-straight",
        Some(BracketLine::Down) => "line-down",
        Some(BracketLine::Up) => "line-up",
    };
    let line_in = if pairing.line_in { "line-in" } else { "" };
    let result = pairing.r#match.as_ref().and_then(|row| row.result);
    // a match with a result recedes, as in the list of matches, and so does a pairing decided by a
    // bye, which has nothing to play
    let bye = pairing.r#match.is_none() && pairing.sides.contains(&BracketEntry::Bye);
    let finished = if result.is_some() || bye {
        "finished"
    } else {
        ""
    };
    // only reported by the tab that changes the competition
    let to_report = (result.is_none() && ctx.changes())
        .then(|| pairing.r#match.clone())
        .flatten();
    let side = |side: usize| {
        let entry = &pairing.sides[side];
        let class = if result == Some([MatchResult::AWins, MatchResult::BWins][side]) {
            "winner"
        } else {
            match entry {
                BracketEntry::Team(_) => "",
                // nobody, rather than somebody to come
                BracketEntry::Bye => "nobody",
                BracketEntry::WinnerOf(_) | BracketEntry::LoserOf(_) => "pending",
            }
        };
        rsx! {
            span { class: "side {class}", {entry_text(&tr, entry)} }
        }
    };
    rsx! {
        li { class: "slot {line_in} {line_out}",
            div { class: "pairing {finished}",
                if let Some(row) = &pairing.r#match {
                    span { class: "number",
                        span { class: "visually-hidden", {tr.get("match")} }
                        " {row.number}"
                    }
                } else {
                    // empty, so that the names line up with those of the numbered pairings
                    span { class: "number", aria_hidden: "true" }
                }
                if let Some(row) = to_report {
                    WinButtons { row, focus }
                } else {
                    {side(0)}
                    span { class: "visually-hidden", {tr.get("versus")} }
                    {side(1)}
                }
                if let Some(text) = pairing.r#match.as_ref().and_then(|row| result_text(&tr, row)) {
                    span { class: "visually-hidden", ", {text}" }
                }
                if pairing.only_if_needed {
                    span { class: "if-needed", {tr.get("if-needed")} }
                }
            }
        }
    }
}

/// The sides of the match `row`, which has no result yet, each a button reporting that it won (a
/// knockout has no draws).
#[component]
fn WinButtons(row: MatchRow, focus: Signal<Option<Focus>>) -> Element {
    let ctx = use_context::<Ctx>();
    let id = row.id;
    let on_mounted = use_focus(focus, Focus::Report(id));
    let tr = ctx.translator.read();
    let (a, b) = (team_name(&tr, &row.a), team_name(&tr, &row.b));
    let (a_wins, b_wins) = (
        t!(tr, "wins", team = a.as_str()),
        t!(tr, "wins", team = b.as_str()),
    );
    let report = move |result| {
        move |_| {
            if let Some(reported) = ctx.act(move |session| session.report(id, result)) {
                ctx.announce(Announcement::Reported(reported));
                // after the result, which may have scheduled the next match of the winner
                let next = ctx
                    .session
                    .read()
                    .bracket()
                    .map_or(Focus::Heading, |bracket| after_report(&bracket, id));
                move_focus(focus, next);
            }
        }
    };
    // the name alone on the screen; what a click does is said under the pointer
    rsx! {
        button {
            class: "side",
            aria_label: "{a_wins}",
            title: "{a_wins}",
            onmounted: on_mounted,
            onclick: report(MatchResult::AWins),
            "{a}"
        }
        span { class: "visually-hidden", {tr.get("versus")} }
        button {
            class: "side",
            aria_label: "{b_wins}",
            title: "{b_wins}",
            onclick: report(MatchResult::BWins),
            "{b}"
        }
    }
}

/// Who takes a side, or where they come from while it is not decided.
fn entry_text(tr: &Translator, entry: &BracketEntry) -> String {
    // the stage, and the place in it where it has several pairings
    let stage = |source: &BracketSource| {
        let stage = source
            .stage
            .as_ref()
            .map(|stage| tr.stage(stage))
            .unwrap_or_default();
        match source.position {
            Some(position) => t!(tr, "stage-position", stage = stage, position = position),
            None => stage,
        }
    };
    match entry {
        BracketEntry::Team(team) => team_name(tr, team),
        BracketEntry::Bye => tr.get("bracket-bye"),
        BracketEntry::WinnerOf(source) => match source.number {
            Some(number) => t!(tr, "winner-of-match", number = number),
            None => t!(tr, "winner-of-stage", stage = stage(source)),
        },
        BracketEntry::LoserOf(source) => match source.number {
            Some(number) => t!(tr, "loser-of-match", number = number),
            None => t!(tr, "loser-of-stage", stage = stage(source)),
        },
    }
}
