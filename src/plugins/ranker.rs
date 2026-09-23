use std::{any::Any, cmp::Ordering, fmt::Debug, marker::PhantomData, num::NonZeroU32};

use itertools::Itertools;

use crate::{
    entities::competition::{Phase, Ranking, RankingId},
    plugins::Plugin,
    type_map::{DebugAny, TypeMap},
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
            .rankings()
            .values()
            .map(|ranking| (ranking, NonZeroU32::new(1).unwrap()))
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
        // `get_any::<M>` returns a dyn Any that has concrete type `M`.
        let (v1, v2) = unsafe {
            (
                v1.downcast_ref::<M>().unwrap_unchecked(),
                v2.downcast_ref::<M>().unwrap_unchecked(),
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
        if num_metrics == 0 {
            // this becomes basically a no-op
            return;
        }
        let mut metrics = slice
            .iter_mut()
            .map(|(ranking, rank)| {
                (
                    self.0
                        .iter()
                        .map(|comparator| {
                            comparator
                                .extract_metric(ranking.metrics())
                                .expect("All queried metrics must exist")
                        })
                        .collect_vec(),
                    rank,
                )
            })
            .collect_vec();
        let comparator = |metrics1: &Vec<&dyn Any>, metrics2: &Vec<&dyn Any>| {
            for i in 0..num_metrics {
                // SAFETY: metrics1[i] and metrics2[i] are guaranteed to be both outputs from
                // `self.0[i].extract_metric` by the way we created them above
                let result = unsafe { self.0[i].compare(metrics1[i], metrics2[i]) };
                if result != Comparison::Equal {
                    return result;
                }
            }
            Comparison::Equal
        };
        metrics.sort_by(|(metrics1, _), (metrics2, _)| {
            comparator(metrics1, metrics2).to_ordering_for_sorting()
        });
        // assign new ranks
        let start_rank = *metrics[0].1;
        for i in 1..metrics.len() {
            if comparator(&metrics[i - 1].0, &metrics[i].0) == Comparison::Equal {
                *metrics[i].1 = *metrics[i - 1].1;
            } else {
                *metrics[i].1 = start_rank.saturating_add(i as u32);
            }
        }
        // reorder slice according to the new ranks
        slice.sort_by_key(|(_, rank)| *rank);
    }
}
