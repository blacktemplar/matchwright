use std::{mem, num::NonZeroU32};

use serde::{Deserialize, Serialize};
use vec1::vec1;

use crate::{
    entities::competition::{Bye, Match, MatchId, MatchResult, PhaseData, RankingId, Team},
    plugins::phase_format::Schedule,
    type_map::TypeMap,
};

/// A fixed tree of pairings, in which the winner and the loser of a pairing move on to given seats
/// of later pairings. Shared by the knockout formats, which build it from the seeds on their first
/// call and keep it as their state.
///
/// A pairing is scheduled as a match as soon as both of its seats are taken, without waiting for
/// the rest of its round. A pairing with a bye on one side is decided without a match, for the
/// other side, which is scheduled a [`Bye`] if it is a ranking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Bracket {
    pairings: Vec<Pairing>,
    /// The pairings whose seats are both taken but which are not decided yet, i.e. those that
    /// [`Bracket::create_new_matches`] has to look at.
    in_play: Vec<PairingId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct PairingId(usize);

/// Who takes a seat of a pairing.
#[derive(Debug, Clone, Copy)]
pub(super) enum Source {
    Ranking(RankingId),
    /// Nobody, so whoever is paired with it advances without a match.
    Bye,
    WinnerOf(PairingId),
    LoserOf(PairingId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Seat {
    Ranking(RankingId),
    Bye,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Slot {
    pairing: PairingId,
    side: Team,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pairing {
    round: NonZeroU32,
    /// Moved into the match or the bye once it is scheduled.
    annotations: TypeMap,
    /// `None` while the source of the seat is not decided yet.
    seats: [Option<Seat>; 2],
    r#match: Option<MatchId>,
    winner_to: Option<Slot>,
    loser_to: Option<Slot>,
    /// If set, the winner and the loser only move on if this side wins.
    advances_only_if_won_by: Option<Team>,
}

impl Bracket {
    /// Adds a pairing to be played in phase round `round` between the two `sources`, as sides A
    /// and B. The match or the bye it schedules is annotated with `annotations`.
    ///
    /// A pairing can only take the winner and the loser of a pairing added before it, each at most
    /// once, and all pairings have to be added before the first [`Bracket::create_new_matches`].
    pub(super) fn add_pairing(
        &mut self,
        round: NonZeroU32,
        sources: [Source; 2],
        annotations: TypeMap,
    ) -> PairingId {
        let id = PairingId(self.pairings.len());
        let mut seats = [None; 2];
        for (side, source) in [Team::A, Team::B].into_iter().zip(sources) {
            let slot = Some(Slot { pairing: id, side });
            match source {
                Source::Ranking(ranking_id) => seats[index(side)] = Some(Seat::Ranking(ranking_id)),
                Source::Bye => seats[index(side)] = Some(Seat::Bye),
                Source::WinnerOf(from) => {
                    let winner_to = &mut self.pairings[from.0].winner_to;
                    debug_assert!(winner_to.is_none(), "a winner can only move on once");
                    *winner_to = slot;
                }
                Source::LoserOf(from) => {
                    let loser_to = &mut self.pairings[from.0].loser_to;
                    debug_assert!(loser_to.is_none(), "a loser can only move on once");
                    *loser_to = slot;
                }
            }
        }
        if seats.iter().all(Option::is_some) {
            self.in_play.push(id);
        }
        self.pairings.push(Pairing {
            round,
            annotations,
            seats,
            r#match: None,
            winner_to: None,
            loser_to: None,
            advances_only_if_won_by: None,
        });
        id
    }

    /// Lets the winner and the loser of `pairing` only move on if `side` wins it.
    pub(super) fn advance_only_if_won_by(&mut self, pairing: PairingId, side: Team) {
        self.pairings[pairing.0].advances_only_if_won_by = Some(side);
    }

    /// Adds a knockout between the rankings of `phase_data`, as many rounds as it takes to leave
    /// one of them, round `r` played in phase round `phase_round(r)` and its matches annotated with
    /// `annotations(r, rounds)`.
    ///
    /// Returns the pairings of every round, each in bracket order, i.e. the winners of the pairings
    /// `2i` and `2i + 1` of a round meet in pairing `i` of the next. The first round pairs the best
    /// seed with the worst, the best two seeds can meet in the final at the earliest, and where the
    /// field is not a power of two the best seeds get the byes.
    pub(super) fn add_knockout(
        &mut self,
        phase_data: &PhaseData,
        phase_round: impl Fn(u32) -> NonZeroU32,
        annotations: impl Fn(u32, u32) -> TypeMap,
    ) -> Vec<Vec<PairingId>> {
        let rounds = phase_data
            .rankings()
            .len()
            .next_power_of_two()
            .trailing_zeros();
        let mut seeds = vec![Source::Bye; 1 << rounds];
        for ranking in phase_data.rankings().values() {
            seeds[ranking.initial_unique_rank().get() as usize - 1] = Source::Ranking(ranking.id());
        }
        let mut sources: Vec<_> = seeding_order(rounds)
            .into_iter()
            .map(|seed| seeds[seed])
            .collect();
        (1..=rounds)
            .map(|round| {
                let pairings: Vec<_> = sources
                    .as_chunks()
                    .0
                    .iter()
                    .map(|&sides| {
                        self.add_pairing(phase_round(round), sides, annotations(round, rounds))
                    })
                    .collect();
                sources = pairings.iter().copied().map(Source::WinnerOf).collect();
                pairings
            })
            .collect()
    }

    /// Decides every pairing in play whose match got a result, moves its winner and its loser on,
    /// and returns the matches and the byes of the pairings that got both of their seats taken by
    /// that, each ordered by round and then by the order their pairings were added in.
    pub(super) fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule {
        let mut new_matches = Vec::new();
        let mut new_byes = Vec::new();
        let mut to_check = mem::take(&mut self.in_play);
        while let Some(id) = to_check.pop() {
            let pairing = &mut self.pairings[id.0];
            let [Some(a), Some(b)] = pairing.seats else {
                unreachable!("only pairings with both seats taken are in play")
            };
            let winner = match (a, b) {
                (Seat::Ranking(a), Seat::Ranking(b)) => {
                    let result = pairing
                        .r#match
                        .map(|match_id| phase_data.matches()[&match_id].result());
                    match result {
                        None => {
                            let r#match = Match::new(vec1![a], vec1![b], pairing.round)
                                .with_annotations(mem::take(&mut pairing.annotations));
                            pairing.r#match = Some(r#match.id());
                            new_matches.push((id, r#match));
                            self.in_play.push(id);
                            continue;
                        }
                        Some(MatchResult::NotFinished) => {
                            self.in_play.push(id);
                            continue;
                        }
                        Some(MatchResult::AWins) => Team::A,
                        Some(MatchResult::BWins) => Team::B,
                        Some(MatchResult::Draw) => {
                            unreachable!("draws are rejected since knockouts do not allow them")
                        }
                    }
                }
                (Seat::Ranking(a), Seat::Bye) => {
                    let bye = Bye::new(a, pairing.round)
                        .with_annotations(mem::take(&mut pairing.annotations));
                    new_byes.push((id, bye));
                    Team::A
                }
                (Seat::Bye, Seat::Ranking(b)) => {
                    let bye = Bye::new(b, pairing.round)
                        .with_annotations(mem::take(&mut pairing.annotations));
                    new_byes.push((id, bye));
                    Team::B
                }
                (Seat::Bye, Seat::Bye) => Team::A,
            };
            self.decide(id, winner, &mut to_check);
        }
        new_matches.sort_by_key(|(id, r#match)| (r#match.round(), *id));
        new_byes.sort_by_key(|(id, bye)| (bye.round(), *id));
        Schedule {
            matches: new_matches
                .into_iter()
                .map(|(_, r#match)| r#match)
                .collect(),
            byes: new_byes.into_iter().map(|(_, bye)| bye).collect(),
        }
    }

    /// Moves the winner and the loser of `id`, which side `winner` won, on to their next pairings,
    /// and pushes those whose seats are both taken now to `to_check`.
    fn decide(&mut self, id: PairingId, winner: Team, to_check: &mut Vec<PairingId>) {
        let pairing = &self.pairings[id.0];
        if pairing
            .advances_only_if_won_by
            .is_some_and(|side| side != winner)
        {
            return;
        }
        let seats = pairing
            .seats
            .map(|seat| seat.expect("a decided pairing has both seats"));
        let moves = [
            (pairing.winner_to, seats[index(winner)]),
            (pairing.loser_to, seats[index(winner.opponent())]),
        ];
        for (to, seat) in moves {
            let Some(Slot { pairing, side }) = to else {
                continue;
            };
            let next = &mut self.pairings[pairing.0];
            next.seats[index(side)] = Some(seat);
            if next.seats.iter().all(Option::is_some) {
                to_check.push(pairing);
            }
        }
    }
}

fn index(side: Team) -> usize {
    match side {
        Team::A => 0,
        Team::B => 1,
    }
}

/// The seeds of a full bracket of `2^rounds`, counted from 0, in the order they are placed into
/// its first round, two neighbours each forming a match. The best seed meets the worst, and the
/// best two seeds can meet in the final at the earliest.
fn seeding_order(rounds: u32) -> Vec<usize> {
    let mut order = vec![0];
    for _ in 0..rounds {
        let size = 2 * order.len();
        order = order
            .iter()
            .flat_map(|&seed| [seed, size - 1 - seed])
            .collect();
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_are_placed_so_that_the_best_ones_meet_last() {
        assert_eq!(seeding_order(0), [0]);
        assert_eq!(seeding_order(3), [0, 7, 3, 4, 1, 6, 2, 5]);
    }
}
