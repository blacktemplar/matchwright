use std::num::NonZeroU32;

use itertools::Itertools;
use vec1::vec1;

use crate::{
    entities::competition::{Match, MatchResult, Phase, RankingId},
    hash_map::HashMap,
    plugins::phase_format::PhaseFormat,
};

#[derive(Debug, Clone)]
pub struct SingleElimination;
impl PhaseFormat for SingleElimination {
    fn create_new_matches(&self, phase: &Phase) -> Vec<Match> {
        if !phase.matches().values().all(Match::is_finished) {
            return vec![];
        }
        let max_round = phase.matches().values().map(Match::round).max();
        if let Some(max_round) = max_round {
            // The ordering of this vector will be determined by match winners. If a lower-ranked
            // team wins, it will be swapped with the higher-ranked losing team.
            let mut ranking_id_to_swap_replaced_rank: HashMap<_, _> = phase
                .rankings()
                .iter()
                .map(|(id, r)| (*id, r.initial_unique_rank()))
                .collect();
            // matches are ordered by round, which is important so that the following works:
            let mut lowest_loser_rank = NonZeroU32::MAX;
            let matches_odered_by_round = phase.matches().values().sorted_by_key(|m| m.round());
            for r#match in matches_odered_by_round {
                let team_a_id = r#match.team_a()[0];
                let team_b_id = r#match.team_b()[0];
                let rank_a = ranking_id_to_swap_replaced_rank[&team_a_id];
                let rank_b = ranking_id_to_swap_replaced_rank[&team_b_id];
                let max_rank = rank_a.max(rank_b);
                if max_rank < lowest_loser_rank {
                    lowest_loser_rank = max_rank;
                }
                let swap = match r#match.result() {
                    MatchResult::AWins => rank_a > rank_b,
                    MatchResult::BWins => rank_a < rank_b,
                    MatchResult::Draw => {
                        panic!("Draw matches are not supported in single elimination")
                    }
                    MatchResult::NotFinished => {
                        unreachable!("We ensured that all matches are finished above")
                    }
                };
                if swap {
                    ranking_id_to_swap_replaced_rank.insert(team_a_id, rank_b);
                    ranking_id_to_swap_replaced_rank.insert(team_b_id, rank_a);
                }
            }
            assert!(
                lowest_loser_rank.get() >= 2
                    && (lowest_loser_rank.get() - 1) & (lowest_loser_rank.get() - 2) == 0,
                "lowest_loser_rank - 1 must be a power of 2"
            );
            if lowest_loser_rank.get() == 2 {
                // final got played we are done
                return vec![];
            }
            let num_survived_rankings = lowest_loser_rank.get() - 1;
            let num_matches = num_survived_rankings / 2;
            let mut ordered_phase_team_ids = vec![RankingId::nil(); num_survived_rankings as usize];
            for (id, rank) in ranking_id_to_swap_replaced_rank {
                if rank.get() <= num_survived_rankings {
                    ordered_phase_team_ids[(rank.get() - 1) as usize] = id;
                }
            }
            assert!(
                ordered_phase_team_ids
                    .iter()
                    .all(|id| *id != RankingId::nil())
            );
            let round = max_round.saturating_add(1);
            (1..=num_matches)
                .map(|i| {
                    let team_a_id = ordered_phase_team_ids[(i - 1) as usize];
                    let team_b_id = ordered_phase_team_ids[(num_survived_rankings - i) as usize];
                    Match::new(vec1![team_a_id], vec1![team_b_id], round)
                })
                .collect()
        } else {
            // first round
            let num_teams = phase.rankings().len();
            // find smallest power of 2 greater or equal than num_teams
            let mut round_of = 2;
            while round_of < num_teams {
                round_of *= 2;
            }
            // 1 means final, 2 means semifinals, 4 means quarterfinals, ...
            let fraction_of_final = round_of / 2;

            ((round_of - num_teams + 1)..=fraction_of_final)
                .map(|i| {
                    let team_a_id = phase.ranking_ids_ordered_by_unique_rank()[i - 1];
                    let team_b_id = phase.ranking_ids_ordered_by_unique_rank()[round_of - i];
                    Match::new(
                        vec1![team_a_id],
                        vec1![team_b_id],
                        NonZeroU32::new(1).unwrap(),
                    )
                })
                .collect()
        }
    }
}
