use std::num::NonZeroU32;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use vec1::vec1;

use crate::{
    entities::competition::{Match, MatchResult, PhaseData, RankingId},
    map::Map,
    plugins::phase_format::PhaseFormat,
    type_map::TypeMapValue,
};

/// A knockout: the loser of a match is out, so every round halves the field until the final.
///
/// Optionally the two entrants knocked out in the semifinals play one more match, for third place,
/// see [`SingleElimination::with_third_place_match`].
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SingleElimination {
    third_place_match: bool,
}

impl SingleElimination {
    /// A knockout that ends with the final, which leaves the two entrants knocked out in the
    /// semifinals as far as the bracket takes them, i.e. third equal.
    pub const fn new() -> Self {
        Self {
            third_place_match: false,
        }
    }

    /// A knockout in which the two entrants knocked out in the semifinals play for third place,
    /// scheduled in the same round as the final.
    ///
    /// Both of them lost their first match in the semifinal whichever way the match for third place
    /// goes, so [`LaterFirstLoss`] ties them and the ranking needs a further criterion to tell the
    /// two apart, e.g. [`FewerLosses`].
    ///
    /// A bracket of three entrants has nobody to play the match against and so is not given it.
    ///
    /// [`LaterFirstLoss`]: crate::plugins::metrics_calculator::round_of_first_loss::LaterFirstLoss
    /// [`FewerLosses`]: crate::plugins::metrics_calculator::wins_draws_losses::FewerLosses
    pub const fn with_third_place_match() -> Self {
        Self {
            third_place_match: true,
        }
    }
}

/// The stage of a [`SingleElimination`] knockout a match belongs to, attached to every match it
/// schedules as an annotation.
///
/// # Examples
/// ```
/// # use tournament_engine::entities::competition::Match;
/// # use tournament_engine::plugins::phase_format::single_elimination::KnockoutRound;
/// fn stage_name(r#match: &Match) -> String {
///     match r#match.annotation::<KnockoutRound>() {
///         Some(KnockoutRound::RoundOf(2)) => "final".to_owned(),
///         Some(KnockoutRound::RoundOf(4)) => "semifinal".to_owned(),
///         Some(KnockoutRound::RoundOf(n)) => format!("round of {n}"),
///         Some(KnockoutRound::ThirdPlace) => "match for third place".to_owned(),
///         None => format!("round {}", r#match.round()),
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KnockoutRound {
    /// The round that leaves this many entrants in a full bracket: 2 is the final, 4 the
    /// semifinals, 8 the quarterfinals and so on. A bye counts as a match won, so the first round
    /// of six entrants is the round of 8.
    RoundOf(u32),
    /// The match for third place, see [`SingleElimination::with_third_place_match`].
    ThirdPlace,
}

#[typetag::serde(name = "tournament_engine::knockout_round")]
impl TypeMapValue for KnockoutRound {}

impl KnockoutRound {
    /// Which fraction of the final this round is: 1 for the final, 2 for the semifinals, 4 for the
    /// quarterfinals and so on. `None` for the match for third place, which is no such fraction.
    ///
    /// # Examples
    /// ```
    /// # use tournament_engine::plugins::phase_format::single_elimination::KnockoutRound;
    /// assert_eq!(KnockoutRound::RoundOf(16).fraction_of_final(), Some(8));
    /// assert_eq!(KnockoutRound::ThirdPlace.fraction_of_final(), None);
    /// ```
    pub const fn fraction_of_final(self) -> Option<u32> {
        match self {
            Self::RoundOf(entrants) => Some(entrants / 2),
            Self::ThirdPlace => None,
        }
    }
}

#[typetag::serde]
impl PhaseFormat for SingleElimination {
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Vec<Match> {
        if !phase_data.matches().values().all(Match::is_finished) {
            return vec![];
        }
        let max_round = phase_data.matches().values().map(Match::round).max();
        if let Some(max_round) = max_round {
            // The ordering of this vector will be determined by match winners. If a lower-ranked
            // team wins, it will be swapped with the higher-ranked losing team.
            let mut ranking_id_to_swap_replaced_rank: Map<_, _> = phase_data
                .rankings()
                .iter()
                .map(|(id, r)| (*id, r.initial_unique_rank()))
                .collect();
            // matches are ordered by round, which is important so that the following works:
            let mut lowest_loser_rank = NonZeroU32::MAX;
            let matches_odered_by_round =
                phase_data.matches().values().sorted_by_key(|m| m.round());
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
                        unreachable!("draws are rejected since this format does not allow them")
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
            // The two teams knocked out in the semifinals end up at the two ranks right below the
            // two finalists, so the match for third place is the one scheduled alongside the
            // final. A bracket of three has nobody at the fourth rank to play it against.
            let plays_for_third_place = self.third_place_match
                && num_survived_rankings == 2
                && phase_data.rankings().len() >= 4;
            // the two ranks of the match for third place are ordered here as well, which is what
            // makes it a match between them rather than a swap-replaced rank looked up twice
            let num_ordered_rankings = if plays_for_third_place {
                4
            } else {
                num_survived_rankings
            };
            let mut ordered_phase_team_ids = vec![RankingId::nil(); num_ordered_rankings as usize];
            for (id, rank) in ranking_id_to_swap_replaced_rank {
                if rank.get() <= num_ordered_rankings {
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
                        .with_annotation(KnockoutRound::RoundOf(num_survived_rankings))
                })
                .chain(plays_for_third_place.then(|| {
                    Match::new(
                        vec1![ordered_phase_team_ids[2]],
                        vec1![ordered_phase_team_ids[3]],
                        round,
                    )
                    .with_annotation(KnockoutRound::ThirdPlace)
                }))
                .collect()
        } else {
            // first round
            let num_teams = phase_data.rankings().len();
            // find smallest power of 2 greater or equal than num_teams
            let mut round_of = 2;
            while round_of < num_teams {
                round_of *= 2;
            }
            // 1 means final, 2 means semifinals, 4 means quarterfinals, ...
            let fraction_of_final = round_of / 2;

            ((round_of - num_teams + 1)..=fraction_of_final)
                .map(|i| {
                    let team_a_id = phase_data.ranking_ids_ordered_by_unique_rank()[i - 1];
                    let team_b_id = phase_data.ranking_ids_ordered_by_unique_rank()[round_of - i];
                    Match::new(
                        vec1![team_a_id],
                        vec1![team_b_id],
                        NonZeroU32::new(1).unwrap(),
                    )
                    .with_annotation(KnockoutRound::RoundOf(round_of as u32))
                })
                .collect()
        }
    }

    /// Every match has to produce a winner that advances to the next round.
    fn allows_draws(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::competition::{Phase, PhaseBuilder};

    /// A knockout between `num_players` entrants with its first round already scheduled.
    fn knockout(format: SingleElimination, num_players: u32) -> Phase {
        let mut phase = PhaseBuilder::new(Box::new(format))
            .with_players(num_players)
            .build();
        phase.create_new_matches();
        phase
    }

    /// The matches of `round`, each as the pair of initial unique ranks it is played between, in
    /// ascending order so that the pairings of a round can be compared as a whole.
    fn pairings(phase: &Phase, round: u32) -> Vec<(u32, u32)> {
        let rank_of = |id: &RankingId| phase.rankings()[id].initial_unique_rank().get();
        phase
            .matches()
            .values()
            .filter(|r#match| r#match.round().get() == round)
            .map(|r#match| (rank_of(&r#match.team_a()[0]), rank_of(&r#match.team_b()[0])))
            .sorted()
            .collect()
    }

    /// Reports every match of `round` as won by the side that entered the bracket higher, i.e.
    /// lets the favourites advance, and schedules whatever follows from that.
    fn play_round(phase: &mut Phase, round: u32) {
        let match_ids = phase
            .matches()
            .values()
            .filter(|r#match| r#match.round().get() == round)
            .map(Match::id)
            .collect_vec();
        assert!(!match_ids.is_empty(), "round {round} has no matches");
        for match_id in match_ids {
            phase.report_match_result(match_id, MatchResult::AWins);
        }
        phase.create_new_matches();
    }

    #[test]
    fn a_knockout_ends_with_the_final_unless_it_plays_for_third_place() {
        let mut phase = knockout(SingleElimination::new(), 4);

        assert_eq!(pairings(&phase, 1), [(1, 4), (2, 3)]);
        play_round(&mut phase, 1);

        assert_eq!(pairings(&phase, 2), [(1, 2)], "the final, and nothing else");
        play_round(&mut phase, 2);
        assert_eq!(phase.matches().len(), 3);
    }

    #[test]
    fn the_match_for_third_place_is_scheduled_alongside_the_final() {
        let mut phase = knockout(SingleElimination::with_third_place_match(), 4);

        play_round(&mut phase, 1);

        assert_eq!(
            pairings(&phase, 2),
            [(1, 2), (3, 4)],
            "the two losers of the semifinals play in the round of the final"
        );
        play_round(&mut phase, 2);
        assert_eq!(phase.matches().len(), 4, "the bracket is over afterwards");
    }

    /// The match is played between whoever lost the semifinals, not between whoever entered the
    /// bracket third and fourth, which is what a bracket the favourites lose shows.
    #[test]
    fn the_losers_of_the_semifinals_play_for_third_place() {
        let mut phase = knockout(SingleElimination::with_third_place_match(), 4);
        for match_id in phase.matches().values().map(Match::id).collect_vec() {
            // the lower ranked side wins both semifinals, so 1 and 2 are the ones knocked out
            phase.report_match_result(match_id, MatchResult::BWins);
        }

        phase.create_new_matches();

        assert_eq!(
            pairings(&phase, 2),
            [(2, 1), (4, 3)],
            "4 and 3 advanced to the final, so 2 and 1 are the ones playing for third place"
        );
    }

    /// A bracket of three gives one entrant a bye into the final, so only one of the three gets
    /// knocked out before it and there is nobody for it to play against.
    #[test]
    fn three_entrants_do_not_play_for_third_place() {
        let mut phase = knockout(SingleElimination::with_third_place_match(), 3);

        assert_eq!(pairings(&phase, 1), [(2, 3)]);
        play_round(&mut phase, 1);

        assert_eq!(pairings(&phase, 2), [(1, 2)]);
        play_round(&mut phase, 2);
        assert_eq!(phase.matches().len(), 2);
    }

    /// The stages of the matches of `round`, in the order the pairings of [`pairings`] list them.
    fn stages(phase: &Phase, round: u32) -> Vec<KnockoutRound> {
        let rank_of = |id: &RankingId| phase.rankings()[id].initial_unique_rank().get();
        phase
            .matches()
            .values()
            .filter(|r#match| r#match.round().get() == round)
            .sorted_by_key(|r#match| (rank_of(&r#match.team_a()[0]), rank_of(&r#match.team_b()[0])))
            .map(|r#match| *r#match.annotation::<KnockoutRound>().unwrap())
            .collect()
    }

    /// The first round is named after the full bracket it fills, byes included, and the final
    /// shares its round with the match for third place but not its stage.
    #[test]
    fn every_match_is_annotated_with_its_stage() {
        let mut phase = knockout(SingleElimination::with_third_place_match(), 6);
        assert_eq!(stages(&phase, 1), [KnockoutRound::RoundOf(8); 2]);

        play_round(&mut phase, 1);
        assert_eq!(stages(&phase, 2), [KnockoutRound::RoundOf(4); 2]);

        play_round(&mut phase, 2);
        assert_eq!(
            stages(&phase, 3),
            [KnockoutRound::RoundOf(2), KnockoutRound::ThirdPlace]
        );
    }

    /// The tag is part of the serialized form of every snapshot ever taken, so a change to it is a
    /// breaking one and has to be deliberate.
    #[test]
    fn a_stage_is_serialized_under_its_explicitly_chosen_tag() {
        let phase = knockout(SingleElimination::new(), 2);
        let the_final = phase.matches().values().next().unwrap();

        let value = serde_json::to_value(the_final).expect("serializing must not fail");

        assert_eq!(
            value["annotations"],
            serde_json::json!([{
                "type": "tournament_engine::knockout_round",
                "value": { "RoundOf": 2 }
            }])
        );
    }

    /// Two entrants play nothing but the final, so the option changes nothing at all.
    #[test]
    fn two_entrants_only_play_the_final() {
        let mut phase = knockout(SingleElimination::with_third_place_match(), 2);

        assert_eq!(pairings(&phase, 1), [(1, 2)]);
        play_round(&mut phase, 1);

        assert_eq!(phase.matches().len(), 1);
    }

    /// Only the semifinals decide who plays for third place, so the earlier rounds of a larger
    /// bracket are scheduled exactly as they are without the option, byes included.
    #[test]
    fn a_larger_bracket_only_gains_the_one_match() {
        let mut phase = knockout(SingleElimination::with_third_place_match(), 6);

        assert_eq!(pairings(&phase, 1), [(3, 6), (4, 5)]);
        play_round(&mut phase, 1);

        assert_eq!(pairings(&phase, 2), [(1, 4), (2, 3)]);
        play_round(&mut phase, 2);

        assert_eq!(pairings(&phase, 3), [(1, 2), (3, 4)]);
        play_round(&mut phase, 3);
        assert_eq!(phase.matches().len(), 6);
    }
}
