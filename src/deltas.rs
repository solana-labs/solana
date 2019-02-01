//! Simple data structure to keep track of deltas (partial state updates).  It
//! stores a map of forks to a type and parent forks.
//!
//! A root is the fork that is a parent to all the leaf forks.

use hashbrown::{HashMap, HashSet};
use std::collections::VecDeque;

pub struct Deltas<T> {
    /// Stores a map from fork to a T and a parent fork
    pub deltas: HashMap<u64, (T, u64)>,
}

impl<T: Clone> Deltas<T> {
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }
    pub fn load(&self, fork: u64) -> Option<&(T, u64)> {
        self.deltas.get(&fork)
    }
    pub fn store(&mut self, fork: u64, data: T, trunk: u64) {
        self.insert(fork, data, trunk);
    }
    pub fn insert(&mut self, fork: u64, data: T, trunk: u64) {
        self.deltas.insert(fork, (data, trunk));
    }
    /// Given a base fork, and a maximum number, collect all the
    /// forks starting from the base fork backwards
    pub fn collect(&self, num: usize, mut base: u64) -> Vec<(u64, &T)> {
        let mut rv = vec![];
        loop {
            if rv.len() == num {
                break;
            }
            if let Some((val, next)) = self.load(base) {
                rv.push((base, val));
                base = *next;
            } else {
                break;
            }
        }
        rv
    }

    ///invert the dag
    pub fn invert(&self) -> HashMap<u64, HashSet<u64>> {
        let mut idag = HashMap::new();
        for (k, (_, v)) in &self.deltas {
            idag.entry(*v).or_insert(HashSet::new()).insert(*k);
        }
        idag
    }

    ///create a new Deltas tree that only derives from the trunk
    pub fn prune(&self, trunk: u64, inverse: &HashMap<u64, HashSet<u64>>) -> Self {
        let mut new = Self::default();
        // simple BFS
        let mut queue = VecDeque::new();
        queue.push_back(trunk);
        loop {
            if queue.is_empty() {
                break;
            }
            let trunk = queue.pop_front().unwrap();
            let (data, prev) = self.load(trunk).expect("load from inverse").clone();
            new.store(trunk, data.clone(), prev);
            if let Some(children) = inverse.get(&trunk) {
                let mut next = children.into_iter().cloned().collect();
                queue.append(&mut next);
            }
        }
        new
    }
}

impl<T> Default for Deltas<T> {
    fn default() -> Self {
        Self {
            deltas: HashMap::new(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let cp: Deltas<bool> = Deltas::default();
        assert!(cp.is_empty());
    }

    #[test]
    fn test_load_store() {
        let mut cp: Deltas<bool> = Deltas::default();
        assert_eq!(cp.load(1), None);
        cp.store(1, true, 0);
        assert_eq!(cp.load(1), Some(&(true, 0)));
    }
    #[test]
    fn test_collect() {
        let mut cp: Deltas<bool> = Deltas::default();
        assert_eq!(cp.load(1), None);
        cp.store(1, true, 0);
        assert_eq!(cp.collect(0, 1), vec![]);
        assert_eq!(cp.collect(1, 1), vec![(1, &true)]);
    }
    #[test]
    fn test_invert() {
        let mut cp: Deltas<bool> = Deltas::default();
        assert_eq!(cp.load(1), None);
        cp.store(1, true, 0);
        cp.store(2, true, 0);
        let inverse = cp.invert();
        assert_eq!(inverse.len(), 1);
        assert_eq!(inverse[&0].len(), 2);
        let list: Vec<u64> = inverse[&0].iter().cloned().collect();
        assert_eq!(list, vec![1, 2]);
    }
    #[test]
    fn test_prune() {
        let mut cp: Deltas<bool> = Deltas::default();
        assert_eq!(cp.load(1), None);
        cp.store(1, true, 0);
        cp.store(2, true, 0);
        cp.store(3, true, 1);
        let inverse = cp.invert();
        let pruned = cp.prune(1, &inverse);
        assert_eq!(pruned.load(0), None);
        assert_eq!(pruned.load(1), Some(&(true, 0)));
        assert_eq!(pruned.load(2), None);
        assert_eq!(pruned.load(3), Some(&(true, 1)));
    }
}
