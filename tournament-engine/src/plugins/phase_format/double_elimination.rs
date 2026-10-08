use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{PhaseData, Team},
    plugins::{
        compatibility::Declaration,
        metrics_calculator::round_of_elimination::Eliminates,
        phase_format::{
            PhaseFormat, Schedule,
            bracket::{Bracket, Source},
        },
    },
    type_map::{TypeMap, TypeMapValue},
};

/// A knockout in which an entrant is only out after its second loss: the winner bracket plays like
/// a single elimination, and whoever loses there drops into the loser bracket. The champions of
/// both brackets meet in the grand final.
///
/// The loser bracket takes in the losers of every round of the winner bracket. So it plays two
/// rounds for every round of the winner bracket after the first: one in which its own survivors
/// play each other, and one in which they meet the entrants that just dropped down. In a full
/// bracket of `2^k` entrants that is `k` rounds of the winner bracket and `2k - 2` of the loser
/// bracket. The rounds of a phase interleave them, so that every entrant plays its matches in the
/// order of their rounds:
///
/// | round          | 1 | 2 | 3 | 4 | 5 | … | 2k          | 2k + 1 |
/// |----------------|---|---|---|---|---|---|-------------|--------|
/// | winner bracket | 1 | 2 |   | 3 |   | … |             |        |
/// | loser bracket  |   | 1 | 2 | 3 | 4 | … |             |        |
/// | grand final    |   |   |   |   |   |   | grand final | reset  |
///
/// A match is scheduled as soon as both of its entrants are known, without waiting for the rest of
/// its round. Seeding follows the initial ranks: in the first round the best seed plays the worst,
/// and the best seeds get the byes of a bracket that is not full, as in a [`SingleElimination`].
/// Entrants dropping down from the winner bracket enter the loser bracket in reversed and in
/// original order, alternately, which keeps those apart that met in the winner bracket for longer.
///
/// Every match is annotated with its [`DoubleEliminationRound`] and every match that can knock an
/// entrant out with [`Eliminates`]. Draws are not allowed.
///
/// [`SingleElimination`]: super::single_elimination::SingleElimination
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DoubleElimination {
    grand_final_reset: bool,
    /// Built from the seeds on the first call.
    bracket: Option<Bracket>,
}

impl DoubleElimination {
    /// A double elimination whose grand final is a single match: whoever loses it is out, even the
    /// champion of the winner bracket with its first loss.
    pub const fn new() -> Self {
        Self {
            grand_final_reset: false,
            bracket: None,
        }
    }

    /// A double elimination in which the champion of the winner bracket also needs to lose twice:
    /// if it loses the grand final, the two play it once more.
    pub const fn with_grand_final_reset() -> Self {
        Self {
            grand_final_reset: true,
            bracket: None,
        }
    }
}

/// The stage of a [`DoubleElimination`] a match belongs to, attached to every match it schedules as
/// an annotation.
///
/// # Examples
/// ```
/// # use tournament_engine::entities::competition::Match;
/// # use tournament_engine::plugins::phase_format::double_elimination::DoubleEliminationRound;
/// fn stage_name(r#match: &Match) -> String {
///     match r#match.annotation::<DoubleEliminationRound>() {
///         Some(DoubleEliminationRound::Winners { round, rounds }) if round == rounds => {
///             "winner bracket final".to_owned()
///         }
///         Some(DoubleEliminationRound::Winners { round, .. }) => format!("winners {round}"),
///         Some(DoubleEliminationRound::Losers { round, .. }) => format!("losers {round}"),
///         Some(DoubleEliminationRound::GrandFinal) => "grand final".to_owned(),
///         Some(DoubleEliminationRound::GrandFinalReset) => "grand final reset".to_owned(),
///         None => format!("round {}", r#match.round()),
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DoubleEliminationRound {
    /// Round `round` of the `rounds` rounds of the winner bracket, the last one being its final.
    Winners { round: u32, rounds: u32 },
    /// Round `round` of the `rounds` rounds of the loser bracket, the last one being its final.
    Losers { round: u32, rounds: u32 },
    /// The match between the champions of both brackets.
    GrandFinal,
    /// The grand final played once more, because the champion of the winner bracket lost it, see
    /// [`DoubleElimination::with_grand_final_reset`].
    GrandFinalReset,
}

#[typetag::serde(name = "tournament_engine::double_elimination_round")]
impl TypeMapValue for DoubleEliminationRound {}

#[typetag::serde]
impl PhaseFormat for DoubleElimination {
    fn declare(&self, _: &mut Declaration) {}

    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule {
        let reset = self.grand_final_reset;
        self.bracket
            .get_or_insert_with(|| bracket(phase_data, reset))
            .create_new_matches(phase_data)
    }

    /// Every match has to produce a winner that advances.
    fn allows_draws(&self) -> bool {
        false
    }
}

/// The whole double elimination between the rankings of `phase_data`, see [`DoubleElimination`].
fn bracket(phase_data: &PhaseData, grand_final_reset: bool) -> Bracket {
    let mut bracket = Bracket::default();
    let winners = bracket.add_knockout(phase_data, winner_bracket_slot, |round, rounds| {
        annotations(DoubleEliminationRound::Winners { round, rounds }, None)
    });
    let Some(winner_bracket_final) = winners.last() else {
        return bracket;
    };
    let winner_rounds = winners.len() as u32;
    let loser_rounds = 2 * winner_rounds - 2;

    let mut losers: Vec<Source> = winners[0].iter().copied().map(Source::LoserOf).collect();
    for round in 1..=loser_rounds {
        let pairs: Vec<[Source; 2]> = if round % 2 == 1 {
            losers.as_chunks().0.to_vec()
        } else {
            // the losers of the next round of the winner bracket drop in
            let drop_in = round / 2;
            let mut dropping: Vec<_> = winners[drop_in as usize]
                .iter()
                .copied()
                .map(Source::LoserOf)
                .collect();
            if drop_in % 2 == 1 {
                dropping.reverse();
            }
            losers
                .into_iter()
                .zip(dropping)
                .map(<[_; 2]>::from)
                .collect()
        };
        let stage = DoubleEliminationRound::Losers {
            round,
            rounds: loser_rounds,
        };
        losers = pairs
            .into_iter()
            .map(|sources| {
                let annotations = annotations(stage, Some(Eliminates::Loser));
                Source::WinnerOf(bracket.add_pairing(slot(round + 1), sources, annotations))
            })
            .collect();
    }

    let grand_final_slot = slot(2 * winner_rounds);
    let grand_final = bracket.add_pairing(
        grand_final_slot,
        [Source::WinnerOf(winner_bracket_final[0]), losers[0]],
        annotations(
            DoubleEliminationRound::GrandFinal,
            Some(if grand_final_reset {
                Eliminates::Only(Team::B)
            } else {
                Eliminates::Loser
            }),
        ),
    );
    if grand_final_reset {
        // only played if the champion of the loser bracket, side B, won the grand final, and then
        // between the same two in the same order
        bracket.add_pairing(
            grand_final_slot.saturating_add(1),
            [Source::LoserOf(grand_final), Source::WinnerOf(grand_final)],
            annotations(
                DoubleEliminationRound::GrandFinalReset,
                Some(Eliminates::Loser),
            ),
        );
        bracket.advance_only_if_won_by(grand_final, Team::B);
    }
    bracket
}

fn annotations(stage: DoubleEliminationRound, eliminates: Option<Eliminates>) -> TypeMap {
    let mut annotations = TypeMap::new();
    annotations.insert(stage);
    if let Some(eliminates) = eliminates {
        annotations.insert(eliminates);
    }
    annotations
}

/// The phase round that round `round` of the winner bracket is played in, see the table of
/// [`DoubleElimination`]. Round `i` of the loser bracket is played in phase round `i + 1`.
fn winner_bracket_slot(round: u32) -> NonZeroU32 {
    slot(if round == 1 { 1 } else { 2 * round - 2 })
}

fn slot(round: u32) -> NonZeroU32 {
    NonZeroU32::new(round).expect("rounds are counted from 1")
}

#[cfg(test)]
mod tests {
    use DoubleEliminationRound::{GrandFinal, GrandFinalReset, Losers, Winners};
    use itertools::Itertools;

    use super::*;
    use crate::{
        entities::competition::{Match, MatchId, MatchResult, Phase, PhaseBuilder, RankingId},
        plugins::{
            metrics_calculator::{
                round_of_elimination::{
                    LaterElimination, RoundOfElimination, RoundOfEliminationCalculator,
                },
                wins_draws_losses::{FewerLosses, WinsDrawsLosses, WinsDrawsLossesCalculator},
            },
            ranker::{HierarchicalRanker, LexicographicalCriteria},
        },
    };

    /// A double elimination between `num_players` entrants with its first matches scheduled,
    /// ranked by the round of elimination and then by the losses.
    fn bracket(format: DoubleElimination, num_players: u32) -> Phase {
        let criteria = LexicographicalCriteria::new()
            .add_criterion(LaterElimination)
            .add_criterion(FewerLosses);
        let mut phase = PhaseBuilder::new(Box::new(format))
            .with_players(num_players)
            .metrics_calculator(Box::new(RoundOfEliminationCalculator))
            .metrics_calculator(Box::new(WinsDrawsLossesCalculator))
            .ranker(Box::new(HierarchicalRanker::new(vec![Box::new(criteria)])))
            .build();
        phase.create_new_matches();
        phase
    }

    fn seed_of(phase: &Phase, ranking_id: &RankingId) -> u32 {
        phase.rankings()[ranking_id].initial_unique_rank().get()
    }

    /// Every match as its round, its stage and the seeds of its two sides, in the order the
    /// matches were scheduled in.
    fn schedule(phase: &Phase) -> Vec<(u32, DoubleEliminationRound, (u32, u32))> {
        phase
            .matches()
            .values()
            .map(|r#match| {
                (
                    r#match.round().get(),
                    *r#match.annotation::<DoubleEliminationRound>().unwrap(),
                    (
                        seed_of(phase, &r#match.team_a()[0]),
                        seed_of(phase, &r#match.team_b()[0]),
                    ),
                )
            })
            .collect()
    }

    /// Reports the open matches one at a time, in the order they were scheduled in, as won by the
    /// side `winner` picks given the match and the seeds of its sides, until none is left.
    fn play_out(phase: &mut Phase, winner: impl Fn(&Match, u32, u32) -> Team) {
        while let Some(r#match) = phase.matches().values().find(|m| !m.is_finished()) {
            let seeds = (
                seed_of(phase, &r#match.team_a()[0]),
                seed_of(phase, &r#match.team_b()[0]),
            );
            let result = match winner(r#match, seeds.0, seeds.1) {
                Team::A => MatchResult::AWins,
                Team::B => MatchResult::BWins,
            };
            phase.report_match_result(r#match.id(), result);
        }
    }

    fn favourite(_: &Match, seed_a: u32, seed_b: u32) -> Team {
        if seed_a < seed_b { Team::A } else { Team::B }
    }

    /// The favourites win everything but the grand final.
    fn underdog_wins_the_grand_final(r#match: &Match, seed_a: u32, seed_b: u32) -> Team {
        match r#match.annotation::<DoubleEliminationRound>() {
            Some(GrandFinal) => Team::B,
            _ => favourite(r#match, seed_a, seed_b),
        }
    }

    /// The rank and the round of elimination of every entrant, ordered by seed.
    fn standings(phase: &Phase) -> Vec<(u32, Option<u32>)> {
        phase
            .rankings()
            .values()
            .sorted_by_key(|ranking| ranking.initial_unique_rank())
            .map(|ranking| {
                let eliminated = ranking.metric::<RoundOfElimination>().unwrap().0;
                (ranking.rank().get(), eliminated.map(NonZeroU32::get))
            })
            .collect()
    }

    #[test]
    fn four_entrants_play_through_both_brackets_into_the_grand_final() {
        let mut phase = bracket(DoubleElimination::new(), 4);

        play_out(&mut phase, favourite);

        let winners = |round| Winners { round, rounds: 2 };
        let losers = |round| Losers { round, rounds: 2 };
        assert_eq!(
            schedule(&phase),
            [
                (1, winners(1), (1, 4)),
                (1, winners(1), (2, 3)),
                (2, winners(2), (1, 2)),
                (2, losers(1), (4, 3)),
                (3, losers(2), (3, 2)),
                (4, GrandFinal, (1, 2)),
            ]
        );
        assert_eq!(
            standings(&phase),
            [(1, None), (2, Some(4)), (3, Some(3)), (4, Some(2))]
        );
    }

    /// The loser bracket knocks out two entrants per round of the winner bracket, which ties the
    /// two knocked out in the same round.
    #[test]
    fn entrants_knocked_out_in_the_same_round_share_their_rank() {
        let mut phase = bracket(DoubleElimination::new(), 8);

        play_out(&mut phase, favourite);

        assert_eq!(
            standings(&phase),
            [
                (1, None),
                (2, Some(6)),
                (3, Some(5)),
                (4, Some(4)),
                (5, Some(3)),
                (5, Some(3)),
                (7, Some(2)),
                (7, Some(2)),
            ]
        );
    }

    #[test]
    fn without_a_reset_the_grand_final_knocks_out_its_loser() {
        let mut phase = bracket(DoubleElimination::new(), 4);

        play_out(&mut phase, underdog_wins_the_grand_final);

        assert_eq!(phase.matches().len(), 6, "nothing follows the grand final");
        assert_eq!(
            standings(&phase)[..2],
            [(2, Some(4)), (1, None)],
            "the champion of the winner bracket is out after its first loss"
        );
    }

    #[test]
    fn with_a_reset_the_champion_of_the_winner_bracket_needs_to_lose_twice() {
        let mut phase = bracket(DoubleElimination::with_grand_final_reset(), 4);

        play_out(&mut phase, underdog_wins_the_grand_final);

        assert_eq!(
            schedule(&phase)[5..],
            [(4, GrandFinal, (1, 2)), (5, GrandFinalReset, (1, 2))]
        );
        assert_eq!(
            standings(&phase)[..2],
            [(1, None), (2, Some(5))],
            "the favourite lost the grand final but won the reset"
        );
    }

    #[test]
    fn a_reset_is_not_played_if_the_champion_of_the_winner_bracket_wins_the_grand_final() {
        let mut phase = bracket(DoubleElimination::with_grand_final_reset(), 4);

        play_out(&mut phase, favourite);

        assert_eq!(phase.matches().len(), 6);
    }

    /// With a reset to come, losing the grand final only knocks out the champion of the loser
    /// bracket, which is side B.
    #[test]
    fn the_grand_final_eliminates_according_to_the_reset() {
        for (format, eliminates) in [
            (DoubleElimination::new(), Eliminates::Loser),
            (
                DoubleElimination::with_grand_final_reset(),
                Eliminates::Only(Team::B),
            ),
        ] {
            let mut phase = bracket(format, 2);
            let first = *phase.matches().keys().next().unwrap();
            phase.report_match_result(first, MatchResult::AWins);

            let grand_final = phase.matches().values().last().unwrap();

            assert_eq!(grand_final.annotation(), Some(&GrandFinal));
            assert_eq!(grand_final.annotation(), Some(&eliminates));
        }
    }

    /// A match of the next round is scheduled as soon as the two matches it follows from are
    /// decided, while the rest of the round is still being played.
    #[test]
    fn a_match_is_scheduled_as_soon_as_both_of_its_entrants_are_known() {
        let mut phase = bracket(DoubleElimination::new(), 8);
        let first_two: Vec<MatchId> = phase.matches().keys().take(2).copied().collect();

        for match_id in first_two {
            phase.report_match_result(match_id, MatchResult::AWins);
        }

        assert_eq!(
            schedule(&phase)[4..],
            [
                (
                    2,
                    Winners {
                        round: 2,
                        rounds: 3
                    },
                    (1, 4)
                ),
                (
                    2,
                    Losers {
                        round: 1,
                        rounds: 4
                    },
                    (8, 5)
                ),
            ]
        );
    }

    /// Where the bracket is not full, the best seeds advance without a match, and two of them that
    /// meet next can play right away.
    #[test]
    fn the_best_seeds_get_the_byes() {
        let phase = bracket(DoubleElimination::new(), 5);

        let winners = |round| Winners { round, rounds: 3 };
        assert_eq!(
            schedule(&phase),
            [(1, winners(1), (4, 5)), (2, winners(2), (2, 3))]
        );
    }

    /// The bye of the winner bracket leaves nobody to drop down, so whoever drops down to meet
    /// nobody has a bye in the loser bracket as well.
    #[test]
    fn a_bye_of_the_winner_bracket_leads_to_one_in_the_loser_bracket() {
        let mut phase = bracket(DoubleElimination::new(), 3);

        play_out(&mut phase, favourite);

        let byes = phase
            .byes()
            .values()
            .map(|bye| {
                (
                    bye.round().get(),
                    *bye.annotation::<DoubleEliminationRound>().unwrap(),
                    seed_of(&phase, &bye.ranking()),
                )
            })
            .collect_vec();
        assert_eq!(
            byes,
            [
                (
                    1,
                    Winners {
                        round: 1,
                        rounds: 2
                    },
                    1
                ),
                (
                    2,
                    Losers {
                        round: 1,
                        rounds: 2
                    },
                    3
                ),
            ]
        );
    }

    /// Whatever the results, every entrant but the champion is knocked out, after its second loss
    /// unless it lost a grand final that is not reset, and plays its matches in the order of
    /// their rounds.
    #[test]
    fn every_bracket_size_plays_out_to_one_champion() {
        // an arbitrary but fixed mix of favourites and underdogs winning
        let mixed = |r#match: &Match, seed_a: u32, seed_b: u32| {
            if (seed_a * 7 + seed_b * 13 + r#match.round().get()).is_multiple_of(3) {
                Team::B
            } else {
                Team::A
            }
        };
        for (reset, num_players) in [false, true].into_iter().cartesian_product(2..=20) {
            let format = if reset {
                DoubleElimination::with_grand_final_reset()
            } else {
                DoubleElimination::new()
            };
            let mut phase = bracket(format, num_players);

            play_out(&mut phase, mixed);

            let context = format!("{num_players} entrants, reset: {reset}");
            let reset_played = phase
                .matches()
                .values()
                .any(|r#match| r#match.annotation() == Some(&GrandFinalReset));
            assert_eq!(
                phase.matches().len() as u32,
                2 * num_players - 2 + u32::from(reset_played),
                "{context}"
            );
            let (still_in, knocked_out): (Vec<_>, Vec<_>) = phase
                .rankings()
                .values()
                .partition(|ranking| ranking.metric::<RoundOfElimination>().unwrap().0.is_none());
            assert_eq!(still_in.len(), 1, "{context}");
            let losses: Vec<u32> = knocked_out
                .iter()
                .map(|ranking| ranking.metric::<WinsDrawsLosses>().unwrap().losses)
                .sorted()
                .collect();
            let out_after_one_loss = losses.iter().filter(|losses| **losses == 1).count();
            assert!(
                losses.iter().all(|losses| (1..=2).contains(losses))
                    && out_after_one_loss <= usize::from(!reset),
                "{context}: {losses:?}"
            );
            for ranking_id in phase.rankings().keys() {
                let rounds: Vec<_> = phase
                    .matches()
                    .values()
                    .filter(|r#match| r#match.rankings().contains(ranking_id))
                    .map(Match::round)
                    .collect();
                assert!(
                    rounds.is_sorted() && rounds.iter().all_unique(),
                    "{context}"
                );
            }
        }
    }

    #[test]
    fn the_stage_is_serialized_under_its_explicitly_chosen_tag() {
        let phase = bracket(DoubleElimination::new(), 2);
        let first = phase.matches().values().next().unwrap();

        let value = serde_json::to_value(first).expect("serializing must not fail");

        assert_eq!(
            value["annotations"],
            serde_json::json!([{
                "type": "tournament_engine::double_elimination_round",
                "value": { "Winners": { "round": 1, "rounds": 1 } }
            }])
        );
    }
}
