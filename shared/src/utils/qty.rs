use fxhash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::{
    hash::Hash,
    ops::{Add, AddAssign, Sub, SubAssign},
};

use super::custom_map::CustomMap;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Qty<T: Hash + Eq>(CustomMap<T, u64>);

impl<T: Hash + Eq> Default for Qty<T> {
    fn default() -> Self {
        Qty(CustomMap::new())
    }
}

impl<T: Hash + Eq + Copy> Qty<T> {
    pub fn with(mut self, resource: T, num: u64) -> Self {
        *self.0.entry(resource).or_default() += num;
        self
    }

    pub fn add(&mut self, resource: T, num: u64) {
        *self.0.entry(resource).or_default() += num;
    }

    pub fn get(&self, resource: &T) -> u64 {
        self.0.get(resource).copied().unwrap_or_default()
    }

    pub fn covers(&self, cost: &Self) -> bool {
        for resource in self
            .0
            .keys()
            .chain(cost.0.keys())
            .copied()
            .collect::<FxHashSet<T>>()
        {
            if self.0.get(&resource).copied().unwrap_or_default()
                < cost.0.get(&resource).copied().unwrap_or_default()
            {
                return false;
            }
        }
        true
    }
}

impl<T: Hash + Eq + Copy> Add for Qty<T> {
    type Output = Self;

    fn add(mut self, mut rhs: Self) -> Self::Output {
        for resource in self
            .0
            .keys()
            .chain(rhs.0.keys())
            .copied()
            .collect::<FxHashSet<T>>()
        {
            *self.0.entry(resource).or_default() += *rhs.0.entry(resource).or_default();
        }
        self
    }
}

impl<T: Hash + Eq + Copy> Sub for Qty<T> {
    type Output = Self;

    fn sub(mut self, mut rhs: Self) -> Self::Output {
        for resource in self
            .0
            .keys()
            .chain(rhs.0.keys())
            .copied()
            .collect::<FxHashSet<T>>()
        {
            *self.0.entry(resource).or_default() -= *rhs.0.entry(resource).or_default();
        }
        self
    }
}

impl<T: Hash + Eq + Copy> AddAssign for Qty<T> {
    fn add_assign(&mut self, mut rhs: Self) {
        for resource in self
            .0
            .keys()
            .chain(rhs.0.keys())
            .copied()
            .collect::<FxHashSet<T>>()
        {
            *self.0.entry(resource).or_default() += *rhs.0.entry(resource).or_default();
        }
    }
}

impl<T: Hash + Eq + Copy> SubAssign for Qty<T> {
    fn sub_assign(&mut self, mut rhs: Self) {
        for resource in self
            .0
            .keys()
            .chain(rhs.0.keys())
            .copied()
            .collect::<FxHashSet<T>>()
        {
            *self.0.entry(resource).or_default() -= *rhs.0.entry(resource).or_default();
        }
    }
}
