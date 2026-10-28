use std::num::NonZeroU32;

use plugin_factory::{Plugin, config::PluginConfig, register};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::PhaseData,
    plugins::{
        metrics_calculator::{
            buchholz::{
                BuchholzCalculator, BuchholzOfBuchholzCalculator, MoreBuchholz,
                MoreBuchholzOfBuchholz, MoreSonnebornBerger, SonnebornBergerCalculator,
            },
            points::{MorePoints, PointsCalculator},
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
        phase_format::{PhaseFormat, Schedule, round_robin::circle::Circle},
    },
};

/// Randomly chosen rounds of a round robin, all scheduled at once.
///
/// If there are more rounds than a full round robin has, a full round robin is scheduled instead.
/// With an odd number of rankings, one ranking has a bye in every round, and no ranking has two.
#[derive(Debug, Clone, Deserialize, Serialize, Plugin)]
#[plugin(
    id = "tournament_engine::random_rounds",
    label = "random rounds",
    description = "randomly chosen rounds of a round robin",
    recommends = Self::recommends
)]
pub struct RandomRounds {
    #[plugin(
        label = "rounds",
        description = "a full round robin is scheduled if that is shorter",
        min = 1,
        default = 5
    )]
    rounds: u32,
}

impl RandomRounds {
    /// Schedules `rounds` randomly chosen rounds of a round robin.
    ///
    /// Requires the `test-util` feature.
    #[cfg(feature = "test-util")]
    pub const fn new(rounds: u32) -> Self {
        Self { rounds }
    }

    /// Ranked by points, then Buchholz, Sonneborn-Berger and Buchholz of Buchholz, and showing the
    /// wins, draws and losses as well.
    fn recommends(&self) -> Vec<PluginConfig> {
        vec![
            PluginConfig::of::<MorePoints>(),
            PluginConfig::of::<MoreBuchholz>(),
            PluginConfig::of::<MoreSonnebornBerger>(),
            PluginConfig::of::<MoreBuchholzOfBuchholz>(),
            PluginConfig::of::<WinsDrawsLossesCalculator>(),
            PluginConfig::of::<PointsCalculator>(),
            PluginConfig::of::<BuchholzCalculator>(),
            PluginConfig::of::<SonnebornBergerCalculator>(),
            PluginConfig::of::<BuchholzOfBuchholzCalculator>(),
        ]
    }
}

#[register]
impl PhaseFormat for RandomRounds {
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule {
        // all rounds are scheduled at once
        if phase_data.has_schedule() {
            return Schedule::default();
        }
        let circle = Circle::new(phase_data);
        // every rotation in an order drawn whatever the number of rounds, the first ones taken, so
        // that more rounds keep those drawn for fewer, e.g. when the phase is changed after the
        // start
        let mut rotations: Vec<_> = (0..circle.rotations()).collect();
        rotations.shuffle(&mut phase_data.rng());
        rotations.truncate(self.rounds as usize);
        let mut schedule = Schedule::default();
        for (rotation, round) in rotations.into_iter().zip(NonZeroU32::MIN..) {
            circle.schedule_round(rotation, round, &mut schedule);
        }
        schedule
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use itertools::Itertools;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;
    use crate::entities::competition::{Bye, Match, PhaseBuilder};

    fn assert_every_pair_played_at_most_once(phase: &PhaseData) {
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
        let mut phase = PhaseBuilder::new()
            .format(RandomRounds::new(3))
            .add_players(6)
            .build();

        phase.create_new_matches();

        let phase = phase.data();
        assert_every_pair_played_at_most_once(phase);
        assert_eq!(phase.matches().len(), 3 * 3);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(3).unwrap())
        );
    }

    #[test]
    fn random_rounds_schedules_number_of_rounds_with_no_match_overlaps_with_odd_players() {
        let mut phase = PhaseBuilder::new()
            .format(RandomRounds::new(3))
            .add_players(7)
            .build();

        phase.create_new_matches();

        let phase = phase.data();
        assert_every_pair_played_at_most_once(phase);
        assert_eq!(phase.matches().len(), 3 * 3);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(3).unwrap())
        );
    }

    #[test]
    fn random_rounds_with_odd_players_give_a_different_ranking_a_bye_every_round() {
        let mut phase = PhaseBuilder::new()
            .format(RandomRounds::new(3))
            .add_players(7)
            .build();

        phase.create_new_matches();

        let phase = phase.data();
        assert_eq!(
            phase
                .byes()
                .values()
                .map(|bye| bye.round().get())
                .sorted()
                .collect_vec(),
            [1, 2, 3]
        );
        assert!(phase.byes().values().map(Bye::ranking).all_unique());
    }

    #[test]
    fn random_rounds_if_rounds_is_larger_than_players_does_full_round_robin() {
        let mut phase = PhaseBuilder::new()
            .format(RandomRounds::new(8))
            .add_players(7)
            .build();

        phase.create_new_matches();

        let phase = phase.data();
        assert_every_pair_played_at_most_once(phase);
        assert_eq!(phase.matches().len(), 7 * 3);
        assert_eq!(
            phase.matches().values().map(Match::round).max(),
            Some(NonZero::new(7).unwrap())
        );
    }

    /// More rounds keep the rounds drawn for fewer, so that rounds can be added to a phase whose
    /// first ones are played.
    #[test]
    fn more_rounds_keep_the_rounds_drawn_for_fewer() {
        let drawn = |rounds| {
            let mut phase = PhaseBuilder::new()
                .format(RandomRounds::new(rounds))
                .add_players(8)
                .build();
            phase.create_new_matches();
            phase
                .data()
                .matches()
                .values()
                .map(Match::id)
                .collect::<Vec<_>>()
        };

        let (fewer, more) = (drawn(3), drawn(5));

        assert_eq!(more.len(), 5 * 4);
        assert_eq!(fewer, more[..3 * 4]);
    }
}
