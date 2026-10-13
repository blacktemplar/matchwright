use std::{any::type_name, cmp::Ordering, fmt::Debug, num::NonZeroU32};

use dyn_clone::DynClone;
use itertools::Itertools;
use plugin_factory::{Plugin, axis, compatibility::Declaration};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::{PhaseData, Ranking, RankingId},
    type_map::TypeMapValue,
};

/// Ranks the rankings of a phase by its slice rankers in order, each one breaking the ties of the
/// ones before it.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HierarchicalRanker(Vec<Box<dyn SliceRanker>>);

impl HierarchicalRanker {
    /// A ranker applying `slice_rankers` in order. Without any slice ranker, all rankings are tied.
    pub fn new(slice_rankers: Vec<Box<dyn SliceRanker>>) -> Self {
        Self(slice_rankers)
    }

    /// The slice rankers, in the order they are applied in.
    pub fn slice_rankers(&self) -> &[Box<dyn SliceRanker>] {
        &self.0
    }

    /// The new rank of every ranking of `phase_data`.
    ///
    /// The ranks satisfy the following invariant: ordered by rank, the first ranking has rank 1 and
    /// every following one has either the rank of its predecessor or rank `index + 1`. Equal ranks
    /// are therefore ties, and the rank after a tie skips the tied ranks.
    pub fn update_ranks(&mut self, phase_data: &PhaseData) -> Vec<(RankingId, NonZeroU32)> {
        let mut rankings_with_new_ranks: Vec<(&Ranking, NonZeroU32)> = phase_data
            .ranking_ids_ordered_by_unique_rank()
            .iter()
            .map(|id| (&phase_data.rankings()[id], NonZeroU32::new(1).unwrap()))
            .collect();

        let mut slices = vec![rankings_with_new_ranks.as_mut_slice()];
        for slice_ranker in &mut self.0 {
            slices = slices
                .drain(..)
                .flat_map(|slice| {
                    slice_ranker.update_ranks(phase_data, slice);
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

/// Ranks rankings that are tied so far, as one step of a [`HierarchicalRanker`].
///
/// A slice ranker sorting by a single metric is best written as a [`RankingCriterion`]. A slice
/// ranker may keep state as a [`PhaseFormat`](super::phase_format::PhaseFormat) does.
#[axis]
pub trait SliceRanker: Plugin + DynClone {
    /// Reorders `slice` by the new ranks and assigns them.
    ///
    /// All rankings of `slice` come in with the same rank `r`. Afterwards, ordered as the slice
    /// is, the first ranking must have rank `r` and every following one either the rank of its
    /// predecessor or rank `r + index`.
    fn update_ranks(&mut self, phase_data: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]);
}

impl Clone for Box<dyn SliceRanker> {
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

/// A [`SliceRanker`] sorting by a single metric.
///
/// A criterion requires its [`Metric`](Self::Metric) by declaring [`requires_metric`] as its static
/// declaration, so its [`SliceRanker`] impl only has to call [`rank_by`]. A criterion that compares
/// its metric in an order other than the metric's own [`Compare`] impl should say in its name
/// which way round it sorts.
///
/// # Examples
/// ```
/// use std::num::NonZeroU32;
///
/// use plugin_factory::{Plugin, register};
/// use serde::{Deserialize, Serialize};
/// use tournament_engine::{
///     entities::competition::{PhaseData, Ranking},
///     plugins::{
///         metrics_calculator::points::Points,
///         ranker::{Comparison, RankingCriterion, SliceRanker, rank_by, requires_metric},
///     },
/// };
///
/// /// Ranks by the points, where fewer points are better.
/// #[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
/// #[plugin(
///     id = "my_crate::fewer_points",
///     label = "fewer points",
///     declare_static = requires_metric::<Self>
/// )]
/// struct FewerPoints;
///
/// impl RankingCriterion for FewerPoints {
///     type Metric = Points;
///
///     fn compare(&self, first: &Points, second: &Points) -> Comparison {
///         Comparison::less_is_better(first.0.total_cmp(&second.0))
///     }
/// }
///
/// #[register]
/// impl SliceRanker for FewerPoints {
///     fn update_ranks(&mut self, _: &PhaseData, slice: &mut [(&Ranking, NonZeroU32)]) {
///         rank_by(self, slice);
///     }
/// }
/// ```
pub trait RankingCriterion: Debug + Clone {
    /// The metric this criterion sorts by.
    type Metric: TypeMapValue + Sync;

    /// How a ranking whose metric is `first` compares to one whose metric is `second`.
    fn compare(&self, first: &Self::Metric, second: &Self::Metric) -> Comparison;
}

/// Declares that the criterion `C` requires its metric, which is what [`rank_by`] reads. Given to
/// the derive of a criterion as `declare_static = requires_metric::<Self>`.
pub fn requires_metric<C: RankingCriterion>(declaration: &mut Declaration) {
    declaration.require::<C::Metric>(type_name::<C::Metric>());
}

/// Reorders `slice` by `criterion` and assigns the new ranks, as [`SliceRanker::update_ranks`]
/// requires. Rankings that `criterion` finds equal stay tied, in the order they came in.
///
/// # Panics
/// Panics if a ranking of `slice` does not hold the metric of `criterion`, i.e. if the phase does
/// not calculate it.
pub fn rank_by<C: RankingCriterion>(criterion: &C, slice: &mut [(&Ranking, NonZeroU32)]) {
    let Some(&(_, start_rank)) = slice.first() else {
        return;
    };
    // This runs for every reported match result, so every value is looked up once here rather than
    // on every comparison of the sort below.
    let mut sorted = slice
        .iter()
        .map(|&(ranking, _)| {
            let metric = ranking
                .metric::<C::Metric>()
                .expect("the phase calculates the metric of every criterion");
            (ranking, metric)
        })
        .collect_vec();
    sorted.sort_by(|(_, first), (_, second)| {
        criterion.compare(first, second).to_ordering_for_sorting()
    });

    let mut rank = start_rank;
    for (index, &(ranking, metric)) in sorted.iter().enumerate() {
        if index > 0 && criterion.compare(sorted[index - 1].1, metric) != Comparison::Equal {
            rank = start_rank.saturating_add(index as u32);
        }
        slice[index] = (ranking, rank);
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

/// The one order a metric is naturally compared in, which a [`RankingCriterion`] over that metric
/// can sort by without naming a direction of its own.
///
/// Only implement this for a metric with a single obvious better end — more points, a later first
/// loss. Leave it out for one that counts several things, or that different phases want sorted
/// different ways round, so that every criterion over it has to state its direction.
pub trait Compare {
    fn compare(&self, other: &Self) -> Comparison;
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;
    use vec1::vec1;

    use super::*;
    use crate::{
        entities::competition::EntrantId,
        plugins::metrics_calculator::points::{MorePoints, Points},
    };

    /// The `index`th of a list of rankings, holding `points` if given.
    fn ranking(index: u128, points: Option<f32>) -> Ranking {
        let ranking = Ranking::new(
            vec1![EntrantId::new(Uuid::from_u128(index))],
            NonZeroU32::new(index as u32 + 1).unwrap(),
        );
        match points {
            Some(points) => ranking.with_metric(Points(points)),
            None => ranking,
        }
    }

    fn rank(rank: u32) -> NonZeroU32 {
        NonZeroU32::new(rank).unwrap()
    }

    #[test]
    fn rank_by_sorts_the_slice_and_ranks_from_the_rank_it_came_in_with() {
        let rankings: Vec<_> = (0..)
            .zip([1.0, 3.0, 1.0, 2.0])
            .map(|(index, points)| ranking(index, Some(points)))
            .collect();
        let mut slice: Vec<_> = rankings.iter().map(|ranking| (ranking, rank(5))).collect();

        rank_by(&MorePoints, &mut slice);

        let ranked: Vec<_> = slice
            .iter()
            .map(|(ranking, rank)| (ranking.id(), rank.get()))
            .collect();
        assert_eq!(
            ranked,
            [
                (rankings[1].id(), 5),
                (rankings[3].id(), 6),
                // the tie keeps the order the rankings came in, and the rank after it would skip
                // the tied ranks
                (rankings[0].id(), 7),
                (rankings[2].id(), 7),
            ]
        );
    }

    #[test]
    #[should_panic(expected = "the phase calculates the metric of every criterion")]
    fn rank_by_panics_if_a_ranking_lacks_the_metric() {
        let rankings = [ranking(0, Some(1.0)), ranking(1, None)];
        let mut slice: Vec<_> = rankings.iter().map(|ranking| (ranking, rank(1))).collect();

        rank_by(&MorePoints, &mut slice);
    }
}
