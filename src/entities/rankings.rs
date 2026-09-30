use std::{any::TypeId, cmp::Ordering, hash::Hash, num::NonZeroU32, ptr};

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::{Bytes, Uuid, uuid};
use vec1::Vec1;

use crate::{
    entities::competition::EntrantId,
    map::{IdAble, IdMap, Map},
    plugins::metrics_calculator::{Metric, MetricId, MetricsCalculator},
    type_map::{TypeMapValue, downcast_unchecked_ref},
};

/// The rankings of a [`Phase`](crate::entities::phase::Phase): one [`Ranking`] per entrant or team
/// of entrants, the metrics calculated for every one of them, and the order the rankings are in.
///
/// # The layout
/// A ranking holds its metrics [type erased](TypeMapValue) in a plain [`Vec`], and the layout is
/// what says which of them is which: the position the metric of every calculator has in that vec,
/// keyed by the type of that metric. It is not stored next to the calculators but derived from
/// them, so the two can never disagree — it is (re)built whenever the calculators are set,
/// deserializing included, which is also why only the calculators are part of the serialized form.
///
/// # Invariant
/// The metrics of every ranking in `rankings` follow `metrics_layout`, in the sense of
/// `is_layout_of`. Reading a metric resolves its position through the layout and then casts without
/// checking it ([`RankingView::metric`] and [`MetricComparator::compare_rankings`]), so this
/// invariant is what keeps those casts sound. Three places establish it, and nothing else may write
/// a metric: `initialize_metrics` for rankings that are played, the `TryFrom<SerializedRankings>`
/// impl for rankings that are read back, and `set_metric`, which can only ever replace a metric by
/// one of the same type.
#[derive(Debug, Serialize, Deserialize)]
#[serde(try_from = "SerializedRankings")]
pub struct Rankings {
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    #[serde(skip)]
    metrics_layout: Map<MetricId, usize>,
    rankings: IdMap<Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
}

/// The fields of a [`Rankings`] as they were serialized, before anything about them is checked.
///
/// A rankings deserializes through this rather than directly, so that [`TryFrom`] gets to reject
/// one whose metrics do not follow the [layout](Rankings#the-layout) of its calculators before it
/// exists as a [`Rankings`]. Nothing else establishes that invariant for deserialized rankings: it
/// is [`initialize_metrics`](Rankings::initialize_metrics) that does so for rankings that are
/// played rather than read, and deserializing never goes through it.
///
/// # Invariant
/// The fields must stay the serialized fields of [`Rankings`], in name and in order: a format that
/// is not self describing, like [`postcard`](https://docs.rs/postcard), writes nothing but the
/// values in declaration order, so rankings would silently read back as something else. The round
/// trip tests through both formats are what keeps the two in sync.
#[derive(Serialize, Deserialize)]
struct SerializedRankings {
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    rankings: IdMap<Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
}

/// Why a serialized [`Rankings`] cannot be read back as one.
#[derive(Error, Debug)]
pub enum InvalidRankings {
    #[error(
        "the metrics of ranking {0:?} do not follow the layout of the metrics calculators of \
         the rankings"
    )]
    MetricsDoNotFollowLayout(RankingId),
}

/// Whether `metrics` is laid out by `layout`, that is whether it holds one metric per calculator
/// and every one of them at the position the layout gives its type.
///
/// This is what everything that reads a metric by its position rather than by its type relies on,
/// [`Rankings::metric_index`] and hence [`RankingView`] and [`MetricComparator`] above all. It can
/// only fail for a [`MetricsCalculator`] that breaks one of two rules that nothing else enforces:
/// that [`default_metric_value`](MetricsCalculator::default_metric_value) returns a metric of the
/// same type on every call, since the layout is built from one call of it and the metrics of a
/// ranking from another, and that no two calculators of a phase produce the same metric type, since
/// the layout can hold only one position per type.
fn is_layout_of(
    calculators: &[Box<dyn MetricsCalculator>],
    layout: &Map<MetricId, usize>,
    metrics: &[Metric],
) -> bool {
    metrics.len() == calculators.len()
        && metrics
            .iter()
            .enumerate()
            .all(|(index, metric)| layout.get(&(**metric).type_id()) == Some(&index))
}

/// The [layout](Rankings#the-layout) of `metrics_calculators`: the position of every calculator,
/// keyed by the type of the metric it produces.
fn layout(metrics_calculators: &Vec<Box<dyn MetricsCalculator + 'static>>) -> Map<MetricId, usize> {
    metrics_calculators
        .iter()
        .enumerate()
        .map(|(i, c)| ((*c.default_metric_value()).type_id(), i))
        .collect()
}

impl TryFrom<SerializedRankings> for Rankings {
    type Error = InvalidRankings;

    fn try_from(value: SerializedRankings) -> Result<Self, Self::Error> {
        let layout = layout(&value.metrics_calculators);

        // Check that all metrics of all rankings follow the layout
        for ranking in value.rankings.values() {
            if !is_layout_of(&value.metrics_calculators, &layout, &ranking.metrics) {
                return Err(InvalidRankings::MetricsDoNotFollowLayout(ranking.id()));
            }
        }
        Ok(Self {
            metrics_calculators: value.metrics_calculators,
            metrics_layout: layout,
            rankings: value.rankings,
            ranking_ids_ordered_by_unique_rank: value.ranking_ids_ordered_by_unique_rank,
        })
    }
}

impl Rankings {
    pub(crate) fn metric_index<T: 'static>(&self) -> Option<usize> {
        self.metrics_layout.get(&TypeId::of::<T>()).copied()
    }

    #[expect(clippy::borrowed_box)]
    fn metric_index_of_box(&self, v: &Box<dyn TypeMapValue>) -> usize {
        *self
            .metrics_layout
            .get(&(**v).type_id())
            .expect("Inner type of box is part of layout")
    }

    pub(crate) fn new(metrics_calculators: Vec<Box<dyn MetricsCalculator + 'static>>) -> Self {
        let metrics_layout = layout(&metrics_calculators);
        Self {
            metrics_calculators,
            metrics_layout,
            rankings: IdMap::default(),
            ranking_ids_ordered_by_unique_rank: vec![],
        }
    }

    #[cfg(test)]
    pub(super) fn new_with(
        metrics_calculators: Vec<Box<dyn MetricsCalculator + 'static>>,
        rankings: Vec<Ranking>,
    ) -> Self {
        let metrics_layout = layout(&metrics_calculators);
        let ranking_ids_ordered_by_unique_rank = rankings.iter().map(Ranking::id).collect();
        let mut rankings = Self {
            metrics_calculators,
            metrics_layout,
            rankings: rankings.into_iter().collect(),
            ranking_ids_ordered_by_unique_rank,
        };
        rankings.initialize_metrics();
        rankings
    }

    pub fn ranking_view(&self, ranking_id: &RankingId) -> RankingView<'_> {
        RankingView(self, &self.rankings[ranking_id])
    }

    pub fn ranking_views(&self) -> impl ExactSizeIterator<Item = RankingView<'_>> {
        self.rankings.values().map(|r| RankingView(self, r))
    }

    pub fn ranking_ids_ordered_by_unique_rank(&self) -> &[RankingId] {
        &self.ranking_ids_ordered_by_unique_rank
    }

    pub(crate) fn initialize(&mut self, ranking_entrants: Vec<Vec1<EntrantId>>) {
        let (ordered_ranking_ids, rankings) = ranking_entrants
            .into_iter()
            .enumerate()
            .map(|(index, entrants)| {
                let ranking = Ranking::new(entrants, NonZeroU32::new(index as u32 + 1).unwrap());
                (ranking.id(), ranking)
            })
            .unzip();
        self.rankings = rankings;
        self.ranking_ids_ordered_by_unique_rank = ordered_ranking_ids;
        self.initialize_metrics();
    }

    /// Gives every ranking the default metric of every calculator, which is what establishes the
    /// [invariant](Rankings#invariant) of the rankings for those metrics.
    ///
    /// # Panics
    /// Panics if those metrics do not follow the layout of the calculators, see [`is_layout_of`].
    fn initialize_metrics(&mut self) {
        for ranking in self.rankings.values_mut() {
            ranking.metrics = self
                .metrics_calculators
                .iter()
                .map(|c| c.default_metric_value())
                .collect();
            // The layout was built from one call of `default_metric_value` per calculator and the
            // metrics just now from another, so this is where the two are checked against each
            // other: reading a metric by its position casts it without checking, and the two
            // agreeing is what keeps that sound. Every later write puts a metric at the position
            // of its own type, which cannot break the agreement again, so this one check covers
            // the whole lifetime of the rankings. It runs once per rankings initialize, which is
            // cheap enough to be a real assertion rather than a debug one.
            assert!(
                is_layout_of(
                    &self.metrics_calculators,
                    &self.metrics_layout,
                    &ranking.metrics
                ),
                "every metrics calculator of a phase must produce a metric of the same type on \
                 every call, and no two of them the same type"
            );
        }
    }

    pub fn contains(&self, ranking_id: &RankingId) -> bool {
        self.rankings.contains_key(ranking_id)
    }

    pub(super) fn apply_new_ranks(&mut self, new_ranks: Vec<(RankingId, NonZeroU32)>) {
        for (rank_id, new_rank) in new_ranks {
            self.rankings.get_mut(&rank_id).unwrap().rank = new_rank;
        }
        self.update_ranking_ordering_and_unique_ranks();
    }

    /// Reestablishes the two things that follow from the ranks:
    /// [`Self::ranking_ids_ordered_by_unique_rank`] and the unique rank of every ranking, which is
    /// the rank with ties broken by [`Ranking::initial_unique_rank`].
    fn update_ranking_ordering_and_unique_ranks(&mut self) {
        // `sort_by_key` would look every ranking up twice per comparison, whereas the cached
        // variant looks every ranking up exactly once
        self.ranking_ids_ordered_by_unique_rank
            .sort_by_cached_key(|id| {
                let ranking = &self.rankings[id];
                (ranking.rank(), ranking.initial_unique_rank())
            });
        // set unique ranks
        for (index, ranking_id) in self.ranking_ids_ordered_by_unique_rank.iter().enumerate() {
            self.rankings.get_mut(ranking_id).unwrap().unique_rank =
                NonZeroU32::new(index as u32 + 1).unwrap()
        }
        // check requirements of [`Ranker::update_ranks`]
        debug_assert!(
            self.rankings[&self.ranking_ids_ordered_by_unique_rank[0]]
                .rank
                .get()
                == 1
                && self
                    .ranking_ids_ordered_by_unique_rank
                    .iter()
                    .map(|id| &self.rankings[id])
                    .enumerate()
                    .array_windows()
                    .all(|[(_, r1), (i2, r2)]| r1.rank == r2.rank || r2.rank.get() == i2 as u32 + 1)
        );
    }

    /// Guarantees to not change self.metrics_calculators
    pub(super) fn set_metric(
        &mut self,
        ranking_id: RankingId,
        metric: Box<dyn TypeMapValue>,
    ) -> Box<dyn TypeMapValue> {
        let new_type = (*metric).type_id();
        let index = self.metric_index_of_box(&metric);
        let old_metric = std::mem::replace(
            &mut self
                .rankings
                .get_mut(&ranking_id)
                .expect("Ranking must exist")
                .metrics[index],
            metric,
        );
        // the type of metrics should never change which is important to uphold
        // the invariant that metrics of rankings adhere to the layout. Note the
        // dereference: `old_metric.type_id()` would be the type id of the box.
        assert!(new_type == (*old_metric).type_id());
        old_metric
    }

    pub(super) fn metrics_calculators(&self) -> &[Box<dyn MetricsCalculator>] {
        &self.metrics_calculators
    }
}

const NAMESPACE_RANKING: Uuid = uuid!("eb5e6d16-15af-42e1-b0f4-f5a643fd1e9a");

#[derive(Debug, Serialize, Deserialize)]
pub struct Ranking {
    id: RankingId,
    entrants: Vec1<EntrantId>,
    initial_unique_rank: NonZeroU32,
    unique_rank: NonZeroU32,
    rank: NonZeroU32,
    metrics: Vec<Metric>,
}

impl IdAble for Ranking {
    type Id = RankingId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

pub fn bytes_from_uuids(uuids: impl Iterator<Item = Uuid>) -> Vec<u8> {
    uuids.flat_map(|uuid| uuid.into_bytes()).collect()
}

impl Ranking {
    pub fn new(entrants: Vec1<EntrantId>, unique_rank: NonZeroU32) -> Self {
        let id = RankingId(Uuid::new_v5(
            &NAMESPACE_RANKING,
            &bytes_from_uuids(entrants.iter().map(|e| e.uuid())),
        ));
        Self {
            id,
            entrants,
            initial_unique_rank: unique_rank,
            unique_rank,
            rank: NonZeroU32::new(1).unwrap(),
            metrics: Vec::new(),
        }
    }

    pub fn id(&self) -> RankingId {
        self.id
    }

    pub fn entrants(&self) -> &Vec1<EntrantId> {
        &self.entrants
    }

    /// The unique rank of the ranking, which is guaranteed to be [`Ranking::rank`] with tie
    /// breaking equal values by their [`Ranking::initial_unique_rank`].
    pub fn unique_rank(&self) -> NonZeroU32 {
        self.unique_rank
    }

    pub fn rank(&self) -> NonZeroU32 {
        self.rank
    }

    pub fn initial_unique_rank(&self) -> NonZeroU32 {
        self.initial_unique_rank
    }
}

/// One ranking part of a [`Rankings`], together with that rankings, which is what makes the ranking
/// readable from outside this crate: a [`Ranking`] on its own knows neither which rankings it
/// belongs to nor which of its type erased metrics is which.
///
/// # Invariant
/// The ranking must be part of `Rankings`. Only this module constructs a view, and the two
/// places that do take the ranking out of the very `rankings` they pair it with. That is what makes
/// the metrics of the ranking follow the [layout](Rankings#the-layout) of the rankings, which
/// [`Self::metric`] and [`MetricComparator::compare_rankings`] rely on to read a metric by its
/// position without checking its type.
#[derive(Debug, Clone, Copy)]
pub struct RankingView<'a>(&'a Rankings, &'a Ranking);

impl<'a> RankingView<'a> {
    pub fn id(&self) -> RankingId {
        self.1.id
    }

    pub fn entrants(&self) -> &'a Vec1<EntrantId> {
        &self.1.entrants
    }

    pub fn initial_unique_rank(&self) -> NonZeroU32 {
        self.1.initial_unique_rank
    }

    /// The unique rank of the ranking, which is guaranteed to be [`Self::rank`] with ties broken by
    /// [`Self::initial_unique_rank`].
    pub fn unique_rank(&self) -> NonZeroU32 {
        self.1.unique_rank
    }

    pub fn rank(&self) -> NonZeroU32 {
        self.1.rank
    }

    /// The metrics of this ranking.
    pub fn metric<T: 'static>(&self) -> &'a T {
        let metric = &self.1.metrics[self.0.metric_index::<T>().unwrap()];
        // SAFETY: We guarantee that `self.1` is part of `self.0.rankings` and `Rankings`
        // guarantees that accessing the metrics of any of its rankings at index
        // `Rankings::metric_index::<T>` returns a box with underlying type `T`.
        unsafe { downcast_unchecked_ref(&**metric) }
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

pub struct MetricComparator<'a> {
    /// Kept only to ensure that the rankings will not change while this is alive
    rankings: &'a Rankings,
    /// The index of the metric according to the layout
    index: usize,
    /// Compares two values of the metric.
    /// # Safety
    /// The two passed in `Metric` values need to be metrics extracted from `self.index` of a
    /// ranking in `self.rankings.rankings`.
    compare_values: unsafe fn(&Metric, &Metric) -> Comparison,
}

/// Compares two type erased values of the metric `M`.
///
/// # Safety
/// Both values must have the concrete type `M`.
unsafe fn compare_values<M: Compare + 'static>(value1: &Metric, value2: &Metric) -> Comparison {
    // SAFETY: the caller guarantees that both values are of type `M`.
    unsafe { downcast_unchecked_ref::<M>(&**value1).compare(downcast_unchecked_ref(&**value2)) }
}

impl<'a> MetricComparator<'a> {
    pub fn new<M: Compare + 'static>(rankings: &'a Rankings) -> Self {
        let index = rankings.metric_index::<M>().expect("metric exists");
        // compare_values::<M> is safe if both input metrics have the concrete type M which is
        // given if the safety requirement of `Self.compare_values` is satisfied that says that
        // both values must be values from rankings inside `self.rankings.rankings` at index
        // `self.index` which is guaranteed to be of type `M` due to the guarantee that all metrics
        // in all rankings of a rankings adhere to the rankings layout.
        let compare_values = compare_values::<M>;
        Self {
            rankings,
            index,
            compare_values,
        }
    }

    pub fn compare_rankings(&self, ranking1: RankingView, ranking2: RankingView) -> Comparison {
        // the asserts are needed to make this function safe
        assert!(ptr::eq(ranking1.0, self.rankings));
        assert!(ptr::eq(ranking2.0, self.rankings));

        // Safety: `RankingView` guarantees that .1 is a ranking in `.0.rankings` which we asserted
        // above is equal to `self.rankings.rankings`. Furthermore, `Self` guarantees that
        // `index` is one of the positions of `self.rankings.metrics_layout` and `Rankings`
        // guarantees that the metrics of any ranking inside `Rankings::rankings` follow
        // `Rankings::metrics_layout`.
        let (metric1, metric2) = unsafe {
            (
                ranking1.1.metrics.get_unchecked(self.index),
                ranking2.1.metrics.get_unchecked(self.index),
            )
        };

        // Safety: both metrics came from index `self.index` from `ranking1` and `ranking2`
        // which are guaranteed to be part of `self.rankings.rankings` (see above argumentation).
        unsafe { (self.compare_values)(metric1, metric2) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RankingId(Uuid);

impl RankingId {
    pub fn nil() -> Self {
        Self(Uuid::nil())
    }

    pub fn into_bytes(self) -> Bytes {
        self.0.into_bytes()
    }
}
