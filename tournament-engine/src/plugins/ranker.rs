use std::{cmp::Ordering, fmt::Debug, num::NonZeroU32};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Phase, Ranking, RankingId},
    plugins::Plugin,
    type_map::{TypeMap, TypeMapValue},
};

#[typetag::serde(tag = "type", content = "value")]
pub trait Ranker: Plugin {
    /// After every rank update the following invariant must uphold: If we would order rankings by
    /// rank we would get: First ranking has rank 1, every following ranking has either the rank of
    /// its predecessor or has rank (index + 1).
    fn update_ranks(&self, phase: &Phase) -> Vec<(RankingId, NonZeroU32)>;
}

impl Clone for Box<dyn Ranker> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

#[typetag::serde(tag = "type", content = "value")]
pub trait SliceRanker: Plugin {
    /// Assigns new ranks to the given slice and reorders them according to the new ranks.
    fn update_ranks(&self, phase: &Phase, slice: &mut [(&Ranking, NonZeroU32)]);
}

impl Clone for Box<dyn SliceRanker> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchicalRanker(Vec<Box<dyn SliceRanker>>);

impl HierarchicalRanker {
    pub fn new(slice_rankers: Vec<Box<dyn SliceRanker>>) -> Self {
        Self(slice_rankers)
    }
}

#[typetag::serde]
impl Ranker for HierarchicalRanker {
    fn update_ranks(&self, phase: &Phase) -> Vec<(RankingId, NonZeroU32)> {
        let mut rankings_with_new_ranks: Vec<(&Ranking, NonZeroU32)> = phase
            .ranking_ids_ordered_by_unique_rank()
            .iter()
            .map(|id| (&phase.rankings()[id], NonZeroU32::new(1).unwrap()))
            .collect();

        let mut slices = vec![rankings_with_new_ranks.as_mut_slice()];
        for slice_ranker in &self.0 {
            slices = slices
                .drain(..)
                .flat_map(|slice| {
                    slice_ranker.update_ranks(phase, slice);
                    slice
                        .chunk_by_mut(|(_, r1), (_, r2)| r1 == r2)
                        .filter(|s| s.len() > 1)
                })
                .collect();
            if slices.is_empty() {
                break;
            }
        }

        rankings_with_new_ranks
            .into_iter()
            .map(|(ranking, rank)| (ranking.id(), rank))
            .collect()
    }
}

/// The order a [`RankingCriterion`] sorts the rankings in, which it states by calling exactly one
/// of the methods here.
///
/// Both of them sort by one metric, which the criterion names as the type parameter; what they
/// differ in is where the order of that metric comes from — the metric's own [`Compare`] impl, or a
/// comparison the criterion passes in. The rankings then compare by the value of that one metric
/// alone, and a ranking that does not hold it makes the criterion sort nothing, see
/// [`LexicographicalCriteria`].
#[must_use]
pub struct RankingOrder<'a, 'b> {
    metrics: &'b [&'a TypeMap],
    output: &'b mut Option<Box<dyn MetricColumn + 'a>>,
}

impl<'a, 'b> RankingOrder<'a, 'b> {
    /// Sorts by metric `M`, in the one order `M` is compared in itself, see [`Compare`].
    pub fn by_metric<M: TypeMapValue + Compare>(self) {
        *self.output = self.column_by_metric::<M>()
    }

    /// Sorts by metric `M` as well, but in the order `compare` puts two of its values in instead of
    /// in `M`'s own — which is also the only way to sort by a metric that has no [`Compare`] impl
    /// because it has no single natural order.
    ///
    /// This is what lets one stored metric back several criteria: a
    /// [`WinsDrawsLosses`](crate::plugins::metrics_calculator::wins_draws_losses::WinsDrawsLosses)
    /// is a single value in the metrics of a ranking, but its wins, draws, losses and the matches
    /// they add up to each order that value differently, so the order belongs to the criterion
    /// asking for it and not to the metric.
    pub fn by_metric_compared_by<M: TypeMapValue>(self, compare: fn(&M, &M) -> Comparison) {
        *self.output = self.column_compared_by::<M>(compare)
    }

    fn column_by_metric<M: TypeMapValue + Compare>(&self) -> Option<Box<dyn MetricColumn + 'a>> {
        let values: Vec<&M> = self.values::<M>()?;
        Some(Box::new(MetricValues(values)))
    }

    fn column_compared_by<M: TypeMapValue>(
        &self,
        compare: fn(&M, &M) -> Comparison,
    ) -> Option<Box<dyn MetricColumn + 'a>> {
        let values: Vec<&M> = self.values::<M>()?;
        Some(Box::new(ComparedBy(values, compare)))
    }

    /// The value of metric `M` of every ranking, or [`None`] if any of them does not hold it.
    fn values<M: TypeMapValue>(&self) -> Option<Vec<&'a M>> {
        self.metrics
            .iter()
            .map(|map| map.get::<M>())
            .collect::<Option<_>>()
    }
}

/// One way to compare two rankings, i.e. a criterion a ranking can be sorted by.
///
/// A criterion is more than a way to read a metric: it picks the metric *and* fixes the order the
/// rankings are compared in. Where that order is the metric's own, see [`Compare`], naming the
/// metric is all the criterion does, see [`RankingOrder::by_metric`]; where a metric has no single
/// natural order, or has several, the criterion passes the comparison in as well and says in its
/// name which way round it sorts, see [`RankingOrder::by_metric_compared_by`].
///
/// [`MorePoints`](crate::plugins::metrics_calculator::points::MorePoints) implements the first
/// shape, [`FewerLosses`](crate::plugins::metrics_calculator::wins_draws_losses::FewerLosses) the
/// second. Either way the order an implementation ends up stating can be read back with
/// [`compare_by`].
#[typetag::serde(tag = "type", content = "value")]
pub trait RankingCriterion: Plugin {
    /// States the order this criterion sorts the rankings in, by calling exactly one method of
    /// `order`.
    fn order_rankings(&self, order: RankingOrder<'_, '_>);
}

/// How `criterion` compares a ranking whose metrics are `first` to one whose metrics are `second`,
/// or [`None`] if either of them does not hold the metric `criterion` sorts by.
///
/// A ranker does not compare one pair at a time — [`LexicographicalCriteria`] resolves a whole
/// column of values per criterion and then compares within it — so this is the way to ask a
/// criterion about a single pair: to test a criterion in isolation, or to tell which of two
/// rankings a criterion puts first and why.
///
/// # Examples
/// ```
/// use tournament_engine::{
///     plugins::{
///         metrics_calculator::wins_draws_losses::{FewerLosses, WinsDrawsLosses},
///         ranker::{Comparison, compare_by},
///     },
///     type_map::TypeMap,
/// };
///
/// let mut one_loss = TypeMap::new();
/// one_loss.insert(WinsDrawsLosses {
///     wins: 1,
///     draws: 0,
///     losses: 1,
/// });
/// let mut two_losses = TypeMap::new();
/// two_losses.insert(WinsDrawsLosses {
///     wins: 5,
///     draws: 0,
///     losses: 2,
/// });
///
/// // the wins do not enter into it, this criterion sorts by the losses alone
/// assert_eq!(
///     compare_by(&FewerLosses, &one_loss, &two_losses),
///     Some(Comparison::Better)
/// );
/// // a ranking that does not hold the metric cannot be compared by it at all
/// assert_eq!(compare_by(&FewerLosses, &one_loss, &TypeMap::new()), None);
/// ```
pub fn compare_by(
    criterion: &dyn RankingCriterion,
    first: &TypeMap,
    second: &TypeMap,
) -> Option<Comparison> {
    let mut column = None;
    criterion.order_rankings(RankingOrder {
        metrics: &[first, second],
        output: &mut column,
    });
    Some(column?.compare(0, 1))
}

impl Clone for Box<dyn RankingCriterion> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

/// The values of one metric, one per ranking, already read out of their [`TypeMap`].
///
/// Extracting all values up front rather than one pair at a time is what lets a column keep them
/// typed: a comparison then indexes into values that are already known to be of one metric type,
/// instead of downcasting a `dyn Any` it has to be told the provenance of.
///
/// How a criterion's order is held is nothing a criterion has to know, so this and its
/// implementations are private to this module; a criterion only ever states an order, see
/// [`RankingOrder`], and asks about one pair through [`compare_by`].
trait MetricColumn {
    /// Compares the values of the rankings at `index1` and `index2`, in terms of the order they
    /// were extracted in.
    fn compare(&self, index1: usize, index2: usize) -> Comparison;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Better,
    Worse,
    Equal,
}

impl Comparison {
    pub fn to_ordering_for_sorting(self) -> Ordering {
        match self {
            Comparison::Better => Ordering::Less,
            Comparison::Worse => Ordering::Greater,
            Comparison::Equal => Ordering::Equal,
        }
    }

    pub fn less_is_better(ord: Ordering) -> Self {
        match ord {
            Ordering::Less => Comparison::Better,
            Ordering::Greater => Comparison::Worse,
            Ordering::Equal => Comparison::Equal,
        }
    }

    pub fn greater_is_better(ord: Ordering) -> Self {
        match ord {
            Ordering::Greater => Comparison::Better,
            Ordering::Less => Comparison::Worse,
            Ordering::Equal => Comparison::Equal,
        }
    }
}

/// The one order a metric is naturally compared in, which a [`RankingCriterion`] over that metric
/// can then sort by without naming a direction of its own.
///
/// Only implement this where the metric has a single obvious better end — more points, a later
/// first loss. A metric that counts several things, or one that different phases want sorted
/// different ways round, is left without this impl, so that every criterion over it has to state
/// its direction, see [`RankingOrder::by_metric_compared_by`].
pub trait Compare {
    fn compare(&self, other: &Self) -> Comparison;
}

/// The values of metric `M` of a list of rankings, which compare without any downcast because
/// [`TypeMap::get`] already resolved them to `&M`.
struct MetricValues<'a, M>(Vec<&'a M>);

impl<M: Compare> MetricColumn for MetricValues<'_, M> {
    fn compare(&self, index1: usize, index2: usize) -> Comparison {
        self.0[index1].compare(self.0[index2])
    }
}

/// The values of metric `M` of a list of rankings, ordered by the comparison the
/// [`RankingCriterion`] that read them picked instead of by `M`'s own [`Compare`] impl, see
/// [`RankingOrder::by_metric_compared_by`].
struct ComparedBy<'a, M>(Vec<&'a M>, fn(&M, &M) -> Comparison);

impl<M> MetricColumn for ComparedBy<'_, M> {
    fn compare(&self, index1: usize, index2: usize) -> Comparison {
        (self.1)(self.0[index1], self.0[index2])
    }
}

/// Sorts rankings by its criteria in order, each one breaking the ties of the ones before it.
///
/// A criterion whose metric a ranking does not hold sorts nothing at all, which is a configuration
/// error rather than something to rank around, so it panics instead of skipping the criterion.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LexicographicalCriteria(Vec<Box<dyn RankingCriterion>>);

impl LexicographicalCriteria {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `criterion`, so that it breaks the ties of every criterion already added.
    pub fn add_criterion(mut self, criterion: impl RankingCriterion + 'static) -> Self {
        self.0.push(Box::new(criterion));
        self
    }
}

#[typetag::serde]
impl SliceRanker for LexicographicalCriteria {
    fn update_ranks(&self, _: &Phase, slice: &mut [(&Ranking, NonZeroU32)]) {
        if self.0.is_empty() || slice.len() < 2 {
            // this becomes basically a no-op
            return;
        }
        // The metrics of all rankings, one dense column per metric. This runs for every single
        // reported match result, so it is worth resolving every value once here rather than on
        // every comparison of the sort below.
        let metrics = slice
            .iter()
            .map(|&(ranking, _)| ranking.metrics())
            .collect_vec();
        let columns = self
            .0
            .iter()
            .map(|criterion| {
                let mut column = None;
                let order = RankingOrder {
                    metrics: &metrics,
                    output: &mut column,
                };
                criterion.order_rankings(order);
                column.expect("All queried metrics must exist")
            })
            .collect_vec();
        let comparator = |row1: usize, row2: usize| {
            for column in &columns {
                let result = column.compare(row1, row2);
                if result != Comparison::Equal {
                    return result;
                }
            }
            Comparison::Equal
        };
        // sorting indices instead of the slice itself keeps the elements that the sort moves
        // around small and leaves the rows of the table in place
        let mut order = (0..slice.len() as u32).collect_vec();
        order.sort_by(|&row1, &row2| {
            comparator(row1 as usize, row2 as usize).to_ordering_for_sorting()
        });
        // assign new ranks and reorder the slice accordingly; all rankings of the slice start out
        // with the same rank, so any of them serves as the rank the first one keeps
        let start_rank = slice[0].1;
        let mut reordered = Vec::with_capacity(slice.len());
        reordered.push((slice[order[0] as usize].0, start_rank));
        for i in 1..order.len() {
            let rank = if comparator(order[i - 1] as usize, order[i] as usize) == Comparison::Equal
            {
                reordered[i - 1].1
            } else {
                start_rank.saturating_add(i as u32)
            };
            reordered.push((slice[order[i] as usize].0, rank));
        }
        slice.copy_from_slice(&reordered);
    }
}
