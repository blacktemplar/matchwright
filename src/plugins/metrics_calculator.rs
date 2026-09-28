use std::{any::TypeId, ops::Deref};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    entities::phase::{Match, MatchResult, Phase, RankingId},
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

/// The metrics calculators of a phase, together with the position each produced metric has among
/// them.
///
/// The layout is not stored next to the calculators but derived from them, so the two can never
/// disagree: it is (re)built whenever the calculators are set, deserializing included. That is why
/// only the calculators are part of the serialized form.
#[derive(Debug)]
pub struct MetricsCalculators {
    calculators: Vec<Box<dyn MetricsCalculator>>,
    layout: Map<MetricId, usize>,
}

impl MetricsCalculators {
    #[expect(clippy::borrowed_box)]
    pub(crate) fn index_of_box(&self, v: &Box<dyn TypeMapValue>) -> usize {
        *self
            .layout
            .get(&(**v).type_id())
            .expect("Inner type of box is part of layout")
    }
    pub(crate) fn index_of<T: 'static>(&self) -> Option<usize> {
        self.layout.get(&TypeId::of::<T>()).copied()
    }

    /// Whether `metrics` is laid out by this layout, that is whether it holds one metric per
    /// calculator and every one of them at the position the layout gives its type.
    ///
    /// This is what everything that reads a metric by its position rather than by its type relies
    /// on, [`index_of`](Self::index_of) and hence
    /// [`RankingView`](crate::entities::phase::RankingView) and
    /// [`MetricComparator`](crate::entities::phase::MetricComparator) above all. It can only fail
    /// for a [`MetricsCalculator`] that breaks one of two rules that nothing else enforces: that
    /// [`default_metric_value`](MetricsCalculator::default_metric_value) returns a metric of the
    /// same type on every call, since the layout is built from one call of it and the metrics of a
    /// ranking from another, and that no two calculators of a phase produce the same metric type,
    /// since the layout can hold only one position per type.
    pub(crate) fn is_layout_of(&self, metrics: &[Metric]) -> bool {
        metrics.len() == self.calculators.len()
            && metrics
                .iter()
                .enumerate()
                .all(|(index, metric)| self.layout.get(&(**metric).type_id()) == Some(&index))
    }
}

impl From<Vec<Box<dyn MetricsCalculator>>> for MetricsCalculators {
    fn from(calculators: Vec<Box<dyn MetricsCalculator>>) -> Self {
        let layout = calculators
            .iter()
            .enumerate()
            .map(|(i, c)| ((*c.default_metric_value()).type_id(), i))
            .collect();
        Self {
            calculators,
            layout,
        }
    }
}

/// Gives access to the calculators
impl Deref for MetricsCalculators {
    type Target = [Box<dyn MetricsCalculator>];

    fn deref(&self) -> &Self::Target {
        &self.calculators
    }
}

impl<'a> IntoIterator for &'a MetricsCalculators {
    type Item = &'a Box<dyn MetricsCalculator>;
    type IntoIter = std::slice::Iter<'a, Box<dyn MetricsCalculator>>;

    fn into_iter(self) -> Self::IntoIter {
        self.calculators.iter()
    }
}

/// Serializes as a plain sequence of the calculators; the layout is left out, since it is nothing
/// but a function of them. See the type level documentation.
impl Serialize for MetricsCalculators {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.calculators.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MetricsCalculators {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Vec::<Box<dyn MetricsCalculator>>::deserialize(deserializer)?.into())
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
        wins_draws_losses::{WinsDrawsLosses, WinsDrawsLossesCalculator},
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

    fn calculators() -> MetricsCalculators {
        vec![
            Box::new(WinsDrawsLossesCalculator) as Box<dyn MetricsCalculator>,
            Box::new(THREE_ONE),
        ]
        .into()
    }

    /// The metrics of a ranking of a phase with these calculators, as
    /// [`Phase::initialize_metrics`](crate::entities::phase::Phase) builds them.
    fn metrics(calculators: &MetricsCalculators) -> Vec<Metric> {
        calculators
            .iter()
            .map(|calculator| calculator.default_metric_value())
            .collect()
    }

    #[test]
    fn the_metrics_of_a_ranking_follow_the_layout_they_were_built_from() {
        let calculators = calculators();

        assert!(calculators.is_layout_of(&metrics(&calculators)));
    }

    #[test]
    fn metrics_in_another_order_than_the_calculators_do_not_follow_the_layout() {
        let calculators = calculators();
        let mut metrics = metrics(&calculators);
        metrics.reverse();

        assert!(!calculators.is_layout_of(&metrics));
    }

    #[test]
    fn fewer_metrics_than_calculators_do_not_follow_the_layout() {
        let calculators = calculators();
        let metrics = metrics(&calculators);

        assert!(!calculators.is_layout_of(&metrics[..1]));
    }

    #[test]
    #[should_panic = "every metrics calculator of a phase must produce a metric of the same type"]
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
