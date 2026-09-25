use std::{cmp::Ordering, fmt::Debug, marker::PhantomData, num::NonZeroU32};

use itertools::Itertools;

use crate::{
    entities::competition::{Phase, Ranking, RankingId},
    plugins::Plugin,
    type_map::{TypeMap, TypeMapValue},
};

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

pub trait SliceRanker: Plugin {
    /// Assigns new ranks to the given slice and reorders them according to the new ranks.
    fn update_ranks(&self, phase: &Phase, slice: &mut [(&Ranking, NonZeroU32)]);
}

impl Clone for Box<dyn SliceRanker> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

#[derive(Debug, Clone)]
pub struct HierarchicalRanker(Vec<Box<dyn SliceRanker>>);

impl HierarchicalRanker {
    pub fn new(slice_rankers: Vec<Box<dyn SliceRanker>>) -> Self {
        Self(slice_rankers)
    }
}

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

#[derive(Debug, Copy, Default)]
struct Comparator<M>(PhantomData<M>);

impl<M> Clone for Comparator<M> {
    fn clone(&self) -> Self {
        Self(PhantomData)
    }
}

trait MetricComparator: Plugin {
    /// Reads this comparator's metric out of every map of `metrics`, returning [`None`] if any of
    /// them does not hold that metric.
    ///
    /// Extracting all values up front rather than one pair at a time is what lets the returned
    /// column keep them typed: a comparison then indexes into values that are already known to be
    /// of this comparator's metric type, instead of downcasting a `dyn Any` it has to be told the
    /// provenance of.
    fn extract_column<'a>(&self, metrics: &[&'a TypeMap]) -> Option<Box<dyn MetricColumn + 'a>>;
}

/// The values of one metric, one per ranking, already read out of their [`TypeMap`].
trait MetricColumn {
    /// Compares the values of the rankings at `index1` and `index2`, in terms of the order they
    /// were extracted in.
    fn compare(&self, index1: usize, index2: usize) -> Comparison;
}

impl Clone for Box<dyn MetricComparator> {
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

impl<M: TypeMapValue + Compare> MetricComparator for Comparator<M> {
    fn extract_column<'a>(&self, metrics: &[&'a TypeMap]) -> Option<Box<dyn MetricColumn + 'a>> {
        let values: Vec<&M> = metrics
            .iter()
            .map(|map| map.get::<M>())
            .collect::<Option<_>>()?;
        Some(Box::new(MetricValues(values)))
    }
}

/// The values of metric `M` of a list of rankings, which compare without any downcast because
/// [`TypeMap::get`] already resolved them to `&M`.
struct MetricValues<'a, M>(Vec<&'a M>);

impl<M: Compare> MetricColumn for MetricValues<'_, M> {
    fn compare(&self, index1: usize, index2: usize) -> Comparison {
        self.0[index1].compare(self.0[index2])
    }
}

#[derive(Debug, Default, Clone)]
pub struct LexicographicalMetrics(Vec<Box<dyn MetricComparator>>);

impl LexicographicalMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_metric<M: TypeMapValue + Compare>(mut self) -> Self {
        self.0.push(Box::new(Comparator::<M>(PhantomData)));
        self
    }
}

impl SliceRanker for LexicographicalMetrics {
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
            .map(|comparator| {
                comparator
                    .extract_column(&metrics)
                    .expect("All queried metrics must exist")
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
