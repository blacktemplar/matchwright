use std::{
    any::{Any, TypeId},
    fmt::{self, Display, Formatter},
};

use dyn_clone::DynClone;
use plugin_factory::{Plugin, axis};

use crate::{
    Score,
    entities::competition::{Bye, Match, PhaseData, Ranking, RankingId},
    map::Map,
    plugins::Phrase,
    type_map::{TypeMapValue, downcast_unchecked_ref},
};

pub mod buchholz;
pub mod points;
pub mod round_of_elimination;
pub mod round_of_first_loss;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod wins_draws_losses;

pub use buchholz::{
    Buchholz, BuchholzCalculator, BuchholzOfBuchholz, BuchholzOfBuchholzCalculator, MoreBuchholz,
    MoreBuchholzOfBuchholz, MoreSonnebornBerger, SonnebornBerger, SonnebornBergerCalculator,
};
pub use points::{MorePoints, Points, PointsCalculator};
pub use round_of_elimination::{
    Eliminates, LaterElimination, RoundOfElimination, RoundOfEliminationCalculator,
};
pub use round_of_first_loss::{LaterFirstLoss, RoundOfFirstLoss, RoundOfFirstLossCalculator};
pub use wins_draws_losses::{
    FewerLosses, FewerMatchesPlayed, MoreDraws, MoreWins, WinsDrawsLosses,
    WinsDrawsLossesCalculator,
};

pub type MetricId = TypeId;

pub type Metric = Box<dyn TypeMapValue>;

/// The values of metrics that the calculators which already ran for an update replaced, keyed by
/// metric type.
#[derive(Debug, Default)]
pub struct OldMetrics(Map<MetricId, Vec<(RankingId, Metric)>>);

impl OldMetrics {
    /// The replaced values of the metric `M`, each with the ranking it belonged to, in the order
    /// they were replaced.
    pub fn of<M: TypeMapValue>(&self) -> impl Iterator<Item = (RankingId, &M)> {
        self.0
            .get(&TypeId::of::<M>())
            .into_iter()
            .flatten()
            .map(|(ranking_id, metric)| {
                let metric: &dyn Any = &**metric;
                // SAFETY: `push` stores every metric under the type id of its concrete type, so
                // the metrics at key `TypeId::of::<M>()` are `M`s.
                (*ranking_id, unsafe { downcast_unchecked_ref::<M>(metric) })
            })
    }

    /// Records `metric` as the value of `ranking_id` that got replaced.
    pub(crate) fn push(&mut self, ranking_id: RankingId, metric: Metric) {
        let metric_id = (&*metric as &dyn Any).type_id();
        self.0
            .entry(metric_id)
            .or_default()
            .push((ranking_id, metric));
    }
}

/// Calculates one metric of every ranking of a phase.
///
/// A calculator has to declare that it provides the [`CalculatesMetric`] of its metric, and that it
/// requires every fact it reads, in particular the [`CalculatesMetric`] of every other metric it
/// reads. It runs after the calculators providing the facts it requires.
///
/// [`CalculatesMetric`]: super::facts::CalculatesMetric
///
/// A calculator may keep state as a [`PhaseFormat`](super::phase_format::PhaseFormat) does.
#[axis]
pub trait MetricsCalculator: Plugin + DynClone {
    /// The value every ranking of a phase starts out with, whose type is the metric this
    /// calculator calculates.
    fn default_metric_value(&self) -> Metric;

    /// The new values of all rankings whose metric the result just reported for `match` changed.
    /// A match is reported once, so `match` was not finished before.
    ///
    /// `old_metrics` holds the values replaced by the calculators that already ran, so that a
    /// calculator deriving its metric from another one can see what changed there.
    ///
    /// All returned metrics must have the same concrete type as
    /// [`Self::default_metric_value`].
    fn update_after_match_result(
        &mut self,
        phase_data: &PhaseData,
        r#match: &Match,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)>;

    /// The new values of all rankings whose metric the `bye` changed, which its ranking has just
    /// [reached](Bye::is_reached).
    ///
    /// A bye is not a match: it has no opponent and no result, and once reached it never changes.
    /// Whether it counts for this metric at all is up to the calculator.
    ///
    /// `old_metrics` and the returned metrics are as for [`Self::update_after_match_result`].
    fn update_after_bye(
        &mut self,
        phase_data: &PhaseData,
        bye: &Bye,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)>;

    /// The column showing this calculator's metric in a table of standings, or `None` if the
    /// metric is not meant to be shown.
    fn column(&self) -> Option<MetricColumn> {
        None
    }
}

dyn_clone::clone_trait_object!(MetricsCalculator);

/// A column of a table of standings showing one metric: its header and the cell of every
/// ranking's value.
#[derive(Debug, Clone, Copy)]
pub struct MetricColumn {
    header: &'static str,
    cell: fn(&Ranking) -> Option<Cell>,
}

impl MetricColumn {
    /// The column headed `header` that shows the metric `M` of a ranking as its [`ToCell`].
    pub fn new<M: TypeMapValue + ToCell>(header: &'static str) -> Self {
        Self {
            header,
            cell: |ranking| ranking.metric::<M>().map(M::to_cell),
        }
    }

    pub fn header(&self) -> &'static str {
        self.header
    }

    /// The cell of the metric of `ranking`, or `None` if `ranking` has no such metric.
    pub fn cell(&self, ranking: &Ranking) -> Option<Cell> {
        (self.cell)(ranking)
    }
}

/// A metric shown in a [`MetricColumn`].
pub trait ToCell {
    /// What its cell shows.
    fn to_cell(&self) -> Cell;
}

/// What a cell of a [`MetricColumn`] shows. Shows as it does in English.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Cell {
    /// A number, which a frontend writes as its language writes numbers.
    Number(Score),
    /// A phrase of the calculator's, which a translation finds by the calculator's id.
    Text(Phrase),
}

impl Display for Cell {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => Display::fmt(number, f),
            Self::Text(phrase) => Display::fmt(phrase, f),
        }
    }
}
