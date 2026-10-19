use std::num::NonZeroU32;

use plugin_factory::{Plugin, config::PluginConfig, register};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::PhaseData,
    plugins::{
        metrics_calculator::{
            buchholz::{MoreSonnebornBerger, SonnebornBergerCalculator},
            points::{MorePoints, PointsCalculator},
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
        phase_format::{PhaseFormat, Schedule},
    },
};

mod circle;
pub mod random_rounds;

use circle::Circle;

/// Everyone plays everyone once. With an odd number of rankings, every ranking has a bye in one of
/// the rounds.
///
/// All rounds are scheduled at once, when the phase starts.
#[derive(Debug, Clone, Deserialize, Serialize, Plugin)]
#[plugin(
    id = "tournament_engine::round_robin",
    label = "round robin",
    description = "everyone plays everyone",
    recommends = Self::recommends
)]
pub struct RoundRobin;

impl RoundRobin {
    /// Ranked by points, then Sonneborn-Berger, and showing the wins, draws and losses as well.
    fn recommends(&self) -> Vec<PluginConfig> {
        vec![
            PluginConfig::of::<MorePoints>(),
            PluginConfig::of::<MoreSonnebornBerger>(),
            PluginConfig::of::<WinsDrawsLossesCalculator>(),
            PluginConfig::of::<PointsCalculator>(),
            PluginConfig::of::<SonnebornBergerCalculator>(),
        ]
    }
}

#[register]
impl PhaseFormat for RoundRobin {
    fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule {
        // all rounds are scheduled at once
        if phase_data.has_schedule() {
            return Schedule::default();
        }
        let circle = Circle::new(phase_data);
        let mut schedule = Schedule::default();
        for (rotation, round) in (0..circle.rotations()).zip(NonZeroU32::MIN..) {
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
    use crate::entities::competition::{Bye, Match, PhaseBuilder};

    /// Asserts that every unordered pair of ranking ids in the phase played against each other
    /// in exactly one scheduled match.
    fn assert_every_pair_played_exactly_once(phase: &PhaseData) {
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

        assert_every_pair_played_exactly_once(phase.data());
        assert_eq!(
            phase.data().matches().values().map(Match::round).max(),
            Some(NonZero::new(5).unwrap())
        );
    }

    #[test]
    fn round_robin_schedules_every_pair_exactly_once_for_odd_number_of_players() {
        let mut phase = PhaseBuilder::new(Box::new(RoundRobin))
            .with_players(7)
            .build();

        phase.create_new_matches();

        assert_every_pair_played_exactly_once(phase.data());
        assert_eq!(
            phase.data().matches().values().map(Match::round).max(),
            Some(NonZero::new(7).unwrap())
        );
    }

    #[test]
    fn every_ranking_has_one_bye_in_a_round_robin_of_an_odd_number_of_players() {
        let mut phase = PhaseBuilder::new(Box::new(RoundRobin))
            .with_players(7)
            .build();

        phase.create_new_matches();

        let byes = phase.data().byes();
        assert_eq!(byes.len(), 7);
        assert!(byes.values().map(Bye::ranking).all_unique());
        assert!(byes.values().map(Bye::round).all_unique());
    }

    #[test]
    fn nobody_has_a_bye_in_a_round_robin_of_an_even_number_of_players() {
        let mut phase = PhaseBuilder::new(Box::new(RoundRobin))
            .with_players(6)
            .build();

        phase.create_new_matches();

        assert!(phase.data().byes().is_empty());
    }
}
