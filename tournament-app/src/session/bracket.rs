//! The bracket of a knockout phase, which only the knockout formats of the engine have.

use std::{any::Any, collections::HashMap};

use tournament_engine::{
    TypeMap,
    plugins::phase_format::{
        Advance, BracketPairing as Pairing, BracketSide, DoubleElimination, DoubleEliminationRound,
        KnockoutRound, PairingId, Seat, SingleElimination,
    },
};

use super::{Session, stage};
use crate::view::{
    Bracket, BracketColumn, BracketEntry, BracketLine, BracketPairing, BracketPart, BracketSection,
    BracketSource, Unplayed,
};

/// What a pairing is laid out with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Part(BracketPart),
    /// The stage below the last column of the section before, apart from it.
    Below,
}

/// Where a pairing is drawn: its section, its column in it, and its place in the column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    section: usize,
    column: usize,
    index: usize,
}

impl Session {
    /// The bracket of the phase, if its format plays one, i.e. a single or a double elimination;
    /// `None` before the start.
    pub fn bracket(&self) -> Option<Bracket> {
        let phase = self.started_phase()?;
        let format: &dyn Any = phase.format();
        let (bracket, part): (_, fn(&TypeMap) -> Section) =
            if let Some(format) = format.downcast_ref::<SingleElimination>() {
                (format.bracket()?, single_elimination_part)
            } else {
                let format = format.downcast_ref::<DoubleElimination>()?;
                (format.bracket()?, double_elimination_part)
            };
        let pairings = bracket.pairings();
        let by_id: HashMap<PairingId, &Pairing> = pairings
            .iter()
            .map(|pairing| (pairing.id, pairing))
            .collect();

        // the sections in the order their first pairings were added, each with its columns by
        // stage, in the same order
        let mut layout: Vec<(Section, Vec<(_, Vec<PairingId>)>)> = Vec::new();
        let mut places = HashMap::new();
        for pairing in &pairings {
            let part = part(pairing.annotations);
            let section = layout
                .iter()
                .position(|(other, _)| *other == part)
                .unwrap_or_else(|| {
                    layout.push((part, Vec::new()));
                    layout.len() - 1
                });
            let columns = &mut layout[section].1;
            let stage = stage(phase, pairing.annotations);
            let column = columns
                .iter()
                .position(|(other, _)| *other == stage)
                .unwrap_or_else(|| {
                    columns.push((stage, Vec::new()));
                    columns.len() - 1
                });
            let ids = &mut columns[column].1;
            places.insert(
                pairing.id,
                Place {
                    section,
                    column,
                    index: ids.len(),
                },
            );
            ids.push(pairing.id);
        }

        let data = phase.data();
        // the sides only a bye can take
        let byes: HashMap<PairingId, [bool; 2]> = pairings
            .iter()
            .map(|pairing| (pairing.id, pairing.sides.map(|side| side.bye)))
            .collect();
        let unplayed: HashMap<PairingId, Unplayed> = pairings
            .iter()
            .filter_map(|pairing| {
                if byes[&pairing.id] == [true; 2] {
                    return Some((pairing.id, Unplayed::Byes));
                }
                // waiting for a pairing whose result moved nobody on
                let decided_without_it = pairing.sides.iter().any(|side| {
                    let Some(Advance::WinnerOf(from) | Advance::LoserOf(from)) = side.from else {
                        return false;
                    };
                    let from = by_id[&from];
                    from.advances_only_if_won_by.is_some()
                        && from
                            .r#match
                            .is_some_and(|id| data.matches()[&id].result().is_some())
                }) && pairing.sides.iter().all(|side| side.seat.is_none());
                decided_without_it.then_some((pairing.id, Unplayed::NotNeeded))
            })
            .collect();

        // the place each pairing's winner is drawn at next, if that is in the next column; none
        // from or to a pairing never played
        let mut next: HashMap<PairingId, Place> = HashMap::new();
        for pairing in &pairings {
            if unplayed.contains_key(&pairing.id) {
                continue;
            }
            let place = places[&pairing.id];
            for side in pairing.sides {
                if let Some(Advance::WinnerOf(from)) = side.from
                    && !unplayed.contains_key(&from)
                    && places[&from].section == place.section
                    && places[&from].column + 1 == place.column
                {
                    next.insert(from, place);
                }
            }
        }

        let number = |pairing: &Pairing| {
            pairing.number.map(|number| {
                usize::try_from(number.get()).expect("a number of a match fits a usize")
            })
        };
        let source = |id: PairingId| {
            let pairing = by_id[&id];
            let place = places[&id];
            let column = &layout[place.section].1[place.column].1;
            BracketSource {
                number: number(pairing),
                stage: stage(phase, pairing.annotations),
                position: (column.len() > 1).then_some(place.index + 1),
            }
        };
        // who takes `side`, where the winner of a pairing against a bye is whoever plays the bye
        let entry = |mut side: BracketSide| loop {
            return match (side.seat, side.from) {
                (Some(Seat::Ranking(ranking)), _) => {
                    BracketEntry::Team(self.team(data, &[ranking]))
                }
                (Some(Seat::Bye), _) => BracketEntry::Bye,
                (None, Some(Advance::WinnerOf(from) | Advance::LoserOf(from)))
                    if byes[&from] == [true; 2] =>
                {
                    BracketEntry::Bye
                }
                (None, Some(Advance::LoserOf(from))) if byes[&from].contains(&true) => {
                    BracketEntry::Bye
                }
                (None, Some(Advance::WinnerOf(from))) if byes[&from].contains(&true) => {
                    let playing = usize::from(byes[&from][0]);
                    side = by_id[&from].sides[playing];
                    continue;
                }
                (None, Some(Advance::WinnerOf(from))) => BracketEntry::WinnerOf(source(from)),
                (None, Some(Advance::LoserOf(from))) => BracketEntry::LoserOf(source(from)),
                (None, None) => unreachable!("a side the bracket was built with is taken"),
            };
        };
        let row = |pairing: &Pairing| {
            let sides = pairing.sides.map(entry);
            let place = places[&pairing.id];
            let line_out = next.get(&pairing.id).map(|to| {
                // the places of the pairings whose winners meet in `to`, this one's included
                let joining: Vec<_> = next
                    .iter()
                    .filter(|(_, other)| *other == to)
                    .map(|(from, _)| places[from].index)
                    .collect();
                if joining.len() == 1 {
                    BracketLine::Straight
                } else if joining.iter().all(|other| place.index <= *other) {
                    BracketLine::Down
                } else {
                    BracketLine::Up
                }
            });
            let from = || pairing.sides.iter().filter_map(|side| side.from);
            let unplayed = unplayed.get(&pairing.id).copied();
            BracketPairing {
                number: number(pairing),
                r#match: pairing.r#match.map(|id| {
                    let r#match = data
                        .matches()
                        .get(&id)
                        .expect("the bracket only holds matches of its phase");
                    self.match_row(phase, r#match)
                }),
                sides,
                line_out,
                line_in: next.values().any(|to| *to == place),
                // until it is scheduled or known not to be played
                only_if_needed: pairing.r#match.is_none()
                    && unplayed.is_none()
                    && from().any(|advance| {
                        let (Advance::WinnerOf(from) | Advance::LoserOf(from)) = advance;
                        by_id[&from].advances_only_if_won_by.is_some()
                    }),
                unplayed,
            }
        };
        let mut sections: Vec<BracketSection> = Vec::new();
        for (section, columns) in &layout {
            let mut columns = columns.iter().map(|(stage, ids)| BracketColumn {
                stage: stage.clone(),
                pairings: ids.iter().map(|id| row(by_id[id])).collect(),
                below: None,
            });
            match section {
                Section::Part(part) => sections.push(BracketSection {
                    part: *part,
                    columns: columns.collect(),
                }),
                Section::Below => {
                    let last = sections
                        .last_mut()
                        .and_then(|section| section.columns.last_mut())
                        .expect("a stage below another follows it");
                    last.below = columns.next().map(Box::new);
                }
            }
        }
        Some(Bracket { sections })
    }
}

/// The match for third place is drawn below the final.
fn single_elimination_part(annotations: &TypeMap) -> Section {
    match annotations.get::<KnockoutRound>() {
        Some(KnockoutRound::ThirdPlace) => Section::Below,
        _ => Section::Part(BracketPart::Knockout),
    }
}

fn double_elimination_part(annotations: &TypeMap) -> Section {
    Section::Part(match annotations.get::<DoubleEliminationRound>() {
        Some(DoubleEliminationRound::Losers { .. }) => BracketPart::Losers,
        Some(DoubleEliminationRound::GrandFinal | DoubleEliminationRound::GrandFinalReset) => {
            BracketPart::GrandFinal
        }
        _ => BracketPart::Winners,
    })
}
