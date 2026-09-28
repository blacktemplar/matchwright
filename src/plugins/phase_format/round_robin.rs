use std::num::NonZeroU32;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use vec1::vec1;

use crate::{
    entities::phase::{Match, Phase},
    plugins::phase_format::PhaseFormat,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoundRobin;

#[typetag::serde]
impl PhaseFormat for RoundRobin {
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
        let mut ranking_ids = phase
            .rankings()
            .values()
            .sorted_by_key(|r| r.initial_unique_rank())
            .map(|r| r.id())
            .collect_vec();
        let first_playing_index = if num_teams_is_odd { 1 } else { 0 };
        let mut matches = Vec::with_capacity(num_teams * (num_teams - 1) / 2);
        for round in
            NonZeroU32::new(1).unwrap()..NonZeroU32::new(num_teams_with_bye as u32).unwrap()
        {
            if round != NonZeroU32::new(1).unwrap() {
                // We rotate everything except the last element. Note that if num teams is odd
                // the last element is the bye which is not part of the vector, so in this case
                // we rotate the whole vector.
                ranking_ids[..(num_teams_with_bye - 1)].rotate_right(1);
            }

            // now index 0 plays against index num_players_with_bye - 1 (if existing), index 1
            // against num_players_with_bye - 2, and in general index `i` against
            // `num_players_with_bye - i - 1`
            matches.extend((first_playing_index..(num_teams_with_bye / 2)).map(|i| {
                let team_a = vec1![ranking_ids[i]];
                let team_b = vec1![ranking_ids[num_teams_with_bye - i - 1]];
                Match::new(team_a, team_b, round)
            }));
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use super::*;
    use crate::entities::phase::PhaseBuilder;

    /// Asserts that every unordered pair of ranking ids in the phase played against each other
    /// in exactly one scheduled match.
    fn assert_every_pair_played_exactly_once(phase: &Phase) {
        let mut expected_pairs = phase
            .rankings()
            .keys()
            .copied()
            .array_combinations::<2>()
            .collect_vec();
        for r#match in phase.matches().values() {
            let team_a_id = r#match.team_a()[0];
            let team_b_id = r#match.team_b()[0];
            let position = expected_pairs
                .iter()
                .position(|&[a, b]| {
                    (a, b) == (team_a_id, team_b_id) || (a, b) == (team_b_id, team_a_id)
                })
                .expect("every scheduled match must be an expected pair that wasn't played yet");
            expected_pairs.remove(position);
        }
        assert!(
            expected_pairs.is_empty(),
            "not all pairs got scheduled, missing: {expected_pairs:?}"
        );
    }

    #[test]
    fn round_robin_schedules_every_pair_exactly_once_for_even_number_of_players() {
        let mut phase = PhaseBuilder::new(Box::new(RoundRobin))
            .with_players(6)
            .build();

        phase.create_new_matches();

        assert_every_pair_played_exactly_once(&phase);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(5).unwrap())
        );
    }

    #[test]
    fn round_robin_schedules_every_pair_exactly_once_for_odd_number_of_players() {
        let mut phase = PhaseBuilder::new(Box::new(RoundRobin))
            .with_players(7)
            .build();

        phase.create_new_matches();

        assert_every_pair_played_exactly_once(&phase);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(7).unwrap())
        );
    }
}
