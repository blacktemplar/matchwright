use std::any::TypeId;

use crate::{
    entities::{
        phase::{Match, MatchResult, Phase},
        rankings::RankingId,
    },
    map::Map,
    plugins::Plugin,
    type_map::TypeMapValue,
};

pub mod buchholz;
pub mod matches;
pub mod points;
pub mod round_of_first_loss;
pub mod sonneborn_berger;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod wins_draws_losses;

pub type MetricId = TypeId;

pub type Metric = Box<dyn TypeMapValue>;

#[typetag::serde(tag = "type", content = "value")]
pub trait MetricsCalculator: Plugin {
    /// The value this calculator's metric starts out at for every ranking, which is also what fixes
    /// the type of that metric.
    ///
    /// # Requirements
    /// Every call must return a metric of the same type, and no two calculators of one phase may
    /// return the same type: the rankings of a phase lay their metrics out by type, so starting a
    /// phase whose calculators break either rule panics.
    fn default_metric_value(&self) -> Metric;

    /// The metrics in the result vec must all have the same underlying type as
    /// [`Self::default_metric_value`]
    fn update_after_match_result_changed(
        &self,
        phase: &Phase,
        r#match: &Match,
        old_match_result: MatchResult,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;
}

impl Clone for Box<dyn MetricsCalculator> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::plugins::metrics_calculator::{
        points::{Points, PointsCalculator},
        test_utils::phase_with,
        wins_draws_losses::WinsDrawsLosses,
    };

    /// The scoring system of most association football leagues.
    const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

    /// A calculator whose metric changes type from one call of
    /// [`MetricsCalculator::default_metric_value`] to the next, which is the one way a calculator
    /// can make the metrics of a ranking disagree with the layout built from the calculators.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct FickleCalculator;

    #[typetag::serde]
    impl MetricsCalculator for FickleCalculator {
        fn default_metric_value(&self) -> Metric {
            static CALLS: AtomicUsize = AtomicUsize::new(0);
            if CALLS.fetch_add(1, Ordering::Relaxed) == 0 {
                Box::new(Points(0.0))
            } else {
                Box::new(WinsDrawsLosses::default())
            }
        }

        fn update_after_match_result_changed(
            &self,
            _: &Phase,
            _: &Match,
            _: MatchResult,
            _: &Map<TypeId, Vec<(RankingId, Metric)>>,
        ) -> Vec<(RankingId, Metric)> {
            vec![]
        }
    }

    #[test]
    #[should_panic = "must produce a metric of the same type on every call"]
    fn a_phase_whose_calculator_changes_its_metric_type_cannot_be_built() {
        phase_with(2, vec![], vec![Box::new(FickleCalculator)]);
    }

    #[test]
    #[should_panic = "no two of them the same type"]
    fn a_phase_with_two_calculators_of_the_same_metric_cannot_be_built() {
        phase_with(
            2,
            vec![],
            vec![
                Box::new(THREE_ONE),
                Box::new(PointsCalculator::new(2.0, 1.0)),
            ],
        );
    }
}
