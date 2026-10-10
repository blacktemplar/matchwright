use std::num::NonZeroU32;

use itertools::Itertools;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use vec1::vec1;

use crate::{
    entities::competition::{Bye, EntropyTypes, Match, PhaseData},
    plugins::{
        Plugin,
        compatibility::{Declaration, EntrantsPerRanking},
        phase_format::{PhaseFormat, Schedule},
    },
};

/// A Monster DYP ("draw your partner"): every round, the rankings are drawn into random teams of
/// two, and every team plays another randomly drawn team.
///
/// Every ranking keeps its own place in the ranking and gets the result of every match its team
/// played. If the number of rankings is not a multiple of four, the ones left over sit the round
/// out, each with a bye. Those are always drawn from the rankings that sat out the fewest rounds so
/// far, so the numbers of matches the rankings play differ by at most one. With fewer than four
/// rankings nothing is scheduled.
///
/// All rounds are scheduled at once, when the phase starts.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MonsterDyp {
    rounds: u32,
}

impl MonsterDyp {
    /// A Monster DYP of `rounds` rounds.
    pub const fn new(rounds: u32) -> Self {
        Self { rounds }
    }
}

impl Plugin for MonsterDyp {
    fn declare(&self, declaration: &mut Declaration) {
        // the teams are drawn per match, so a ranking that already stands for a team of two
        // cannot be drawn into one
        declaration.requires_that::<EntrantsPerRanking>("rankings of single players", |entrants| {
            *entrants == EntrantsPerRanking::SINGLE
        });
    }
}

#[typetag::serde]
impl PhaseFormat for MonsterDyp {
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule {
        if !phase_data.matches().is_empty()
            || !phase_data.byes().is_empty()
            || phase_data.rankings().len() < 4
        {
            // we generate all rounds at once, so as soon as one match or bye exists nothing is to
            // be done
            return Schedule::default();
        }
        // sorted, because the iteration order of the rankings is not the same for every phase
        // with the same seeding
        let ranking_ids = phase_data
            .rankings()
            .values()
            .sorted_by_key(|r| r.initial_unique_rank())
            .map(|r| r.id())
            .collect_vec();
        let num_sitting_out = ranking_ids.len() % 4;
        let mut rounds_sat_out = vec![0_u32; ranking_ids.len()];
        let mut order = (0..ranking_ids.len()).collect_vec();
        let mut rng = phase_data.new_seeded_std_rng(EntropyTypes::PhaseFormat);
        let mut schedule = Schedule::default();
        for round in 1..=self.rounds {
            let round = NonZeroU32::new(round).unwrap();
            order.shuffle(&mut rng);
            // the sort is stable, so the shuffle decides between rankings that sat out equally
            // often
            let sitting_out = order
                .iter()
                .copied()
                .sorted_by_key(|&i| rounds_sat_out[i])
                .take(num_sitting_out)
                .collect_vec();
            for &i in &sitting_out {
                rounds_sat_out[i] += 1;
                schedule.byes.push(Bye::new(ranking_ids[i], round));
            }
            // the playing rankings are taken in shuffled order rather than in the sorted one above,
            // which would team up rankings that sat out equally often
            schedule.matches.extend(
                order
                    .iter()
                    .filter(|i| !sitting_out.contains(i))
                    .map(|&i| ranking_ids[i])
                    .tuples()
                    .map(|(a1, a2, b1, b2)| Match::new(vec1![a1, a2], vec1![b1, b2], round)),
            );
        }
        schedule
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::{MatchResult, Phase, PhaseBuilder, RankingId},
        map::Map,
        plugins::metrics_calculator::wins_draws_losses::{
            WinsDrawsLosses, WinsDrawsLossesCalculator,
        },
    };

    fn phase(rounds: u32, num_players: u32) -> Phase {
        let mut phase = PhaseBuilder::new(Box::new(MonsterDyp::new(rounds)))
            .metrics_calculator(Box::new(WinsDrawsLossesCalculator))
            .with_players(num_players)
            .build();
        phase.create_new_matches();
        phase
    }

    fn matches_per_ranking(phase: &Phase) -> Map<RankingId, u32> {
        let mut matches_per_ranking: Map<_, _> = phase.rankings().keys().map(|r| (*r, 0)).collect();
        for ranking_id in phase.matches().values().flat_map(Match::rankings) {
            matches_per_ranking[ranking_id] += 1;
        }
        matches_per_ranking
    }

    #[test]
    fn every_ranking_plays_every_round_in_a_team_of_two_if_the_rankings_are_a_multiple_of_four() {
        let phase = phase(3, 8);

        assert_eq!(phase.matches().len(), 3 * 2);
        assert!(
            phase
                .matches()
                .values()
                .all(|m| m.team_a().len() == 2 && m.team_b().len() == 2)
        );
        assert!(matches_per_ranking(&phase).values().all(|&n| n == 3));
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            NonZeroU32::new(3)
        );
    }

    #[test]
    fn the_rankings_left_over_take_turns_sitting_out() {
        // 2 of 10 rankings sit out every round, so in 5 rounds every ranking sits out once
        let phase = phase(5, 10);

        assert_eq!(phase.matches().len(), 5 * 2);
        assert!(matches_per_ranking(&phase).values().all(|&n| n == 4));
        assert_eq!(phase.byes().len(), 5 * 2);
        assert!(
            phase
                .rankings()
                .values()
                .all(|ranking| { ranking.metric::<WinsDrawsLosses>().unwrap().byes == 1 })
        );
    }

    #[test]
    fn the_numbers_of_matches_played_differ_by_at_most_one() {
        // 3 of 7 rankings sit out every round
        let phase = phase(5, 7);

        let (min, max) = matches_per_ranking(&phase)
            .values()
            .copied()
            .minmax()
            .into_option()
            .unwrap();
        assert_eq!(phase.matches().len(), 5);
        assert!(max - min <= 1, "{min} to {max} matches played");
    }

    #[test]
    fn the_teams_are_drawn_anew_every_round() {
        let phase = phase(5, 8);

        let teams = phase
            .matches()
            .values()
            .flat_map(|m| [m.team_a(), m.team_b()])
            .map(|team| {
                team.iter()
                    .copied()
                    .sorted_by_key(|r| phase.rankings()[r].initial_unique_rank())
                    .collect_vec()
            })
            .unique()
            .count();
        assert!(teams > 4, "only {teams} different teams in 5 rounds");
    }

    #[test]
    fn nothing_is_scheduled_for_fewer_than_four_rankings() {
        let phase = phase(5, 3);

        assert!(phase.matches().is_empty());
        assert!(phase.byes().is_empty());
    }

    #[test]
    fn nothing_is_scheduled_after_the_first_call() {
        let mut phase = phase(5, 8);
        let num_matches = phase.matches().len();

        phase.create_new_matches();

        assert_eq!(phase.matches().len(), num_matches);
    }

    #[test]
    fn both_rankings_of_a_team_get_its_result() {
        let mut phase = phase(1, 4);
        let r#match = phase.matches().values().next().unwrap();
        let (match_id, winners, losers) = (
            r#match.id(),
            r#match.team_a().clone(),
            r#match.team_b().clone(),
        );

        phase.report_match_result(match_id, MatchResult::AWins);

        for (team, expected) in [
            (
                winners,
                WinsDrawsLosses {
                    wins: 1,
                    draws: 0,
                    losses: 0,
                    byes: 0,
                },
            ),
            (
                losers,
                WinsDrawsLosses {
                    wins: 0,
                    draws: 0,
                    losses: 1,
                    byes: 0,
                },
            ),
        ] {
            for ranking_id in team {
                assert_eq!(
                    phase.rankings()[&ranking_id].metric::<WinsDrawsLosses>(),
                    Some(&expected)
                );
            }
        }
    }
}
