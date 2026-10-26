//! The bracket of a knockout phase, every match in it from the start, drawn as columns of stages
//! joined by lines. Read in order, it is the list of the matches of each stage, which is what a
//! screen reader gets.

use dioxus::prelude::*;
use tournament_app::{
    i18n::Translator,
    view::{
        BracketColumn, BracketEntry, BracketLine, BracketPairing, BracketPart, BracketSection,
        BracketSource, StageName,
    },
};
use tournament_engine::MatchResult;

use crate::{competition::Ctx, i18n::t, result_text, team_name};

/// The bracket, if the format of the phase plays one.
#[component]
pub fn Bracket() -> Element {
    let ctx = use_context::<Ctx>();
    let Some(bracket) = ctx.session.read().bracket() else {
        return rsx! {};
    };
    rsx! {
        section { class: "card", aria_labelledby: "bracket",
            h2 { id: "bracket", {ctx.translator.read().get("bracket")} }
            for (index , section) in bracket.sections.into_iter().enumerate() {
                Section { key: "{index}", index, section }
            }
        }
    }
}

#[component]
fn Section(index: usize, section: BracketSection) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    // a single elimination is one bracket, its match for third place below the final
    let heading = match section.part {
        BracketPart::Knockout => None,
        BracketPart::Winners => Some(tr.get("bracket-winners")),
        BracketPart::Losers => Some(tr.get("bracket-losers")),
        BracketPart::GrandFinal => Some(tr.get("bracket-grand-final")),
    };
    let id = format!("bracket-{index}");
    let labelled_by = if heading.is_some() {
        id.clone()
    } else {
        "bracket".to_owned()
    };
    let nested = heading.is_some();
    rsx! {
        div { class: "bracket-section",
            if let Some(heading) = heading {
                h3 { id, {heading} }
            }
            // scrolls sideways if its columns do not fit
            div {
                class: "scroll",
                role: "group",
                tabindex: "0",
                aria_labelledby: labelled_by,
                div { class: "bracket",
                    for (index , column) in section.columns.into_iter().enumerate() {
                        Column { key: "{index}", column, nested }
                    }
                }
            }
        }
    }
}

/// A stage, headed by its name, a level below the section's heading if it has one, and the stage
/// drawn below it, if any.
#[component]
fn Column(column: BracketColumn, nested: bool) -> Element {
    rsx! {
        div { class: "bracket-column",
            Stage { stage: column.stage, nested }
            div { class: "bracket-slots",
                ol {
                    for (index , pairing) in column.pairings.into_iter().enumerate() {
                        Pairing { key: "{index}", pairing }
                    }
                }
                if let Some(below) = column.below {
                    div { class: "below",
                        Stage { stage: below.stage, nested }
                        ol {
                            for (index , pairing) in below.pairings.into_iter().enumerate() {
                                Pairing { key: "{index}", pairing }
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
fn Pairing(pairing: BracketPairing) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let line_out = match pairing.line_out {
        None => "",
        Some(BracketLine::Straight) => "line-straight",
        Some(BracketLine::Down) => "line-down",
        Some(BracketLine::Up) => "line-up",
    };
    let line_in = if pairing.line_in { "line-in" } else { "" };
    let result = pairing.r#match.as_ref().and_then(|row| row.result);
    // a match with a result recedes, as in the list of matches
    let finished = if result.is_some() { "finished" } else { "" };
    let side = |side: usize| {
        let entry = &pairing.sides[side];
        let class = if result == Some([MatchResult::AWins, MatchResult::BWins][side]) {
            "winner"
        } else if matches!(entry, BracketEntry::Team(_)) {
            ""
        } else {
            "pending"
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
                }
                {side(0)}
                span { class: "visually-hidden", {tr.get("versus")} }
                {side(1)}
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
