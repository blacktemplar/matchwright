use std::any::TypeId;

use serde::{Deserialize, Serialize};

use super::Metric;
use crate::{
    entities::competition::{Match, MatchId, MatchResult, PhaseData, RankingId},
    map::Map,
    plugins::metrics_calculator::MetricsCalculator,
    type_map::TypeMapValue,
};

#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Matches(pub Vec<MatchId>);

#[typetag::serde]
impl TypeMapValue for Matches {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MatchesCalculator;

#[typetag::serde]
impl MetricsCalculator for MatchesCalculator {
    fn default_metric_value(&self) -> Metric {
        Box::new(Matches::default())
    }

    fn update_after_match_result_changed(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        _: MatchResult,
        _: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)> {
        r#match
            .rankings()
            .map(|r_id| {
                let old_matches = phase_data.rankings()[r_id].metric::<Matches>().unwrap();
                let mut new_matches = Matches(old_matches.0.clone());
                new_matches.0.push(r#match.id());
                (*r_id, Box::new(new_matches) as Metric)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::competition::Phase,
        plugins::metrics_calculator::test_utils::{self, MatchSpec, match_id},
    };

    /// Builds a phase with `num_players` rankings whose played matches got initialized and with
    /// all matches of `matches` already scheduled.
    fn phase_with(num_players: u32, matches: Vec<MatchSpec>) -> Phase {
        test_utils::phase_with(num_players, matches, vec![Box::new(MatchesCalculator)])
    }

    fn metric_of(phase: &Phase, ranking_index: usize) -> &Matches {
        test_utils::metric_ref_of(phase, ranking_index)
    }

    #[test]
    fn initialize_metrics_starts_every_ranking_without_matches() {
        let phase = phase_with(4, vec![]);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), &Matches(vec![]));
        }
    }

    #[test]
    fn a_reported_match_is_recorded_for_the_rankings_of_both_teams() {
        let mut phase = phase_with(4, vec![(1, vec![0], vec![1]), (1, vec![2], vec![3])]);
        let match_id = match_id(&phase, 1, &[0], &[1]);

        phase.report_match_result(match_id, MatchResult::AWins);

        assert_eq!(metric_of(&phase, 0), &Matches(vec![match_id]));
        assert_eq!(metric_of(&phase, 1), &Matches(vec![match_id]));
        // the rankings of the other, still unreported match are untouched
        assert_eq!(metric_of(&phase, 2), &Matches(vec![]));
        assert_eq!(metric_of(&phase, 3), &Matches(vec![]));
    }

    #[test]
    fn every_ranking_of_a_team_records_the_match_of_the_team() {
        let mut phase = phase_with(4, vec![(1, vec![0, 1], vec![2, 3])]);
        let match_id = match_id(&phase, 1, &[0, 1], &[2, 3]);

        phase.report_match_result(match_id, MatchResult::BWins);

        for ranking_index in 0..4 {
            assert_eq!(metric_of(&phase, ranking_index), &Matches(vec![match_id]));
        }
    }

    #[test]
    fn the_result_of_a_match_does_not_influence_whether_it_is_recorded() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (2, vec![0], vec![1]),
            (3, vec![0], vec![1]),
        ];
        let mut phase = phase_with(2, matches);

        let mut match_ids = vec![];
        for (round, result) in [
            (1, MatchResult::AWins),
            (2, MatchResult::Draw),
            (3, MatchResult::BWins),
        ] {
            let match_id = match_id(&phase, round, &[0], &[1]);
            phase.report_match_result(match_id, result);
            match_ids.push(match_id);
        }

        let recorded = Matches(match_ids);
        assert_eq!(metric_of(&phase, 0), &recorded);
        assert_eq!(metric_of(&phase, 1), &recorded);
    }

    #[test]
    fn matches_accumulate_in_the_order_they_are_reported() {
        let matches = vec![
            (1, vec![0], vec![1]),
            (1, vec![2], vec![3]),
            (2, vec![0], vec![2]),
            (2, vec![1], vec![3]),
            (3, vec![1], vec![2]),
        ];
        let mut phase = phase_with(4, matches.clone());

        let match_ids: Vec<_> = matches
            .iter()
            .map(|(round, team_a, team_b)| match_id(&phase, *round, team_a, team_b))
            .collect();
        let [m_0_1, m_2_3, m_0_2, m_1_3, m_1_2] = match_ids[..] else {
            unreachable!("five matches are scheduled")
        };
        // reported in the reverse order of the schedule, which the metric has to follow
        for match_id in match_ids.iter().rev() {
            phase.report_match_result(*match_id, MatchResult::AWins);
        }

        assert_eq!(metric_of(&phase, 0), &Matches(vec![m_0_2, m_0_1]));
        assert_eq!(metric_of(&phase, 1), &Matches(vec![m_1_2, m_1_3, m_0_1]));
        assert_eq!(metric_of(&phase, 2), &Matches(vec![m_1_2, m_0_2, m_2_3]));
        assert_eq!(metric_of(&phase, 3), &Matches(vec![m_1_3, m_2_3]));
    }
}
