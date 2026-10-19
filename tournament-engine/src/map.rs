//! The map types of the engine.
//!
//! Every map in this crate is an [`IndexMap`] and therefore iterates in insertion order,
//! reproducibly across runs and architectures. Code may rely on that order, e.g. to derive a match
//! number from an index.
//!
//! Removing an entry has to preserve that order, so use
//! [`shift_remove`](IndexMap::shift_remove) rather than [`swap_remove`](IndexMap::swap_remove)
//! unless the order genuinely does not matter.

use std::{hash::Hash, marker::PhantomData, ops::Deref};

use indexmap::{IndexMap, map as index_map};
use rustc_hash::FxBuildHasher;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{SeqAccess, Visitor},
    ser::SerializeSeq,
};

/// An [`IndexMap`] with a fast, fixed hasher, suited to the small `Copy` id types this crate keys
/// its maps by.
///
/// Note that [`new`](IndexMap::new) and [`with_capacity`](IndexMap::with_capacity) are not
/// available on a map with a custom hasher; use [`Map::default`] and [`map_with_capacity`]
/// instead.
pub type Map<K, V> = IndexMap<K, V, FxBuildHasher>;

/// Creates an empty [`Map`] that can hold at least `capacity` entries without reallocating.
pub fn map_with_capacity<K, V>(capacity: usize) -> Map<K, V> {
    Map::with_capacity_and_hasher(capacity, FxBuildHasher)
}

/// A value that carries its own id, so that an [`IdMap`] can key it without being told the key.
pub trait IdAble {
    type Id;
    fn id(&self) -> Self::Id;
}

/// A [`Map`] of values that know their own id, keyed by that id.
///
/// Every entry is stored under `value.id()` and nothing can choose a key: values only go in through
/// [`insert`](Self::insert) or [`extend`](Extend::extend), which derive the key from the value, and
/// [`Deref`] hands out the read-only part of [`Map`] only.
///
/// Serializes as a plain sequence of its values, in insertion order, leaving the keys out and
/// rebuilding them when deserializing. A round trip therefore reproduces the order exactly.
#[derive(Debug)]
pub struct IdMap<V: IdAble>(Map<V::Id, V>);

impl<V: IdAble> Default for IdMap<V> {
    fn default() -> Self {
        Self(Map::default())
    }
}

impl<V: IdAble> IdMap<V> {
    /// Creates an empty map.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty map that can hold at least `capacity` entries without reallocating.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(map_with_capacity(capacity))
    }
}

/// Gives read-only access to the whole [`Map`] API. Every mutating operation is reimplemented on
/// [`IdMap`] itself, so that no caller can pick the key an entry is stored under.
impl<V: IdAble> Deref for IdMap<V> {
    type Target = Map<V::Id, V>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<V: IdAble> IdMap<V>
where
    V::Id: Eq + Hash,
{
    /// Inserts `value` under its own id, returning the value it replaced, if any.
    ///
    /// A value that replaces one already stored under the same id keeps that entry's position, so
    /// replacing never reorders the map.
    #[inline]
    pub fn insert(&mut self, value: V) -> Option<V> {
        self.0.insert(value.id(), value)
    }

    /// Returns a mutable reference to the value stored for `id`, or [`None`] if there is none.
    #[inline]
    pub fn get_mut(&mut self, id: &V::Id) -> Option<&mut V> {
        self.0.get_mut(id)
    }

    /// Returns an iterator that allows modifying every value, in insertion order.
    #[inline]
    pub fn values_mut(&mut self) -> index_map::ValuesMut<'_, V::Id, V> {
        self.0.values_mut()
    }

    /// Removes the entry for `id` and returns its value, or [`None`] if there was none.
    ///
    /// The remaining entries keep their relative order, which costs `O(n)`.
    #[inline]
    pub fn remove(&mut self, id: &V::Id) -> Option<V> {
        self.0.shift_remove(id)
    }
}

/// Serializes as a plain sequence of the values, in insertion order, leaving their ids out.
impl<V: IdAble + Serialize> Serialize for IdMap<V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for v in self.0.values() {
            seq.serialize_element(v)?;
        }
        seq.end()
    }
}

struct IdMapVisitor<V>(PhantomData<V>);

impl<'de, V: IdAble + Deserialize<'de>> Visitor<'de> for IdMapVisitor<V>
where
    V::Id: Eq + Hash,
{
    type Value = IdMap<V>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a sequence of IdAble values")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut map = IdMap::with_capacity(seq.size_hint().unwrap_or(0));

        while let Some(value) = seq.next_element::<V>()? {
            map.insert(value);
        }

        Ok(map)
    }
}

impl<'de, V: IdAble + Deserialize<'de>> Deserialize<'de> for IdMap<V>
where
    V::Id: Eq + Hash,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(IdMapVisitor(PhantomData::<V>))
    }
}

// Implemented manually because deriving would miss V::Id: Clone
impl<V: IdAble + Clone> Clone for IdMap<V>
where
    V::Id: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// Implemented manually because deriving would miss V::Id: Eq + Hash
impl<V: IdAble + PartialEq> PartialEq for IdMap<V>
where
    V::Id: Eq + Hash,
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<V: IdAble + PartialEq> Eq for IdMap<V> where V::Id: Eq + Hash {}

/// Iterates in insertion order.
impl<V: IdAble> IntoIterator for IdMap<V> {
    type Item = V;
    type IntoIter = index_map::IntoValues<V::Id, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

/// Iterates in insertion order.
impl<'a, V: IdAble> IntoIterator for &'a IdMap<V> {
    type Item = &'a V;
    type IntoIter = index_map::Values<'a, V::Id, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.values()
    }
}

impl<V: IdAble> Extend<V> for IdMap<V>
where
    V::Id: Eq + Hash,
{
    #[inline]
    fn extend<I: IntoIterator<Item = V>>(&mut self, iter: I) {
        self.0.extend(iter.into_iter().map(|v| (v.id(), v)));
    }
}

impl<V: IdAble> FromIterator<V> for IdMap<V>
where
    V::Id: Eq + Hash,
{
    #[inline]
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        Self(iter.into_iter().map(|v| (v.id(), v)).collect())
    }
}
