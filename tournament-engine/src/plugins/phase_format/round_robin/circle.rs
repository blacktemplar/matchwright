use std::num::NonZeroU32;

use itertools::Itertools;
use vec1::vec1;

use crate::{
    entities::competition::{Bye, Match, PhaseData, RankingId},
    plugins::phase_format::Schedule,
};

/// The rounds of a round robin, built with the circle method, see
/// <https://en.wikipedia.org/wiki/Round-robin_tournament#Circle_method>. Shared by the formats
/// that schedule rounds of a round robin, which differ only in which rotations they pick.
///
/// The rankings sit on a circle of an even number of seats, ordered by their initial unique rank.
/// With an odd number of rankings the last seat is a bye. The last seat stays fixed, every other
/// seat moves one step per rotation, and seat `i` plays seat `seats - i - 1`.
pub(super) struct Circle {
    ranking_ids: Vec<RankingId>,
    /// The number of seats, i.e. the number of rankings rounded up to an even number.
    seats: usize,
}

impl Circle {
    /// # Panics
    ///
    /// If `phase_data` has fewer than two rankings.
    pub(super) fn new(phase_data: &PhaseData) -> Self {
        let ranking_ids = phase_data.rankings().keys().copied().collect_vec();
        assert!(ranking_ids.len() > 1);
        let seats = ranking_ids.len().next_multiple_of(2);
        Self { ranking_ids, seats }
    }

    /// The number of distinct rotations, i.e. the number of rounds of a full round robin. Every
    /// pair of rankings meets in exactly one of them.
    pub(super) fn rotations(&self) -> usize {
        self.seats - 1
    }

    /// Adds the matches and the bye (if the number of rankings is odd) of `rotation` to
    /// `schedule`, as round `round`.
    ///
    /// # Panics
    ///
    /// If `rotation` is not less than [`Circle::rotations`].
    pub(super) fn schedule_round(
        &self,
        rotation: usize,
        round: NonZeroU32,
        schedule: &mut Schedule,
    ) {
        let rotations = self.rotations();
        assert!(rotation < rotations);
        // The ranking on `seat` after rotating every seat but the last one `rotation` steps to the
        // right. `None` is the bye.
        let on_seat = |seat: usize| {
            let index = if seat < rotations {
                (seat + rotations - rotation) % rotations
            } else {
                seat
            };
            self.ranking_ids.get(index).copied()
        };
        let pairs = (0..self.seats / 2).map(|i| (on_seat(i), on_seat(self.seats - i - 1)));
        for pair in pairs {
            match pair {
                (Some(a), Some(b)) => schedule.matches.push(Match::new(vec1![a], vec1![b], round)),
                (Some(ranking), None) | (None, Some(ranking)) => {
                    schedule.byes.push(Bye::new(ranking, round))
                }
                (None, None) => unreachable!("there is at most one bye"),
            }
        }
    }
}
