//! Simple data structure to keep track of checkpointed state.  It stores a map
//! of forks to a type and parent forks.
//!
//! `latest` forks is a set of all the forks with no children.
//!
//! A trunk is the most recent fork that is a parent to all the `latest` forks.  If
//! consensus works correctly, then latest should be pruned such that only one
//! trunk exists within N links.

use hashbrown::{HashMap, HashSet};

pub struct Checkpoints<T> {
    /// Stores a map from fork to a T and a parent fork
    pub checkpoints: HashMap<u64, (T, u64)>,

    /// The latest forks that have been added
    pub latest: HashSet<u64>,
}

impl<T> Checkpoints<T> {
    pub fn load(&self, fork: u64) -> Option<&(T, u64)> {
        self.checkpoints.get(&fork)
    }
    pub fn store(&mut self, fork: u64, data: T, parent: u64) {
        if fork <= parent {
            panic!(
                "fork: {}, parent: {} error: out of order or trivial cycle",
                fork, parent
            );
        }

        if self.checkpoints.get(&fork).is_some() {
            panic!(
                "fork: {}, parent: {} error: fork is already checkpointed",
                fork, parent
            );
        }

        self.latest.remove(&parent);
        self.latest.insert(fork);
        self.checkpoints.insert(fork, (data, parent));
    }
    /// Given a base fork, and a maximum number, collect all the
    /// forks starting from the base fork backwards
    pub fn collect(&self, num: usize, mut fork: u64) -> Vec<(u64, &T)> {
        let mut rv = vec![];
        while rv.len() < num {
            if let Some((val, parent)) = self.load(fork) {
                rv.push((fork, val));
                fork = *parent;
            } else {
                break;
            }
        }
        rv
    }

    /// returns the trunks, and an inverse dag
    pub fn invert(&self) -> (HashSet<u64>, HashMap<u64, HashSet<u64>>) {
        let mut trunks = HashSet::new();
        let mut idag = HashMap::new();
        for (k, (_, v)) in &self.checkpoints {
            trunks.remove(k);
            trunks.insert(*v);
            idag.entry(*v).or_insert(HashSet::new()).insert(*k);
        }
        (trunks, idag)
    }

    /// given a maximum depth, find the highest parent shared by all
    ///  forks
    pub fn trunk(&self, num: usize) -> Option<u64> {
        // no chains of length 0
        if num == 0 {
            return None;
        }

        let mut chains: Vec<_> = self
            .latest
            .iter()
            .map(|tip| self.collect(num, *tip))
            .collect();

        // no chains, no trunk
        if chains.is_empty() {
            return None;
        }
        // one chain, trunk is tip
        if chains.len() == 1 {
            return chains[0].first().map(|fork| fork.0);
        }

        let mut trunk = None;

        'outer: loop {
            // find the lowest possible trunk, is always last
            let trytrunk = chains.iter().fold(
                chains[0]
                    .last()
                    .expect("chain built from any latest can't be empty")
                    .0,
                |min, chain| std::cmp::min(min, chain.last().unwrap().0),
            );

            // verify it's in every chain
            for chain in chains.iter_mut() {
                // chain is in reverse order, so search comparison is backwards
                match chain.binary_search_by(|probe| trytrunk.cmp(&probe.0)) {
                    Ok(idx) => chain.truncate(idx),
                    _ => break 'outer, // not found
                }
            }
            trunk = Some(trytrunk);

            // all chains are now truncated before the trytrunk entry,
            // none should be empty, implies a structure like
            //   3->1
            //   1
            // which we prevent in store()
            for chain in chains.iter() {
                if chain.is_empty() {
                    panic!("parent forks should never appear in latest");
                }
            }
        }
        trunk
    }
}

impl<T> std::fmt::Debug for Checkpoints<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{{ ")?;
        for tip in self.latest.iter() {
            write!(f, "[ ")?;
            for (fork, _) in self.collect(std::usize::MAX, *tip) {
                write!(f, "{}, ", fork)?;
            }
            write!(f, "], ")?;
        }
        write!(f, "}}")
    }
}

impl<T> Default for Checkpoints<T> {
    fn default() -> Self {
        Self {
            checkpoints: HashMap::new(),
            latest: HashSet::new(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[should_panic]
    fn test_checkpoints_trivial_cycle() {
        let mut checkpoints: Checkpoints<u64> = Default::default();
        checkpoints.store(0, 0, 0);
    }
    #[test]
    #[should_panic]
    fn test_checkpoints_out_of_order() {
        let mut checkpoints: Checkpoints<u64> = Default::default();
        checkpoints.store(0, 0, 1);
    }

    #[test]
    fn test_checkpoints() {
        let mut checkpoints: Checkpoints<usize> = Default::default();

        assert_eq!(checkpoints.trunk(std::usize::MAX), None);

        // store 10 versions of data (i is data and fork key)
        for i in 1..11 {
            checkpoints.store(i as u64, i, (i - 1) as u64);
        }
        // now checkpoints:
        // 10->9->8->7->6->5->4->3->2->1

        let points = checkpoints.collect(std::usize::MAX, 10);
        assert_eq!(points.len(), 10);
        for (i, (parent, data)) in points.iter().enumerate() {
            assert_eq!(**data, 10 - i);
            assert_eq!(*parent, 10 - i as u64);
        }

        let points = checkpoints.collect(0, 10);
        assert_eq!(points.len(), 0);

        let points = checkpoints.collect(1, 10);
        assert_eq!(points.len(), 1);

        let points = checkpoints.collect(std::usize::MAX, 11);
        assert_eq!(points.len(), 0);

        // only one chain, so tip of trunk is latest
        assert_eq!(checkpoints.trunk(11), Some(10));

        // only one chain, tip trunk is latest
        assert_eq!(checkpoints.trunk(1), Some(10));

        // zero length ask
        assert_eq!(checkpoints.trunk(0), None);

        for i in 11..15 {
            checkpoints.store(i as u64, i, 1 as u64);
        }
        // now checkpoints:
        // 10->9->8->7->6->5->4->3->2->1
        // 11->1
        // 12->1
        // 13->1
        // 14->1

        assert_eq!(checkpoints.trunk(1), None);
        assert_eq!(checkpoints.trunk(0), None);
        assert_eq!(checkpoints.trunk(9), None);
        assert_eq!(checkpoints.trunk(10), Some(1));
        assert_eq!(checkpoints.trunk(11), Some(1));

        // test higher trunk
        let mut checkpoints: Checkpoints<usize> = Default::default();

        // 3->2->1
        // 4->2->1
        checkpoints.store(1, 0, 0);
        checkpoints.store(2, 0, 1);
        checkpoints.store(3, 0, 2);
        checkpoints.store(4, 0, 2);
        assert_eq!(
            checkpoints.collect(std::usize::MAX, 3),
            vec![(3u64, &0), (2u64, &0), (1u64, &0)]
        );
        assert_eq!(
            checkpoints.collect(std::usize::MAX, 4),
            vec![(4u64, &0), (2u64, &0), (1u64, &0)]
        );
        assert_eq!(checkpoints.trunk(std::usize::MAX), Some(2));
    }

    #[test]
    fn test_checkpoints_invert() {
        // test higher trunk
        let mut checkpoints: Checkpoints<usize> = Default::default();

        // 3->2->1
        // 4->2->1
        checkpoints.store(1, 0, 0);
        checkpoints.store(2, 0, 1);
        checkpoints.store(3, 0, 2);
        checkpoints.store(4, 0, 2);

        let _trunks: Vec<_> = checkpoints.invert().0.into_iter().collect();
        // TODO: returns [0, 2] ??

        // 30->20->10
        // 40->20->10
        checkpoints.store(10, 0, 0);
        checkpoints.store(20, 0, 10);
        checkpoints.store(30, 0, 20);
        checkpoints.store(40, 0, 20);

        let _trunks: Vec<_> = checkpoints.invert().0.into_iter().collect();
        // TODO: returns [0, 2, 10, 20, 1] ??
    }

    #[test]
    #[should_panic]
    fn test_checkpoints_latest_is_a_parent() {
        // trunk is actually live...
        let mut checkpoints: Checkpoints<usize> = Default::default();

        // 2->1
        // 1
        checkpoints.store(1, 0, 0);
        checkpoints.store(3, 0, 1);
        checkpoints.store(1, 0, 0); // <== panic...
    }
    #[test]
    #[should_panic]
    fn test_checkpoints_latest_is_a_parent_corrupt() {
        // trunk is actually live...
        let mut checkpoints: Checkpoints<usize> = Default::default();

        // 2->1
        // 1
        checkpoints.store(1, 0, 0);
        checkpoints.store(3, 0, 1);

        // same as:
        // checkpoints.store(1, 0, 0); // <== would corrupt the checkpoints...
        checkpoints.latest.remove(&0);
        checkpoints.latest.insert(1);
        checkpoints.checkpoints.insert(1, (0, 0));

        checkpoints.trunk(std::usize::MAX); // panic
    }

}
