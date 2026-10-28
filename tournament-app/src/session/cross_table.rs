//! The cross table of a phase whose matches are played by fixed teams, e.g. a round robin.

use std::collections::HashMap;

use tournament_engine::{EntrantId, MatchResult, RankingId};

use super::{Session, bracket::plays_bracket};
use crate::view::{CrossMatch, CrossRow, CrossTable, Outcome};

impl Session {
    /// The cross table of the phase, if each of its matches is played by two teams of one
    /// ranking each and its format plays no bracket (see [`Self::bracket`]), e.g. a round robin
    /// or random rounds; `None` before the start, and for teams drawn anew each round, as in a
    /// Monster DYP.
    pub fn cross_table(&self) -> Option<CrossTable> {
        let phase = self.started_phase()?;
        if plays_bracket(phase) {
            return None;
        }
        let data = phase.data();
        let mut pairs: Vec<(RankingId, RankingId, &_)> = Vec::new();
        for r#match in data.matches().values() {
            let ([a], [b]) = (r#match.team_a().as_slice(), r#match.team_b().as_slice()) else {
                return None;
            };
            pairs.push((*a, *b, r#match));
        }
        // by the first entrant of each, in the order they were entered, as the list of entrants
        let entered: HashMap<EntrantId, usize> = self
            .competition()
            .entrants()
            .keys()
            .enumerate()
            .map(|(position, id)| (*id, position))
            .collect();
        let mut seeded: Vec<_> = data.rankings().values().collect();
        seeded.sort_by_key(|ranking| entered.get(ranking.entrants().first()));
        let index: HashMap<RankingId, usize> = seeded
            .iter()
            .enumerate()
            .map(|(index, ranking)| (ranking.id(), index))
            .collect();
        let mut rows: Vec<CrossRow> = seeded
            .iter()
            .map(|ranking| CrossRow {
                entrants: self.team(data, &[ranking.id()]),
                rank: ranking.rank(),
                cells: vec![Vec::new(); seeded.len()],
            })
            .collect();
        for (a, b, r#match) in pairs {
            let row = self.match_row(phase, r#match);
            let outcomes = match row.result {
                None => [None; 2],
                Some(MatchResult::AWins) => [Some(Outcome::Win), Some(Outcome::Loss)],
                Some(MatchResult::BWins) => [Some(Outcome::Loss), Some(Outcome::Win)],
                Some(MatchResult::Draw) => [Some(Outcome::Draw); 2],
            };
            let (a, b) = (index[&a], index[&b]);
            for ((team, against), outcome) in [(a, b), (b, a)].into_iter().zip(outcomes) {
                rows[team].cells[against].push(CrossMatch {
                    row: row.clone(),
                    outcome,
                });
            }
        }
        for row in &mut rows {
            for cell in &mut row.cells {
                cell.sort_by_key(|played| played.row.number);
            }
        }
        Some(CrossTable {
            rows,
            draws: phase.format().allows_draws(),
        })
    }
}
