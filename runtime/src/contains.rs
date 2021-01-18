use std::{
    borrow::Borrow,
    cmp::Eq,
    collections::{HashMap, HashSet},
    hash::Hash,
};

pub trait Contains<'a, T: Eq + Hash> {
    type Item: Borrow<T>;
    type Iter: Iterator<Item = Self::Item>;
    fn contains(&self, key: &T) -> bool;
    fn contains_iter(&'a self) -> Self::Iter;
}

impl<'a, T: 'a + Eq + Hash, U: 'a> Contains<'a, T> for HashMap<T, U> {
    type Item = &'a T;
    type Iter = std::collections::hash_map::Keys<'a, T, U>;

    fn contains(&self, key: &T) -> bool {
        <HashMap<T, U>>::contains_key(self, key)
    }
    fn contains_iter(&'a self) -> Self::Iter {
        self.keys()
    }
}

impl<'a, T: 'a + Eq + Hash> Contains<'a, T> for HashSet<T> {
    type Item = &'a T;
    type Iter = std::collections::hash_set::Iter<'a, T>;

    fn contains(&self, key: &T) -> bool {
        <HashSet<T>>::contains(self, key)
    }
    fn contains_iter(&'a self) -> Self::Iter {
        self.iter()
    }
}

impl<'a, T: 'a + Eq + Hash + Copy> Contains<'a, T> for T {
    type Item = &'a T;
    type Iter = std::iter::Once<&'a T>;

    fn contains(&self, key: &T) -> bool {
        key == self
    }
    fn contains_iter(&'a self) -> Self::Iter {
        std::iter::once(self)
    }
}
