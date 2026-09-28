use std::num::NonZeroU32;

use itertools::Itertools;
use rand::seq::{IteratorRandom, SliceRandom};
use serde::{Deserialize, Serialize};
use vec1::vec1;

use crate::{
    entities::{
        competition::EntropyTypes,
        phase::{Match, Phase},
    },
    plugins::phase_format::PhaseFormat,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RandomRounds {
    rounds: u32,
}

impl RandomRounds {
    /// Schedules `rounds` randomly chosen rounds of a round robin.
    ///
    /// If `rounds` is larger than the number of rounds a full round robin has, a full round robin
    /// is scheduled instead.
    pub const fn new(rounds: u32) -> Self {
        Self { rounds }
    }
}

#[typetag::serde]
impl PhaseFormat for RandomRounds {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match> {
        // we use the circle method to generate rounds, see
        // https://en.wikipedia.org/wiki/Round-robin_tournament#Circle_method
        if !phase.matches().is_empty() {
            // we generate all matches at once, so as soon as one match exists nothing is to be done
            return vec![];
        }
        let num_teams = phase.rankings().len();
        let num_teams_is_odd = num_teams % 2 == 1;
        // We always fix the last element, but if we have an odd number of players the last element
        // is the by which is not part of the ranking_ids_by_initial_unique_rank vec.
        let num_teams_with_bye = if num_teams_is_odd {
            num_teams + 1
        } else {
            num_teams
        };
        assert!(num_teams_with_bye > 1);
        let ranking_ids = phase
            .rankings()
            .values()
            .sorted_by_key(|r| r.initial_unique_rank())
            .map(|r| r.id())
            .collect_vec();
        let first_playing_index = if num_teams_is_odd { 1 } else { 0 };
        let mut rng = phase.new_seeded_std_rng(EntropyTypes::PhaseFormat);
        let mut rotates = (0..(num_teams_with_bye - 1)).sample(&mut rng, self.rounds as usize);
        rotates.shuffle(&mut rng);
        rotates
            .into_iter()
            .enumerate()
            .flat_map(|(i, r)| {
                let mut ranking_ids = ranking_ids.clone();
                // We rotate everything except the last element. Note that if num teams is odd
                // the last element is the bye which is not part of the vector, so in this case
                // we rotate the whole vector.
                ranking_ids[..(num_teams_with_bye - 1)].rotate_right(r);

                let round = NonZeroU32::new(i as u32 + 1).unwrap();
                // now index 0 plays against index num_players_with_bye - 1 (if existing), index 1
                // against num_players_with_bye - 2, and in general index `i` against
                // `num_players_with_bye - i - 1`
                (first_playing_index..(num_teams_with_bye / 2)).map(move |j| {
                    let team_a = vec1![ranking_ids[j]];
                    let team_b = vec1![ranking_ids[num_teams_with_bye - j - 1]];
                    Match::new(team_a, team_b, round)
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use super::*;
    use crate::entities::phase::PhaseBuilder;

    fn assert_every_pair_played_at_most_once(phase: &Phase) {
        assert!(phase.matches().values().all(|m| m.team_a().len() == 1));
        assert!(phase.matches().values().all(|m| m.team_b().len() == 1));
        assert!(
            phase
                .matches()
                .values()
                .map(|m| (m.team_a()[0], m.team_b()[0]))
                .all_unique()
        );
    }

    #[test]
    fn random_rounds_schedules_number_of_rounds_with_no_match_overlaps() {
        let mut phase = PhaseBuilder::new(Box::new(RandomRounds { rounds: 3 }))
            .with_players(6)
            .build();

        phase.create_new_matches();

        assert_every_pair_played_at_most_once(&phase);
        assert_eq!(phase.matches().len(), 3 * 3);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(3).unwrap())
        );
    }

    #[test]
    fn random_rounds_schedules_number_of_rounds_with_no_match_overlaps_with_odd_players() {
        let mut phase = PhaseBuilder::new(Box::new(RandomRounds { rounds: 3 }))
            .with_players(7)
            .build();

        phase.create_new_matches();

        assert_every_pair_played_at_most_once(&phase);
        assert_eq!(phase.matches().len(), 3 * 3);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(3).unwrap())
        );
    }

    #[test]
    fn random_rounds_if_rounds_is_larger_than_players_does_full_round_robin() {
        let mut phase = PhaseBuilder::new(Box::new(RandomRounds { rounds: 8 }))
            .with_players(7)
            .build();

        phase.create_new_matches();

        assert_every_pair_played_at_most_once(&phase);
        assert_eq!(phase.matches().len(), 7 * 3);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(7).unwrap())
        );
    }
}
