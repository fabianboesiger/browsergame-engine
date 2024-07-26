use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

use fxhash::{FxBuildHasher, FxHasher};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomMap<K: Eq + Hash, V>(IndexMap<K, V, FxBuildHasher>);

impl<K: Eq + Hash, V> Default for CustomMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash, V> CustomMap<K, V> {
    pub fn new() -> Self {
        Self(IndexMap::with_hasher(FxBuildHasher::default()))
    }
}

impl<K: Eq + Hash, V> Deref for CustomMap<K, V> {
    type Target = IndexMap<K, V, FxBuildHasher>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K: Eq + Hash, V> DerefMut for CustomMap<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K: Eq + Hash, V: Hash> Hash for CustomMap<K, V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash = self
            .iter()
            .map(|(k, v)| {
                let mut hasher = FxHasher::default();
                k.hash(&mut hasher);
                v.hash(&mut hasher);
                hasher.finish()
            })
            .fold(0, u64::wrapping_add);

        state.write_u64(hash);
    }
}

impl<K: Eq + Hash, V> IntoIterator for CustomMap<K, V> {
    type Item = (K, V);

    type IntoIter = indexmap::map::IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, K: Eq + Hash + 'a, V: 'a> IntoIterator for &'a CustomMap<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = indexmap::map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<K: Eq + Hash, V: Hash> FromIterator<(K, V)> for CustomMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut map = CustomMap::new();
        for (k, v) in iter.into_iter() {
            map.insert(k, v);
        }
        map
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CustomSet<T: Eq + Hash>(IndexSet<T, FxBuildHasher>);

impl<T: Eq + Hash> CustomSet<T> {
    pub fn new() -> Self {
        Self(IndexSet::with_hasher(FxBuildHasher::default()))
    }
}

impl<T: Eq + Hash> Deref for CustomSet<T> {
    type Target = IndexSet<T, FxBuildHasher>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Eq + Hash> DerefMut for CustomSet<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Eq + Hash> Hash for CustomSet<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash = self
            .iter()
            .map(|v| {
                let mut hasher = FxHasher::default();
                v.hash(&mut hasher);
                hasher.finish()
            })
            .fold(0, u64::wrapping_add);

        state.write_u64(hash);
    }
}

impl<'a, T: Eq + Hash + 'a> IntoIterator for &'a CustomSet<T> {
    type Item = &'a T;

    type IntoIter = indexmap::set::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: Eq + Hash> IntoIterator for CustomSet<T> {
    type Item = T;

    type IntoIter = indexmap::set::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: Eq + Hash> FromIterator<T> for CustomSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut set = CustomSet::new();
        for t in iter.into_iter() {
            set.insert(t);
        }
        set
    }
}
