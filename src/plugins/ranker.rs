use std::{any::Any, cmp::Ordering, fmt::Debug, marker::PhantomData, num::NonZeroU32};

use itertools::Itertools;

use crate::{
    entities::competition::{Phase, Ranking, RankingId},
    plugins::Plugin,
    type_map::{DebugAny, TypeMap, downcast_unchecked_ref},
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
    fn extract_metric<'a>(&'a self, map: &'a TypeMap) -> Option<&'a dyn Any>;

    /// # Safety
    /// `v1` and `v2` must be values that got extracted earlier by calling
    /// [`Self::extract_metric`].
    unsafe fn compare(&self, v1: &dyn Any, v2: &dyn Any) -> Comparison;
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

impl<M: DebugAny + Compare> MetricComparator for Comparator<M> {
    fn extract_metric<'a>(&'a self, map: &'a TypeMap) -> Option<&'a dyn Any> {
        map.get_any::<M>()
    }

    unsafe fn compare(&self, v1: &dyn Any, v2: &dyn Any) -> Comparison {
        // Safety: due to the safety guarantee of compare + the guarantee of TypeMap that
        // `get_any::<M>` returns a dyn Any that has concrete type `M`. This runs for every single
        // comparison of a sort, so the virtual `Any::type_id` calls that a checked downcast would
        // not be able to optimize away dominate the actual comparison of the metric.
        let (v1, v2) = unsafe {
            (
                downcast_unchecked_ref::<M>(v1),
                downcast_unchecked_ref::<M>(v2),
            )
        };
        v1.compare(v2)
    }
}

#[derive(Debug, Default, Clone)]
pub struct LexicographicalMetrics(Vec<Box<dyn MetricComparator>>);

impl LexicographicalMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_metric<M: DebugAny + Compare>(mut self) -> Self {
        self.0.push(Box::new(Comparator::<M>(PhantomData)));
        self
    }
}

impl SliceRanker for LexicographicalMetrics {
    fn update_ranks(&self, _: &Phase, slice: &mut [(&Ranking, NonZeroU32)]) {
        let num_metrics = self.0.len();
        if num_metrics == 0 || slice.len() < 2 {
            // this becomes basically a no-op
            return;
        }
        // The metrics of all rankings as a single flat table, where row `i` holds the metrics of
        // `slice[i]`. This runs for every single reported match result, so it is worth avoiding
        // the one allocation per ranking that a `Vec` of rows would need.
        let metrics: Vec<&dyn Any> = slice
            .iter()
            .flat_map(|&(ranking, _)| {
                self.0.iter().map(move |comparator| {
                    comparator
                        .extract_metric(ranking.metrics())
                        .expect("All queried metrics must exist")
                })
            })
            .collect();
        let comparator = |row1: usize, row2: usize| {
            let (row1, row2) = (row1 * num_metrics, row2 * num_metrics);
            for (i, comparator) in self.0.iter().enumerate() {
                // SAFETY: both metrics are guaranteed to be outputs from
                // `self.0[i].extract_metric` by the way we filled the table above
                let result = unsafe { comparator.compare(metrics[row1 + i], metrics[row2 + i]) };
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
