use std::{any::TypeId, ops::Deref};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    entities::competition::{Match, MatchResult, Phase, RankingId},
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

    /// The metrics in the result vec must all have the type id returned by
    /// [`Self::produced_metric`].
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
    pub(crate) fn index_of_box(&self, v: &Box<dyn TypeMapValue>) -> usize {
        *self
            .layout
            .get(&(&*v).type_id())
            .expect("Inner type of box is part of layout")
    }
    pub(crate) fn index_of<T: 'static>(&self) -> Option<usize> {
        self.layout.get(&TypeId::of::<T>()).copied()
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

/// Gives access to the calculators themselves; the layout is read-only by way of
/// [`layout`](MetricsCalculators::layout), since nothing may set it but the calculators it is
/// derived from.
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
