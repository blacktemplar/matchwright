use std::{any::TypeId, collections::HashSet};

use crate::{
    entities::competition::{MatchId, MatchResult, Phase, RankingId},
    plugins::metrics_calculator::{Metric, MetricId, MetricsCalculator},
};

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct WinsDrawsLosses {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct WinsDrawsLossesCalculator;

impl MetricsCalculator for WinsDrawsLossesCalculator {
    fn produced_metrics(&self) -> HashSet<MetricId> {
        [TypeId::of::<WinsDrawsLosses>()].into_iter().collect()
    }

    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        match_id: MatchId,
        old_match_result: MatchResult,
    ) -> Vec<(RankingId, Metric)> {
        // for now we only support setting the match result, not updating it
        assert_eq!(old_match_result, MatchResult::NotFinished);

        let r#match = phase.matches().get(&match_id).unwrap();
        assert!(r#match.is_finished());
        let (winning_team, losing_team) = match r#match.result() {
            MatchResult::AWins => (r#match.team_a(), r#match.team_b()),
            MatchResult::BWins => (r#match.team_b(), r#match.team_a()),
            MatchResult::Draw => {
                return r#match
                    .rankings()
                    .map(|r_id| {
                        let mut metric =
                            *phase.rankings()[r_id].metric::<WinsDrawsLosses>().unwrap();
                        metric.draws += 1;
                        (*r_id, Box::new(metric) as Metric)
                    })
                    .collect();
            }
            _ => unreachable!("We expect the match to be finished above"),
        };
        winning_team
            .iter()
            .map(|r_id| {
                let mut metric = *phase.rankings()[r_id].metric::<WinsDrawsLosses>().unwrap();
                metric.wins += 1;
                (*r_id, Box::new(metric) as Metric)
            })
            .chain(losing_team.iter().map(|r_id| {
                let mut metric = *phase.rankings()[r_id].metric::<WinsDrawsLosses>().unwrap();
                metric.losses += 1;
                (*r_id, Box::new(metric) as Metric)
            }))
            .collect()
    }

    fn initialize_metrics(&self, phase: &Phase) -> Vec<(RankingId, Metric)> {
        phase
            .rankings()
            .keys()
            .map(|ranking_id| (*ranking_id, Box::new(WinsDrawsLosses::default()) as Metric))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::metrics_calculator::test_utils::{self, MatchSpec, match_id};

    /// Builds a phase with `num_players` rankings whose wins/draws/losses got initialized and
    /// with all matches of `matches` already scheduled.
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(
            num_players,
            matches,
            vec![Box::new(WinsDrawsLossesCalculator)],
        )
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> WinsDrawsLosses {
        test_utils::metric_of(phase, ranking_index)
    }

    const fn wins_draws_losses(wins: u32, draws: u32, losses: u32) -> WinsDrawsLosses {
        WinsDrawsLosses {
            wins,
            draws,
            losses,
        }
    }

    #[test]
    fn produces_exactly_the_wins_draws_losses_metric() {
        assert_eq!(
            WinsDrawsLossesCalculator.produced_metrics(),
            [TypeId::of::<WinsDrawsLosses>()].into_iter().collect()
        );
    }

    #[test]
    fn initialize_metrics_starts_every_ranking_at_zero() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), wins_draws_losses(0, 0, 0));
        }
    }

    #[test]
    fn a_wins_counts_as_win_for_team_a_and_as_loss_for_team_b() {
        let mut phase = phase_with(4, vec![(1, vec![0], vec![1]), (1, vec![2], vec![3])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(1, 0, 0));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(0, 0, 1));
        // the rankings of the other match are untouched
        assert_eq!(metric_of(&phase, 2), wins_draws_losses(0, 0, 0));
        assert_eq!(metric_of(&phase, 3), wins_draws_losses(0, 0, 0));
    }

    #[test]
    fn b_wins_counts_as_win_for_team_b_and_as_loss_for_team_a() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(0, 0, 1));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(1, 0, 0));
    }

    #[test]
    fn draw_counts_as_draw_for_both_teams() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::Draw);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(0, 1, 0));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(0, 1, 0));
    }

    #[test]
    fn every_ranking_of_a_team_gets_the_result_of_the_team() {
        let mut phase = phase_with(4, vec![(1, vec![0, 1], vec![2, 3])]);
        let match_id = match_id(&phase, 1, &[0, 1], &[2, 3]);

        phase.report_match_result(match_id, MatchResult::BWins);

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(0, 0, 1));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(0, 0, 1));
        assert_eq!(metric_of(&phase, 2), wins_draws_losses(1, 0, 0));
        assert_eq!(metric_of(&phase, 3), wins_draws_losses(1, 0, 0));
    }

    #[test]
    fn results_accumulate_over_multiple_matches() {
        let mut phase = phase_with(
            4,
            vec![
                (1, vec![0], vec![1]),
                (1, vec![2], vec![3]),
                (2, vec![0], vec![2]),
                (2, vec![1], vec![3]),
                (3, vec![1], vec![2]),
            ],
        );

        for (round, team_a, team_b, result) in [
            (1, vec![0], vec![1], MatchResult::AWins),
            (1, vec![2], vec![3], MatchResult::Draw),
            (2, vec![0], vec![2], MatchResult::BWins),
            (2, vec![1], vec![3], MatchResult::AWins),
            (3, vec![1], vec![2], MatchResult::BWins),
        ] {
            let match_id = match_id(&phase, round, &team_a, &team_b);
            phase.report_match_result(match_id, result);
        }

        assert_eq!(metric_of(&phase, 0), wins_draws_losses(1, 0, 1));
        assert_eq!(metric_of(&phase, 1), wins_draws_losses(1, 0, 2));
        assert_eq!(metric_of(&phase, 2), wins_draws_losses(2, 1, 0));
        assert_eq!(metric_of(&phase, 3), wins_draws_losses(0, 1, 1));
    }

    #[test]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn updating_an_already_reported_result_is_unsupported() {
        let mut phase = phase_with(2, vec![(1, vec![0], vec![1])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);
        phase.report_match_result(match_id, MatchResult::AWins);

        WinsDrawsLossesCalculator.update_after_match_result_changed(
            &phase,
            match_id,
            MatchResult::AWins,
        );
    }
}
