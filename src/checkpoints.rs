//! Simple data structure to keep track of checkpointed state.  It stores a map
//! of forks to a type and parent forks.
//!
//! `latest` forks is a set of all the forks with no children.
//!
//! A trunk is the latest fork that is a parent all the `latest` forks.  If
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
            panic!("fork: {}, parent: {} error: out of order or trivial cycle");
        }
        self.latest.remove(&parent);
        self.latest.insert(fork);
        self.checkpoints.insert(fork, (data, parent));
    }
    /// Given a base fork, and a maximum number, collect all the
    /// forks starting from the base fork backwards
    pub fn collect(&self, num: usize, mut fork: u64) -> Vec<(u64, &T)> {
        let mut rv = vec![];
        loop {
            if let Some((val, parent)) = self.load(fork) {
                rv.push((fork, val));
                fork = *parent;
            } else {
                break;
            }
            if rv.len() == num {
                break;
            }
        }
        rv
    }
    /// given a maximum depth, find the highest parent shared by all
    ///  forks
    pub fn trunk(&self, num: usize) -> Option<u64> {
        let mut chains: Vec<_> = self
            .latest
            .iter()
            .map(|tip| self.collect(num, *tip))
            .collect();

        // no chains, no trunk
        if chains.is_empty() {
            return None;
        }
        // one chains, no trunk
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
                    Ok(idx) => {
                        chain.truncate(idx);
                    }
                    _ => {
                        break 'outer; // not found
                    }
                }
            }
            trunk = Some(trytrunk);

            // now all chains end with trytrunk
            // remove trytrunk item and try the minimum next highest
            for chain in chains.iter_mut() {
                if chain.len() < 2 {
                    // outer loop needs all chains to have at least one item
                    // (because of last().unwrap() above)
                    break 'outer;
                }
                chain.pop();
            }
        }
        trunk
    }
}

impl<T> std::fmt::Debug for Checkpoints<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for tip in self.latest.iter() {
            for (fork, _) in self.collect(std::usize::MAX, *tip) {
                write!(f, "{} ", fork)?;
            }
            write!(f, "\n")?;
        }
        Ok(())
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
    fn test_checkpoints() {
        let mut checkpoints: Checkpoints<usize> = Default::default();

        assert_eq!(checkpoints.trunk(std::usize::MAX), None);

        // store 10 versions of data (i is data and fork key)
        for i in 1..11 {
            checkpoints.store(i as u64, i, (i - 1) as u64);
        }

        let points = checkpoints.collect(1000, 11);
        assert_eq!(points.len(), 0);

        let points = checkpoints.collect(1000, 10);
        assert_eq!(points.len(), 10);

        for (i, (parent, data)) in points.iter().enumerate() {
            assert_eq!(**data, 10 - i);
            assert_eq!(*parent, 10 - i as u64);
        }
        // only one chain, trunk is latest
        assert_eq!(checkpoints.trunk(11), Some(10));

        for i in 11..15 {
            checkpoints.store(i as u64, i, 1 as u64);
        }

        assert_eq!(checkpoints.trunk(1), None);
        assert_eq!(checkpoints.trunk(10), None);
        assert_eq!(checkpoints.trunk(11), Some(1));
    }
}
