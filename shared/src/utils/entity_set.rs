use serde::{Deserialize, Serialize};
use std::{hash::Hash, marker::PhantomData, str::FromStr};
use uuid::Uuid;

use super::custom_map::{CustomMap, CustomSet};

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct EntitySet<T: Hash> {
    entities: CustomMap<EntityRef<T>, T>,
}

impl<T: Hash> Default for EntitySet<T> {
    fn default() -> Self {
        Self {
            entities: CustomMap::new(),
        }
    }
}

impl<T: Hash> EntitySet<T> {
    pub fn insert(&mut self, entity: T) -> EntityRef<T>
    where
        EntityRef<T>: Copy,
    {
        let entity_ref = EntityRef::new();
        self.entities.insert(entity_ref, entity);
        entity_ref
    }

    pub fn get(&self, entity_ref: &EntityRef<T>) -> Option<&T> {
        self.entities.get(entity_ref)
    }

    pub fn get_mut(&mut self, entity_ref: &EntityRef<T>) -> Option<&mut T> {
        self.entities.get_mut(entity_ref)
    }

    pub fn remove(&mut self, entity_ref: &EntityRef<T>) -> Option<T> {
        self.entities.swap_remove(entity_ref)
    }

    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
        EntityRef<T>: Copy,
    {
        let mut to_remove = CustomSet::new();
        for (&entity_ref, entity) in self.entities.as_mut_slice() {
            if f(entity) {
                to_remove.insert(entity_ref);
            }
        }

        self.entities
            .retain(|entity_ref, _| !to_remove.contains(entity_ref))
    }

    pub fn for_each_in_mut<F>(&mut self, set: &mut EntityRefSet<T>, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
        EntityRef<T>: Copy,
    {
        let mut to_remove = CustomSet::new();
        for &entity_ref in set.entities.as_slice() {
            if let Some(entity) = self.entities.get_mut(&entity_ref) {
                if f(entity) {
                    to_remove.insert(entity_ref);
                }
            } else {
                to_remove.insert(entity_ref);
            }
        }

        self.entities
            .retain(|entity_ref, _| !to_remove.contains(entity_ref));
        set.entities
            .retain(|entity_ref| !to_remove.contains(entity_ref));
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (&EntityRef<T>, &T)> + 'a
    where
        EntityRef<T>: Copy,
    {
        self.entities.as_slice().iter()
    }

    pub fn iter_in<'a, 'b: 'a>(
        &'a self,
        set: &'b EntityRefSet<T>,
    ) -> impl Iterator<Item = (&EntityRef<T>, &T)> + 'a
    where
        EntityRef<T>: Copy,
    {
        set.entities.as_slice().iter().flat_map(move |entity_ref| {
            self.entities
                .get(entity_ref)
                .map(|entity| (entity_ref, entity))
        })
    }
}

impl<'a, T: Hash> IntoIterator for &'a EntitySet<T> {
    type Item = (&'a EntityRef<T>, &'a T);

    type IntoIter = indexmap::map::Iter<'a, EntityRef<T>, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.entities.iter()
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct EntityRefSet<T: Hash> {
    entities: CustomSet<EntityRef<T>>,
}

impl<T: Hash> Default for EntityRefSet<T> {
    fn default() -> Self {
        Self {
            entities: CustomSet::new(),
        }
    }
}

impl<T: Hash> EntityRefSet<T> {
    pub fn insert(&mut self, entity_ref: EntityRef<T>) {
        self.entities.insert(entity_ref);
    }

    pub fn remove(&mut self, entity_ref: &EntityRef<T>) -> bool {
        self.entities.swap_remove(entity_ref)
    }
}

#[derive(Debug, Hash, Serialize, Deserialize)]
pub struct EntityRef<T>(Uuid, PhantomData<T>);

impl<T> PartialEq for EntityRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for EntityRef<T> {}

impl<T> PartialOrd for EntityRef<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T> Ord for EntityRef<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T> EntityRef<T> {
    fn new() -> Self {
        EntityRef(Uuid::new_v4(), PhantomData::default())
    }
}

impl<T> Clone for EntityRef<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl<T> Copy for EntityRef<T> {}

impl<T> FromStr for EntityRef<T> {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(EntityRef(
            Uuid::from_str(s).map_err(|_| ())?,
            PhantomData::default(),
        ))
    }
}

impl<T> ToString for EntityRef<T> {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}
