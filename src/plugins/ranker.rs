use std::{cmp::Ordering, fmt::Debug, num::NonZeroU32};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{Metrics, Phase, Ranking, RankingId},
    plugins::{Plugin, metrics_calculator::Metric},
    type_map::{TypeMapValue, downcast_unchecked_ref},
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
    /// Assigns new ranks to the rankings of `slice` and reorders them according to the new ranks.
    ///
    /// All rankings of `slice` come in tied at one rank, which the best of them keeps: a slice
    /// ranker only ever breaks a tie, never reorders rankings a ranker before it has already put in
    /// an order.
    fn update_ranks(&self, slice: &mut RankingSlice<'_, '_>);
}

impl Clone for Box<dyn SliceRanker> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

/// The rankings a [`SliceRanker`] ranks, together with the phase they belong to.
///
/// # Invariant
/// Every ranking of a slice is a ranking of [`phase`](Self::phase). Nothing outside this crate can
/// construct a slice, and the two places that do only ever build one from the rankings of the phase
/// they pass along. That is what lets [`Self::rank_by`] hand a ranking's metrics to the comparison
/// as a [`Metrics`], which resolves them against the layout of that phase.
pub struct RankingSlice<'phase, 'slice> {
    phase: &'phase Phase,
    rankings: &'slice mut [(&'phase Ranking, NonZeroU32)],
}

impl<'phase, 'slice> RankingSlice<'phase, 'slice> {
    /// The phase all rankings of this slice belong to.
    pub fn phase(&self) -> &'phase Phase {
        self.phase
    }

    /// The rankings of this slice with the rank each of them currently has, ordered from best to
    /// worst.
    pub fn rankings(&self) -> &[(&'phase Ranking, NonZeroU32)] {
        self.rankings
    }

    /// Sorts the rankings of this slice from best to worst by `compare` and gives them the ranks
    /// that order implies: the best ranking keeps the rank all of them came in with, and every
    /// following one gets either the rank of its predecessor, if the two compare
    /// [`Comparison::Equal`], or the rank its position gives it.
    ///
    /// Rankings that compare equal keep the order they came in, which is the order of their
    /// [`unique_rank`](Ranking::unique_rank).
    ///
    /// This is the only way a ranker changes a slice, which is what keeps the invariant of a slice
    /// intact: comparing and reordering can never bring a ranking of another phase in.
    pub fn rank_by(
        &mut self,
        mut compare: impl FnMut(
            (&'phase Ranking, Metrics<'phase>),
            (&'phase Ranking, Metrics<'phase>),
        ) -> Comparison,
    ) {
        // all rankings of a slice come in tied at one rank, so any of them holds the rank the best
        // of them keeps
        let Some(&(_, start_rank)) = self.rankings.first() else {
            return;
        };
        let phase = self.phase;
        let mut compare_rankings = move |ranking1: &'phase Ranking, ranking2: &'phase Ranking| {
            compare(
                (ranking1, phase.metrics_of_ranking(ranking1)),
                (ranking2, phase.metrics_of_ranking(ranking2)),
            )
        };
        // The rankings come in in the order of the last ranking, which only the few rankings whose
        // metrics just changed are out of, and a stable sort exploits exactly that: it detects the
        // runs that are already in order and merges them instead of comparing everything against
        // everything.
        self.rankings.sort_by(|&(ranking1, _), &(ranking2, _)| {
            compare_rankings(ranking1, ranking2).to_ordering_for_sorting()
        });
        // Only comparisons of neighbours are left to decide which rankings share a rank, since a
        // ranking that compares equal to neither of its neighbours cannot compare equal to any
        // other ranking of the slice either.
        let mut rank = start_rank;
        for index in 1..self.rankings.len() {
            let (previous, current) = (self.rankings[index - 1].0, self.rankings[index].0);
            if compare_rankings(previous, current) != Comparison::Equal {
                rank = start_rank.saturating_add(index as u32);
            }
            self.rankings[index].1 = rank;
        }
    }

    /// Splits this slice into the subslices of rankings that ended up sharing a rank, leaving out
    /// those that hold a single ranking, since there is no tie left to break for those.
    pub fn chunks_of_equal_rank(self) -> impl Iterator<Item = RankingSlice<'phase, 'slice>> {
        let Self { phase, rankings } = self;
        rankings
            .chunk_by_mut(|(_, rank1), (_, rank2)| rank1 == rank2)
            .filter(|chunk| chunk.len() > 1)
            .map(move |rankings| RankingSlice { phase, rankings })
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
        let first_rank = NonZeroU32::new(1).unwrap();
        // Ranking by unique rank is what gives the slice rankers a nearly ordered slice to work on,
        // and the unique ranks of a phase are exactly the numbers from one up to the number of its
        // rankings, so every ranking can be put at its place directly. Walking
        // `ranking_ids_ordered_by_unique_rank` instead would hash and probe the ranking map once
        // per ranking, and this runs for every reported match result.
        debug_assert!(
            phase
                .rankings()
                .values()
                .map(|ranking| ranking.unique_rank())
                .all_unique()
        );
        let Some(any_ranking) = phase.rankings().values().next() else {
            return Vec::new();
        };
        let mut rankings_with_new_ranks = vec![(any_ranking, first_rank); phase.rankings().len()];
        for ranking in phase.rankings() {
            rankings_with_new_ranks[ranking.unique_rank().get() as usize - 1] =
                (ranking, first_rank);
        }

        let mut slices = vec![RankingSlice {
            phase,
            rankings: &mut rankings_with_new_ranks,
        }];
        for slice_ranker in &self.0 {
            slices = slices
                .drain(..)
                .flat_map(|mut slice| {
                    slice_ranker.update_ranks(&mut slice);
                    slice.chunks_of_equal_rank()
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

/// One metric of a phase, resolved to the position it has in the metrics of every ranking of that
/// phase, together with a comparison that knows the type the metric has.
///
/// Resolving that position once per slice instead of once per ranking, let alone once per
/// comparison, is what the metrics layout of a phase buys: reading the metric of a ranking is then
/// an indexing operation and a pointer cast.
///
/// # Invariant
/// Both the position and the comparison belong to the same metric type, which
/// [`MetricStorage::set_metric`] is the only way to set and hence upholds.
struct MetricColumn {
    /// The position the metric has in the metrics of every ranking of the phase.
    index: usize,
    /// Compares two values of the metric.
    ///
    /// A plain function pointer rather than a trait object, since a column is not tied to any set
    /// of rankings and therefore has nothing to hold but the metric type, which a function pointer
    /// to a monomorphization already captures.
    compare_values: unsafe fn(&Metric, &Metric) -> Comparison,
}

impl MetricColumn {
    /// Compares this column's metric of two rankings.
    ///
    /// # Invariant
    /// Both metrics must be those of a ranking of the phase this column was resolved against, which
    /// is what makes the value at [`Self::index`] the type [`Self::compare_values`] expects.
    fn compare(&self, metrics1: Metrics<'_>, metrics2: Metrics<'_>) -> Comparison {
        // SAFETY: the metrics of a ranking of the phase this column was resolved against hold one
        // value per metrics calculator of that phase, so the position `set_metric` resolved the
        // metric type to is in bounds and holds a value of that very type.
        unsafe {
            (self.compare_values)(
                metrics1.values().get_unchecked(self.index),
                metrics2.values().get_unchecked(self.index),
            )
        }
    }
}

/// Compares two type erased values of the metric `M`.
///
/// # Safety
/// Both values must have the concrete type `M`.
unsafe fn compare_values<M: TypeMapValue + Compare>(
    value1: &Metric,
    value2: &Metric,
) -> Comparison {
    // SAFETY: the caller guarantees that both values are of type `M`.
    unsafe { downcast_unchecked_ref::<M>(&**value1).compare(downcast_unchecked_ref(&**value2)) }
}

/// Where a [`MetricGetter`] stores the metric a ranker is to compare rankings by.
#[must_use]
pub struct MetricStorage<'a> {
    phase: &'a Phase,
    output: &'a mut Option<MetricColumn>,
}

impl MetricStorage<'_> {
    /// Stores the metric `M`, so that the ranker compares rankings by their value of `M`.
    ///
    /// Storing nothing at all if the phase does not calculate `M` is deliberate: which metrics a
    /// phase has is up to its calculators, so it is the ranker that has to deal with a metric it
    /// cannot read, not every getter of one.
    pub fn set_metric<M: TypeMapValue + Compare>(self) {
        *self.output = self.phase.metric_index::<M>().map(|index| MetricColumn {
            index,
            compare_values: compare_values::<M>,
        });
    }
}

#[typetag::serde(tag = "type", content = "value")]
pub trait MetricGetter: Plugin {
    fn store_metric(&self, storage: MetricStorage<'_>);
}

impl Clone for Box<dyn MetricGetter> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
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

pub trait Compare {
    fn compare(&self, other: &Self) -> Comparison;
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LexicographicalMetrics(Vec<Box<dyn MetricGetter>>);

impl LexicographicalMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_metric(mut self, metric_getter: impl MetricGetter + 'static) -> Self {
        self.0.push(Box::new(metric_getter));
        self
    }
}

#[typetag::serde]
impl SliceRanker for LexicographicalMetrics {
    /// # Panics
    /// Panics if the phase does not calculate one of the metrics to compare by.
    fn update_ranks(&self, slice: &mut RankingSlice<'_, '_>) {
        if self.0.is_empty() || slice.rankings().len() < 2 {
            // this becomes basically a no-op
            return;
        }
        // This runs for every single reported match result, so every metric is resolved to its
        // position in the metrics of a ranking once here rather than on every comparison below.
        let columns = self
            .0
            .iter()
            .map(|metric_getter| {
                let mut column = None;
                metric_getter.store_metric(MetricStorage {
                    phase: slice.phase(),
                    output: &mut column,
                });
                column.expect("All queried metrics must exist")
            })
            .collect_vec();
        slice.rank_by(|(_, metrics1), (_, metrics2)| {
            for column in &columns {
                let comparison = column.compare(metrics1, metrics2);
                if comparison != Comparison::Equal {
                    return comparison;
                }
            }
            Comparison::Equal
        });
    }
}
