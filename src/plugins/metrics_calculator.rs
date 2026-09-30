use std::any::{Any, TypeId};

use serde::{Serialize, de::DeserializeOwned};

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

/// Everything the rankings of a phase need to know about the metric of one [`MetricsCalculator`]:
/// the value it starts out at, and how to write that metric and read it back.
///
/// A kind is built from one type parameter, [`MetricKind::of::<M>()`](MetricKind::of), and all
/// three of those follow from it, so a kind can never describe one type but produce or read a value
/// of another. That is what lets the rankings serialize their metrics without a type tag: the
/// position of a metric plus the kind of the calculator at that position says everything the tag
/// would have.
///
/// # Requirements
/// What a kind cannot promise is that the *next* call of [`MetricsCalculator::metric_kind`] returns
/// one of the same type; see the requirements there.
#[derive(Debug)]
pub struct MetricKind {
    /// The value the metric starts out at, of the type the kind was built from.
    default: Metric,
    /// Casts a metric back to the type the kind was built from, so that it can be serialized as
    /// that type rather than as a type erased box. Returns `None` for a metric of another type,
    /// which only a calculator breaking the requirements of [`MetricsCalculator::metric_kind`] can
    /// produce.
    erase: fn(&Metric) -> Option<&dyn erased_serde::Serialize>,
    /// Reads a metric of the type the kind was built from.
    deserialize: fn(&mut dyn erased_serde::Deserializer<'_>) -> erased_serde::Result<Metric>,
}

impl MetricKind {
    /// The kind of a metric that starts out at its [`Default`] value, which is every metric this
    /// crate ships with.
    pub fn of<M: TypeMapValue + Default + Serialize + DeserializeOwned>() -> Self {
        Self::with_default(M::default())
    }

    /// The kind of a metric that starts out at a value derived from the calculator, e.g. a
    /// handicap. Use [`Self::of`] unless the initial value really depends on the configuration of
    /// the calculator.
    pub fn with_default<M: TypeMapValue + Serialize + DeserializeOwned>(default: M) -> Self {
        Self {
            default: Box::new(default),
            erase: |metric| {
                let metric: &dyn Any = &**metric;
                metric
                    .downcast_ref::<M>()
                    .map(|m| m as &dyn erased_serde::Serialize)
            },
            deserialize: |deserializer| Ok(Box::new(erased_serde::deserialize::<M>(deserializer)?)),
        }
    }

    /// The type of the metric, which is the type the kind was built from.
    pub(crate) fn metric_id(&self) -> MetricId {
        // note the dereference: `self.default.type_id()` would be the type id of the box
        (*self.default).type_id()
    }

    /// The value the metric starts out at.
    pub(crate) fn into_default(self) -> Metric {
        self.default
    }

    /// `metric` as something that serializes as its own type rather than as a type erased box, or
    /// `None` if it is not of the type of this kind.
    pub(crate) fn erase<'a>(&self, metric: &'a Metric) -> Option<&'a dyn erased_serde::Serialize> {
        (self.erase)(metric)
    }

    /// Reads a metric of the type of this kind.
    pub(crate) fn deserialize_metric(
        &self,
        deserializer: &mut dyn erased_serde::Deserializer<'_>,
    ) -> erased_serde::Result<Metric> {
        (self.deserialize)(deserializer)
    }
}

#[typetag::serde(tag = "type", content = "value")]
pub trait MetricsCalculator: Plugin {
    /// The [kind](MetricKind) of metric this calculator produces, which is what fixes the type of
    /// that metric, the value it starts out at for every ranking, and its serialized form.
    ///
    /// # Requirements
    /// Every call must return a kind of the same type, and no two calculators of one phase may
    /// return the same type: the rankings of a phase lay their metrics out by type, so starting a
    /// phase whose calculators break either rule panics, and serializing one that somehow exists
    /// anyway fails rather than writing metrics that cannot be read back.
    fn metric_kind(&self) -> MetricKind;

    /// The metrics in the result vec must all have the same underlying type as the one
    /// [`Self::metric_kind`] describes
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
    /// [`MetricsCalculator::metric_kind`] to the next, which is the one way a calculator can make
    /// the metrics of a ranking disagree with the layout built from the calculators.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct FickleCalculator;

    #[typetag::serde]
    impl MetricsCalculator for FickleCalculator {
        fn metric_kind(&self) -> MetricKind {
            static CALLS: AtomicUsize = AtomicUsize::new(0);
            if CALLS.fetch_add(1, Ordering::Relaxed) == 0 {
                MetricKind::of::<Points>()
            } else {
                MetricKind::of::<WinsDrawsLosses>()
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
