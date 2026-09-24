use std::num::NonZeroU32;

use itertools::Itertools;
use vec1::vec1;

use crate::{
    entities::competition::{Match, Phase},
    plugins::phase_format::PhaseFormat,
};

#[derive(Debug, Clone)]
pub struct RoundRobin;
impl PhaseFormat for RoundRobin {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match> {
        // we use the circle method to generate rounds, see
        // https://en.wikipedia.org/wiki/Round-robin_tournament#Circle_method
        if phase.matches().values().any(|m| !m.is_finished()) {
            // we only create a new round once all matches are finished
            return vec![];
        }
        let next_round = phase
            .matches()
            .values()
            .map(|m| m.round())
            .max()
            .map(|r| r.saturating_add(1))
            .unwrap_or(NonZeroU32::new(1).unwrap());
        let mut ranking_ids = phase
            .rankings()
            .values()
            .sorted_by_key(|r| r.initial_unique_rank())
            .map(|r| r.id())
            .collect_vec();
        let num_players = ranking_ids.len();
        let odd_num_players = num_players % 2 == 1;
        // We always fix the last element, but if we have an odd number of players the last element
        // is the by which is not part of the ranking_ids_by_initial_unique_rank vec.
        let num_players_with_bye = if odd_num_players {
            num_players + 1
        } else {
            num_players
        };
        assert!(num_players_with_bye > 0);
        // Every player plays every other player exactly once after `num_players_with_bye - 1`
        // rounds (the circle method's rotation period), so we stop generating further rounds then.
        if next_round.get() as usize > num_players_with_bye - 1 {
            return vec![];
        }
        // We rotate everything except the last (maybe not existing element)
        ranking_ids[..(num_players_with_bye - 1)].rotate_right((next_round.get() - 1) as usize);

        // now index 0 plays against index num_players_with_bye - 1 (if existing), index 1 against
        // num_players_with_bye - 2, and in general index `i` against `num_players_with_bye - i - 1`
        let first_index = if odd_num_players { 1 } else { 0 };
        (first_index..(num_players_with_bye / 2))
            .map(|i| {
                let team_a = vec1![ranking_ids[i]];
                let team_b = vec1![ranking_ids[num_players_with_bye - 1 - i]];
                Match::new(team_a, team_b, next_round)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use super::*;
    use crate::entities::competition::{MatchResult, PhaseBuilder};

    /// Plays out all rounds of a round robin phase with `num_players` players by repeatedly
    /// calling `create_new_matches` and then finishing all newly created matches, until no more
    /// matches get scheduled.
    fn play_out_all_rounds(num_players: u32) -> Phase {
        let mut phase = PhaseBuilder::new(Box::new(RoundRobin))
            .with_players(num_players)
            .build();
        loop {
            phase.create_new_matches();
            let unfinished_match_ids = phase
                .matches()
                .values()
                .filter(|m| !m.is_finished())
                .map(|m| m.id())
                .collect_vec();
            if unfinished_match_ids.is_empty() {
                // no new matches got scheduled, the phase is done
                break;
            }
            for match_id in unfinished_match_ids {
                phase.report_match_result(match_id, MatchResult::Draw);
            }
        }
        phase
    }

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
        let phase = play_out_all_rounds(6);
        assert_every_pair_played_exactly_once(&phase);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(5).unwrap())
        );
    }

    #[test]
    fn round_robin_schedules_every_pair_exactly_once_for_odd_number_of_players() {
        let phase = play_out_all_rounds(7);
        assert_every_pair_played_exactly_once(&phase);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(7).unwrap())
        );
    }
}
