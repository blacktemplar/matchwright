use std::num::NonZeroU32;

use plugin_factory::{Plugin, config::PluginConfig, register};
use rand::seq::{IteratorRandom, SliceRandom};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{EntropyTypes, PhaseData},
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

/// Randomly chosen rounds of a round robin, see [`RandomRounds::new`].
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
        default = 5
    )]
    rounds: u32,
}

impl RandomRounds {
    /// Schedules `rounds` randomly chosen rounds of a round robin, all at once.
    ///
    /// If `rounds` is larger than the number of rounds a full round robin has, a full round robin
    /// is scheduled instead. With an odd number of rankings, one ranking has a bye in every round,
    /// and no ranking has two.
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
        if !phase_data.matches().is_empty() || !phase_data.byes().is_empty() {
            // we generate all rounds at once, so as soon as one match or bye exists nothing is to
            // be done
            return Schedule::default();
        }
        let circle = Circle::new(phase_data);
        let mut rng = phase_data.new_seeded_std_rng(EntropyTypes::PhaseFormat);
        let mut rotations = (0..circle.rotations()).sample(&mut rng, self.rounds as usize);
        rotations.shuffle(&mut rng);
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

    use super::*;
    use crate::entities::competition::{Bye, Match, Phase, PhaseBuilder};

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
    fn random_rounds_with_odd_players_give_a_different_ranking_a_bye_every_round() {
        let mut phase = PhaseBuilder::new(Box::new(RandomRounds { rounds: 3 }))
            .with_players(7)
            .build();

        phase.create_new_matches();

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
