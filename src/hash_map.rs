//! A hash map with a fixed, deterministic hasher.
//!
//! [`HashMap`] wraps [`FxHashMap`] so that the engine has a single place to control which
//! operations are available and how they behave. Going through this wrapper instead of
//! [`std::collections::HashMap`] guarantees that no part of the engine accidentally uses a
//! randomly seeded hasher, which would make iteration order (and therefore anything derived from
//! it) non-reproducible across runs.
//!
//! The API is intentionally a subset of the standard library one, and deviates from it where that
//! buys simplicity: lookups take `&K` instead of an arbitrary `&Q: Borrow<K>`, since all keys in
//! this crate are small `Copy` id types.

use std::{
    collections::hash_map::{self, Entry},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    ops::{Deref, Index, IndexMut},
};

use rustc_hash::FxHashMap;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{SeqAccess, Visitor},
    ser::SerializeSeq,
};

#[derive(Debug, Clone)]
pub struct HashMap<K, V>(FxHashMap<K, V>);

impl<K, V> HashMap<K, V> {
    /// Creates an empty map.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty map that can hold at least `capacity` entries without reallocating.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(FxHashMap::with_capacity_and_hasher(
            capacity,
            Default::default(),
        ))
    }

    /// Returns the number of entries in the map.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the map contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Removes all entries, keeping the allocated capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Returns an iterator over all key-value pairs in arbitrary order.
    ///
    /// # Important
    /// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
    /// one should not rely on the ordering for determinism.
    #[inline]
    pub fn iter(&self) -> hash_map::Iter<'_, K, V> {
        self.0.iter()
    }

    /// Returns an iterator over all keys in arbitrary order.
    ///
    /// # Important
    /// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
    /// one should not rely on the ordering for determinism.
    #[inline]
    pub fn keys(&self) -> hash_map::Keys<'_, K, V> {
        self.0.keys()
    }

    /// Returns an iterator over all values in arbitrary order.
    ///
    /// # Important
    /// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
    /// one should not rely on the ordering for determinism.
    #[inline]
    pub fn values(&self) -> hash_map::Values<'_, K, V> {
        self.0.values()
    }

    /// Returns an iterator that allows modifying every value in arbitrary order.
    ///
    /// # Important
    /// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
    /// one should not rely on the ordering for determinism.
    #[inline]
    pub fn values_mut(&mut self) -> hash_map::ValuesMut<'_, K, V> {
        self.0.values_mut()
    }
}

impl<K: Eq + Hash, V> HashMap<K, V> {
    /// Returns a reference to the value stored for `key`, or [`None`] if there is none.
    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }

    /// Returns a mutable reference to the value stored for `key`, or [`None`] if there is none.
    #[inline]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.0.get_mut(key)
    }

    /// Returns `true` if the map contains a value for `key`.
    #[inline]
    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    /// Inserts `value` at `key` and returns the value that was stored there before, if any.
    #[inline]
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }

    /// Removes the entry for `key` and returns its value, or [`None`] if there was none.
    #[inline]
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.0.remove(key)
    }

    /// Returns the entry for `key` for in-place manipulation.
    #[inline]
    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.0.entry(key)
    }
}

impl<K, V> Default for HashMap<K, V> {
    fn default() -> Self {
        Self(Default::default())
    }
}

// Implemented manually because deriving would bound `K: PartialEq` instead of `K: Eq + Hash`.
impl<K: Eq + Hash, V: PartialEq> PartialEq for HashMap<K, V> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Eq + Hash, V: Eq> Eq for HashMap<K, V> {}

/// Serializes as a plain map, exactly like [`std::collections::HashMap`] does; the wrapper adds
/// nothing to the serialized form.
///
/// # Important
/// The order in which the entries are serialized is the arbitrary iteration order of the map, so
/// the serialized form of two equal maps may differ. Do not rely on it for determinism.
impl<K: Serialize, V: Serialize> Serialize for HashMap<K, V> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Deserializes from a plain map, exactly like [`std::collections::HashMap`] does.
impl<'de, K: Deserialize<'de> + Eq + Hash, V: Deserialize<'de>> Deserialize<'de> for HashMap<K, V> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        FxHashMap::deserialize(deserializer).map(Self)
    }
}

impl<K: Eq + Hash, V> Index<&K> for HashMap<K, V> {
    type Output = V;

    /// # Panics
    /// Panics if the map contains no entry for `key`.
    #[inline]
    fn index(&self, key: &K) -> &V {
        self.get(key).expect("no entry found for key")
    }
}

impl<K: Eq + Hash, V> IndexMut<&K> for HashMap<K, V> {
    /// # Panics
    /// Panics if the map contains no entry for `key`.
    #[inline]
    fn index_mut(&mut self, key: &K) -> &mut V {
        self.get_mut(key).expect("no entry found for key")
    }
}

/// # Important
/// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
/// one should not rely on the ordering for determinism.
impl<K, V> IntoIterator for HashMap<K, V> {
    type Item = (K, V);
    type IntoIter = hash_map::IntoIter<K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// # Important
/// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
/// one should not rely on the ordering for determinism.
impl<'a, K, V> IntoIterator for &'a HashMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = hash_map::Iter<'a, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K: Eq + Hash, V> Extend<(K, V)> for HashMap<K, V> {
    #[inline]
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<K: Eq + Hash, V> FromIterator<(K, V)> for HashMap<K, V> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// A value that carries its own id, so that an [`IdMap`] can key it without being told the key.
pub trait IdAble {
    type Id;
    fn id(&self) -> Self::Id;
}

/// A [`HashMap`] of values that know their own id, keyed by that id.
///
/// Every entry is stored under `value.id()`, and the whole API upholds that: values go in through
/// [`insert`](Self::insert) or [`extend`](Extend::extend), which derive the key from the value, and
/// [`Deref`] only ever hands out the read-only part of [`HashMap`]. Because the key is therefore
/// never anything but a copy of data the value already holds, it is left out of the serialized form
/// entirely: an [`IdMap`] serializes as a plain sequence of its values and rebuilds its keys when
/// deserializing.
///
/// # Important
/// The order in which the values are serialized is the arbitrary iteration order of the underlying
/// map, so the serialized form of two equal maps may differ. Do not rely on it for determinism.
#[derive(Debug)]
pub struct IdMap<V: IdAble>(HashMap<V::Id, V>);

impl<V: IdAble> Default for IdMap<V> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<V: IdAble> IdMap<V> {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Gives read-only access to the whole [`HashMap`] API; every mutating operation is reimplemented
/// on [`IdMap`] itself, so that no caller can pick the key an entry is stored under. See the type
/// level documentation for why that matters.
impl<V: IdAble> Deref for IdMap<V> {
    type Target = HashMap<V::Id, V>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<V: IdAble> IdMap<V>
where
    V::Id: Eq + Hash,
{
    /// Inserts `value` under its own id, replacing the value that was stored there before.
    #[inline]
    pub fn insert(&mut self, value: V) {
        self.0.insert(value.id(), value);
    }

    /// Returns a mutable reference to the value stored for `id`, or [`None`] if there is none.
    #[inline]
    pub fn get_mut(&mut self, id: &V::Id) -> Option<&mut V> {
        self.0.get_mut(id)
    }

    /// Returns an iterator that allows modifying every value in arbitrary order.
    ///
    /// # Important
    /// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
    /// one should not rely on the ordering for determinism.
    #[inline]
    pub fn values_mut(&mut self) -> hash_map::ValuesMut<'_, V::Id, V> {
        self.0.values_mut()
    }

    /// Removes the entry for `id` and returns its value, or [`None`] if there was none.
    #[inline]
    pub fn remove(&mut self, id: &V::Id) -> Option<V> {
        self.0.remove(id)
    }
}

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
        let mut map = IdMap(HashMap::with_capacity(seq.size_hint().unwrap_or(0)));

        while let Some(value) = seq.next_element::<V>()? {
            map.0.insert(value.id(), value);
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

impl<V: IdAble> Index<&V::Id> for IdMap<V>
where
    V::Id: Eq + Hash,
{
    type Output = V;

    /// # Panics
    /// Panics if the map contains no entry for `key`.
    #[inline]
    fn index(&self, key: &V::Id) -> &V {
        &self.0[key]
    }
}

impl<V: IdAble> IndexMut<&V::Id> for IdMap<V>
where
    V::Id: Eq + Hash,
{
    /// # Panics
    /// Panics if the map contains no entry for `key`.
    #[inline]
    fn index_mut(&mut self, key: &V::Id) -> &mut V {
        &mut self.0[key]
    }
}

/// # Important
/// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
/// one should not rely on the ordering for determinism.
impl<V: IdAble> IntoIterator for IdMap<V> {
    type Item = V;
    type IntoIter = hash_map::IntoValues<V::Id, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.0.into_values()
    }
}

/// # Important
/// There is no guarantee that the ordering is stable accross runs or architectures. Therefore,
/// one should not rely on the ordering for determinism.
impl<'a, V: IdAble> IntoIterator for &'a IdMap<V> {
    type Item = &'a V;
    type IntoIter = hash_map::Values<'a, V::Id, V>;

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
