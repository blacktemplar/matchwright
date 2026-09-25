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
    ops::{Index, IndexMut},
};

use rustc_hash::FxHashMap;

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
