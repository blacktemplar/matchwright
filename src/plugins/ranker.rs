use std::{cmp::Ordering, fmt::Debug, num::NonZeroU32};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    entities::{
        phase::Phase,
        rankings::{Comparison, MetricComparator, RankingId, RankingView, Rankings},
    },
    plugins::Plugin,
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
    ///
    /// Every ranking of `slice` is a ranking of `phase`, which is what lets a slice ranker compare
    /// them by a [`MetricComparator`] resolved against that phase.
    fn update_ranks(&self, phase: &Phase, slice: &mut [(RankingView<'_>, NonZeroU32)]);
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
        let first_rank = NonZeroU32::new(1).unwrap();
        // Ranking by unique rank is what gives the slice rankers a nearly ordered slice to work on,
        // and the unique ranks of a phase are exactly the numbers from one up to the number of its
        // rankings, so every ranking can be put at its place directly. Walking
        // `ranking_ids_ordered_by_unique_rank` instead would hash and probe the ranking map once
        // per ranking, and this runs for every reported match result.
        debug_assert!(
            phase
                .ranking_views()
                .map(|ranking| ranking.unique_rank())
                .all_unique()
        );
        let Some(any_ranking) = phase.ranking_views().next() else {
            return Vec::new();
        };
        let mut rankings_with_new_ranks =
            vec![(any_ranking, first_rank); phase.ranking_views().len()];
        for ranking in phase.ranking_views() {
            rankings_with_new_ranks[ranking.unique_rank().get() as usize - 1] =
                (ranking, first_rank);
        }

        let mut slices: Vec<&mut [_]> = vec![&mut rankings_with_new_ranks];
        for slice_ranker in &self.0 {
            slices = slices
                .drain(..)
                .flat_map(|slice| {
                    slice_ranker.update_ranks(phase, slice);
                    slice
                        .chunk_by_mut(|(_, rank1), (_, rank2)| rank1 == rank2)
                        .filter(|chunk| chunk.len() > 1)
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

#[typetag::serde(tag = "type", content = "value")]
pub trait MetricComparatorProvider: Plugin {
    fn metric_comparator<'a>(&self, rankings: &'a Rankings) -> MetricComparator<'a>;
}

impl Clone for Box<dyn MetricComparatorProvider> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LexicographicalMetrics(Vec<Box<dyn MetricComparatorProvider>>);

impl LexicographicalMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_metric(
        mut self,
        metric_comparator_provider: impl MetricComparatorProvider + 'static,
    ) -> Self {
        self.0.push(Box::new(metric_comparator_provider));
        self
    }
}

#[typetag::serde]
impl SliceRanker for LexicographicalMetrics {
    /// # Panics
    /// Panics if the phase does not calculate one of the metrics to compare by.
    fn update_ranks(&self, phase: &Phase, slice: &mut [(RankingView<'_>, NonZeroU32)]) {
        if self.0.is_empty() || slice.len() < 2 {
            // this becomes basically a no-op
            return;
        }
        // we collect the providers that store the indices so that on the hotpath of comparsion
        // we only access indices, no hashing at all.
        let comparators = self
            .0
            .iter()
            .map(|metric_comparator_provider| {
                metric_comparator_provider.metric_comparator(phase.rankings())
            })
            .collect_vec();

        // all rankings of a slice come in tied at one rank, so any of them holds the rank the best
        // of them keeps
        let start_rank = slice.first().unwrap().1;

        let compare = |ranking1, ranking2| {
            for comparator in &comparators {
                let comparison = comparator.compare_rankings(ranking1, ranking2);
                if comparison != Comparison::Equal {
                    return comparison.to_ordering_for_sorting();
                }
            }
            Ordering::Equal
        };

        slice.sort_by(|&(ranking1, _), &(ranking2, _)| compare(ranking1, ranking2));

        let mut rank = start_rank;
        for index in 1..slice.len() {
            let (previous, current) = (slice[index - 1].0, slice[index].0);
            if compare(previous, current) != Ordering::Equal {
                rank = start_rank.saturating_add(index as u32);
            }
            slice[index].1 = rank;
        }
    }
}
