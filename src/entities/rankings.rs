use std::{any::TypeId, cmp::Ordering, fmt, hash::Hash, num::NonZeroU32, ptr};

use itertools::Itertools;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
    ser::{self, SerializeSeq, SerializeStruct, SerializeTupleStruct},
};
use thiserror::Error;
use uuid::{Bytes, Uuid, uuid};
use vec1::Vec1;

use crate::{
    entities::competition::EntrantId,
    map::{IdAble, IdMap, IdMapSeed, Map, serialize_id_map_with},
    plugins::metrics_calculator::{Metric, MetricId, MetricKind, MetricsCalculator},
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
/// deserializing included, which is also why the layout itself is not part of the serialized form.
///
/// The layout is what lets the metrics be serialized without a type tag of their own: the
/// [kind](MetricKind) of the calculator at a position says what the metric at that position is, so
/// the calculators have to be read back before the rankings. See the [`Serialize`] impl for how
/// that ordering is made part of the serialized form rather than left to chance.
///
/// # Invariant
/// The metrics of every ranking in `rankings` follow `metrics_layout`, in the sense of
/// `is_layout_of`. Reading a metric resolves its position through the layout and then casts without
/// checking it ([`RankingView::metric`] and [`MetricComparator::compare_rankings`]), so this
/// invariant is what keeps those casts sound. Three places establish it, and nothing else may write
/// a metric: `initialize_metrics` for rankings that are played, `RankingsVisitor` for rankings that
/// are read back, and `set_metric`, which can only ever replace a metric by one of the same type.
#[derive(Debug)]
pub struct Rankings {
    metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    metrics_layout: Map<MetricId, usize>,
    rankings: IdMap<Ranking>,
    ranking_ids_ordered_by_unique_rank: Vec<RankingId>,
}

/// Why a serialized [`Rankings`] cannot be read back as one.
#[derive(Error, Debug)]
enum InvalidRankings {
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
/// that [`metric_kind`](MetricsCalculator::metric_kind) returns a kind of the same type on every
/// call, since the layout is built from one call of it and the metrics of a ranking may come from
/// another, and that no two calculators of a phase produce the same metric type, since the layout
/// can hold only one position per type.
fn is_layout_of(
    calculator_count: usize,
    layout: &Map<MetricId, usize>,
    metrics: &[Metric],
) -> bool {
    metrics.len() == calculator_count
        && metrics
            .iter()
            .enumerate()
            .all(|(index, metric)| layout.get(&(**metric).type_id()) == Some(&index))
}

/// The [kind](MetricKind) of the metric of every calculator, at the position of that calculator.
///
/// Everything about the metric types of a rankings comes from one call of this — the layout, the
/// initial values, the serialized form — so that a calculator whose kind differs from one call of
/// [`MetricsCalculator::metric_kind`] to the next cannot make two of them disagree.
fn metric_kinds(metrics_calculators: &[Box<dyn MetricsCalculator>]) -> Vec<MetricKind> {
    metrics_calculators
        .iter()
        .map(|calculator| calculator.metric_kind())
        .collect()
}

/// The [layout](Rankings#the-layout) of `metric_kinds`: the position of every calculator, keyed by
/// the type of the metric it produces.
fn layout(metric_kinds: &[MetricKind]) -> Map<MetricId, usize> {
    metric_kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| (kind.metric_id(), index))
        .collect()
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
        let metrics_layout = layout(&metric_kinds(&metrics_calculators));
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
        let metrics_layout = layout(&metric_kinds(&metrics_calculators));
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
                .map(|c| c.metric_kind().into_default())
                .collect();
            // The layout was built from one call of `metric_kind` per calculator and the metrics
            // just now from another, so this is where the two are checked against each other:
            // reading a metric by its position casts it without checking, and the two agreeing is
            // what keeps that sound. Every later write puts a metric at the position of its own
            // type, which cannot break the agreement again, so this one check covers the whole
            // lifetime of the rankings. It runs once per rankings initialize, which is cheap
            // enough to be a real assertion rather than a debug one.
            assert!(
                is_layout_of(
                    self.metrics_calculators.len(),
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

/// The name [`Rankings`] is serialized under, and the number of elements it is serialized as.
const RANKINGS: &str = "Rankings";
const RANKINGS_ELEMENTS: usize = 3;

/// The name and the fields [`Ranking`] is serialized under.
///
/// # Invariant
/// These must stay the fields of [`Ranking`], in name and in order, and `SerializedRanking` and
/// `RankingVisitor` must both write and read exactly these, in this order: a format that is not
/// self describing, like [`postcard`](https://docs.rs/postcard), writes nothing but the values in
/// that order, so a ranking would otherwise silently read back as something else. The round trip
/// tests through both formats are what keeps the three in sync.
const RANKING: &str = "Ranking";
const RANKING_FIELDS: &[&str] = &[
    "id",
    "entrants",
    "initial_unique_rank",
    "unique_rank",
    "rank",
    "metrics",
];

/// Serializes as a tuple of the metrics calculators, the rankings and their order, rather than as
/// a struct of those three.
///
/// The metrics of a ranking carry no type tag — what they are follows from the
/// [layout](Rankings#the-layout) of the calculators — so the calculators have to be read back
/// before the rankings. A tuple is what makes that hold in every format: the fields of a struct
/// arrive in whatever order the input happens to have them in, since a self describing format
/// names them, while the elements of a tuple can only arrive in the order they were written. The
/// ordering is therefore a property of the serialized form rather than a requirement on whoever
/// produced it.
impl Serialize for Rankings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let metric_kinds = metric_kinds(&self.metrics_calculators);
        let mut rankings = serializer.serialize_tuple_struct(RANKINGS, RANKINGS_ELEMENTS)?;
        rankings.serialize_field(&self.metrics_calculators)?;
        rankings.serialize_field(&SerializedRankings {
            rankings: &self.rankings,
            metric_kinds: &metric_kinds,
        })?;
        rankings.serialize_field(&self.ranking_ids_ordered_by_unique_rank)?;
        rankings.end()
    }
}

/// The rankings of a [`Rankings`] as they are serialized: the form of an [`IdMap`], with every
/// ranking written against the kinds of the calculators.
struct SerializedRankings<'a> {
    rankings: &'a IdMap<Ranking>,
    metric_kinds: &'a [MetricKind],
}

impl Serialize for SerializedRankings<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_id_map_with(self.rankings, serializer, |ranking| SerializedRanking {
            ranking,
            metric_kinds: self.metric_kinds,
        })
    }
}

/// A [`Ranking`] as it is serialized: its fields, with the metrics written as the types the
/// calculators say they are. [`Ranking`] has no [`Serialize`] impl of its own, since it cannot be
/// written without those kinds.
struct SerializedRanking<'a> {
    ranking: &'a Ranking,
    metric_kinds: &'a [MetricKind],
}

impl Serialize for SerializedRanking<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut ranking = serializer.serialize_struct(RANKING, RANKING_FIELDS.len())?;
        ranking.serialize_field(RANKING_FIELDS[0], &self.ranking.id)?;
        ranking.serialize_field(RANKING_FIELDS[1], &self.ranking.entrants)?;
        ranking.serialize_field(RANKING_FIELDS[2], &self.ranking.initial_unique_rank)?;
        ranking.serialize_field(RANKING_FIELDS[3], &self.ranking.unique_rank)?;
        ranking.serialize_field(RANKING_FIELDS[4], &self.ranking.rank)?;
        ranking.serialize_field(
            RANKING_FIELDS[5],
            &SerializedMetrics {
                metrics: &self.ranking.metrics,
                metric_kinds: self.metric_kinds,
            },
        )?;
        ranking.end()
    }
}

/// The metrics of one ranking, written as a plain sequence of untagged values: which of them is
/// which follows from their position, so the type each one is written as is the one the kind at
/// that position describes.
struct SerializedMetrics<'a> {
    metrics: &'a [Metric],
    metric_kinds: &'a [MetricKind],
}

impl Serialize for SerializedMetrics<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut metrics = serializer.serialize_seq(Some(self.metrics.len()))?;
        for (metric, kind) in self.metrics.iter().zip(self.metric_kinds) {
            // the metric is of the type of the kind at its position by the invariant of `Rankings`
            // — unless a calculator returned a kind of a different type than it did when the
            // metrics were written, which is the one case the cast can fail in
            let metric = kind.erase(metric).ok_or_else(|| {
                ser::Error::custom(
                    "every metrics calculator of a phase must produce a metric of the same type \
                     on every call",
                )
            })?;
            metrics.serialize_element(&ErasedMetric(metric))?;
        }
        metrics.end()
    }
}

/// A metric that has been cast back to its own type, so that it serializes as that type rather
/// than as a [type erased](TypeMapValue) box, which would carry a type tag.
struct ErasedMetric<'a>(&'a dyn erased_serde::Serialize);

impl Serialize for ErasedMetric<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        erased_serde::serialize(self.0, serializer)
    }
}

/// Reads back what the [`Serialize`] impl writes; see there for why the order of the three
/// elements is what makes this work.
impl<'de> Deserialize<'de> for Rankings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple_struct(RANKINGS, RANKINGS_ELEMENTS, RankingsVisitor)
    }
}

struct RankingsVisitor;

impl<'de> Visitor<'de> for RankingsVisitor {
    type Value = Rankings;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("the metrics calculators, the rankings and the order of the rankings")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let metrics_calculators: Vec<Box<dyn MetricsCalculator>> = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        // one call of `metric_kind` per calculator, used both to build the layout and to read the
        // metrics, so that the metric at a position is of the type the layout gives that position
        // whatever a later call of `metric_kind` would say
        let metric_kinds = metric_kinds(&metrics_calculators);
        let metrics_layout = layout(&metric_kinds);
        let rankings: IdMap<Ranking> = seq
            .next_element_seed(IdMapSeed(RankingSeed {
                metric_kinds: &metric_kinds,
            }))?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let ranking_ids_ordered_by_unique_rank = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(2, &self))?;

        // Establishes the invariant of `Rankings` for rankings that are read rather than played.
        // The metrics came from the very kinds the layout was built from, so this can only fail
        // for calculators that produce fewer distinct metric types than there are calculators,
        // which would leave the layout with no position for one of them.
        for ranking in rankings.values() {
            if !is_layout_of(metrics_calculators.len(), &metrics_layout, &ranking.metrics) {
                return Err(de::Error::custom(
                    InvalidRankings::MetricsDoNotFollowLayout(ranking.id()),
                ));
            }
        }

        Ok(Rankings {
            metrics_calculators,
            metrics_layout,
            rankings,
            ranking_ids_ordered_by_unique_rank,
        })
    }
}

/// Reads a [`Ranking`] whose metrics are the ones `metric_kinds` describes. [`Ranking`] has no
/// [`Deserialize`] impl of its own, since it cannot be read back without those kinds.
#[derive(Clone, Copy)]
struct RankingSeed<'a> {
    metric_kinds: &'a [MetricKind],
}

impl<'de> DeserializeSeed<'de> for RankingSeed<'_> {
    type Value = Ranking;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_struct(RANKING, RANKING_FIELDS, RankingVisitor(self))
    }
}

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum RankingField {
    Id,
    Entrants,
    InitialUniqueRank,
    UniqueRank,
    Rank,
    Metrics,
}

struct RankingVisitor<'a>(RankingSeed<'a>);

impl<'de> Visitor<'de> for RankingVisitor<'_> {
    type Value = Ranking;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a ranking")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let metrics_seed = MetricsSeed {
            metric_kinds: self.0.metric_kinds,
        };
        Ok(Ranking {
            id: seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(0, &self))?,
            entrants: seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(1, &self))?,
            initial_unique_rank: seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(2, &self))?,
            unique_rank: seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(3, &self))?,
            rank: seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(4, &self))?,
            metrics: seq
                .next_element_seed(metrics_seed)?
                .ok_or_else(|| de::Error::invalid_length(5, &self))?,
        })
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut id = None;
        let mut entrants = None;
        let mut initial_unique_rank = None;
        let mut unique_rank = None;
        let mut rank = None;
        let mut metrics = None;

        while let Some(field) = map.next_key()? {
            /// Reads the value of `field` into `slot`, which no earlier field may have filled.
            macro_rules! read {
                ($slot:ident, $index:expr, $value:expr) => {{
                    if $slot.is_some() {
                        return Err(de::Error::duplicate_field(RANKING_FIELDS[$index]));
                    }
                    $slot = Some($value);
                }};
            }
            match field {
                RankingField::Id => read!(id, 0, map.next_value()?),
                RankingField::Entrants => read!(entrants, 1, map.next_value()?),
                RankingField::InitialUniqueRank => {
                    read!(initial_unique_rank, 2, map.next_value()?)
                }
                RankingField::UniqueRank => read!(unique_rank, 3, map.next_value()?),
                RankingField::Rank => read!(rank, 4, map.next_value()?),
                RankingField::Metrics => read!(
                    metrics,
                    5,
                    map.next_value_seed(MetricsSeed {
                        metric_kinds: self.0.metric_kinds,
                    })?
                ),
            }
        }

        /// The value read into `slot`, or the error that names the field it is missing from.
        macro_rules! required {
            ($slot:ident, $index:expr) => {
                $slot.ok_or_else(|| de::Error::missing_field(RANKING_FIELDS[$index]))?
            };
        }
        Ok(Ranking {
            id: required!(id, 0),
            entrants: required!(entrants, 1),
            initial_unique_rank: required!(initial_unique_rank, 2),
            unique_rank: required!(unique_rank, 3),
            rank: required!(rank, 4),
            metrics: required!(metrics, 5),
        })
    }
}

/// Reads the metrics of one ranking: one metric per kind, of the type of the kind at its position.
#[derive(Clone, Copy)]
struct MetricsSeed<'a> {
    metric_kinds: &'a [MetricKind],
}

impl<'de> DeserializeSeed<'de> for MetricsSeed<'_> {
    type Value = Vec<Metric>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for MetricsSeed<'_> {
    type Value = Vec<Metric>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "one metric per metrics calculator, {} of them",
            self.metric_kinds.len()
        )
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut metrics = Vec::with_capacity(self.metric_kinds.len());
        for (index, kind) in self.metric_kinds.iter().enumerate() {
            metrics.push(
                seq.next_element_seed(MetricSeed(kind))?
                    .ok_or_else(|| de::Error::invalid_length(index, &self))?,
            );
        }
        // a ranking with more metrics than the phase has calculators has no position for the
        // extra ones, and a format that does not check the length of a sequence itself would
        // otherwise let them pass silently
        if seq.next_element::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(
                self.metric_kinds.len() + 1,
                &self,
            ));
        }
        Ok(metrics)
    }
}

/// Reads one metric as the type its [`MetricKind`] describes, which is what the type tag would
/// have said if the metric carried one.
struct MetricSeed<'a>(&'a MetricKind);

impl<'de> DeserializeSeed<'de> for MetricSeed<'_> {
    type Value = Metric;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut deserializer = <dyn erased_serde::Deserializer>::erase(deserializer);
        self.0
            .deserialize_metric(&mut deserializer)
            .map_err(de::Error::custom)
    }
}

const NAMESPACE_RANKING: Uuid = uuid!("eb5e6d16-15af-42e1-b0f4-f5a643fd1e9a");

/// One entrant or team of entrants in the [`Rankings`] of a phase, its rank and its metrics.
///
/// A ranking is neither [`Serialize`] nor [`Deserialize`]: its metrics are stored
/// [type erased](TypeMapValue) and written without a type tag, so it can only be read or written
/// as part of the rankings it belongs to, which knows the [kinds](MetricKind) of those metrics.
/// See `SerializedRanking` and `RankingSeed`.
#[derive(Debug)]
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
