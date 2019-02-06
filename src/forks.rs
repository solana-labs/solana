use crate::bank_delta::BankDelta;
/// This module tracks the forks in the bank
use crate::bank_fork::BankFork;
use std::sync::Arc;
//TODO: own module error
use crate::bank::{BankError, Result};
use crate::checkpoints::Checkpoints;
use solana_sdk::hash::Hash;
use std;

pub const ROLLBACK_DEPTH: usize = 32usize;

#[derive(Default)]
pub struct Forks {
    pub deltas: Checkpoints<Arc<BankDelta>>,

    /// Last fork to be initialized
    /// This should be the last fork to be replayed or the TPU fork
    pub active_fork: u64,

    /// Fork that is root
    pub root: u64,
}

impl Forks {
    pub fn active_fork(&self) -> BankFork {
        self.fork(self.active_fork).expect("live fork")
    }
    pub fn root(&self) -> BankFork {
        self.fork(self.root).expect("root fork")
    }

    pub fn fork(&self, fork: u64) -> Option<BankFork> {
        let cp: Vec<_> = self
            .deltas
            .collect(ROLLBACK_DEPTH + 1, fork)
            .into_iter()
            .map(|x| x.1)
            .cloned()
            .collect();
        if cp.is_empty() {
            None
        } else {
            Some(BankFork { deltas: cp })
        }
    }
    /// Collapse the bottom two deltas.
    /// The tree is computed from the `leaf` to the `root`
    /// The path from `leaf` to the `root` is the active chain.
    /// The leaf is the last possible fork, it should have no descendants.
    /// The direct child of the root that leads the leaf becomes the new root.
    /// The forks that are not a descendant of the new root -> leaf path are pruned.
    /// active_fork is the leaf.
    /// root is the new root.
    /// Return the new root id.
    pub fn merge_into_root(&mut self, max_depth: usize, leaf: u64) -> Result<Option<u64>> {
        // `old` root, should have `root` as its fork_id
        // `new` root is a direct descendant of old and has new_root_id as its fork_id
        // new is merged into old
        // and old is swapped into the delta under new_root_id
        let merge_root = {
            let active_chain = self.deltas.collect(ROLLBACK_DEPTH + 1, leaf);
            let leaf_id = active_chain
                .first()
                .map(|x| x.0)
                .ok_or(BankError::UnknownFork)?;
            assert_eq!(leaf_id, leaf);
            let len = active_chain.len();
            if len > max_depth {
                let old_root = active_chain[len - 1];
                let new_root = active_chain[len - 2];
                if !new_root.1.frozen() {
                    trace!("new_root id {}", new_root.1.fork_id());
                    return Err(BankError::DeltaNotFrozen);
                }
                if !old_root.1.frozen() {
                    trace!("old id {}", old_root.1.fork_id());
                    return Err(BankError::DeltaNotFrozen);
                }
                //stupid sanity checks
                assert_eq!(new_root.1.fork_id(), new_root.0);
                assert_eq!(old_root.1.fork_id(), old_root.0);
                Some((old_root.1.clone(), new_root.1.clone(), new_root.0))
            } else {
                None
            }
        };
        if let Some((old_root, new_root, new_root_id)) = merge_root {
            let idag = self.deltas.invert();
            let new_deltas = self.deltas.prune(new_root_id, &idag);
            let old_root_id = old_root.fork_id();
            self.deltas = new_deltas;
            self.root = new_root_id;
            self.active_fork = leaf;
            // old should have been pruned
            assert!(self.deltas.load(old_root_id).is_none());
            // new_root id should be in the new tree
            assert!(!self.deltas.load(new_root_id).is_none());

            // swap in the old instance under the new_root id
            // this should be the last external ref to `new_root`
            self.deltas
                .insert(new_root_id, old_root.clone(), old_root_id);

            // merge all the new changes into the old instance under the new id
            // this should consume `new`
            // new should have no other references
            let new_root: BankDelta = Arc::try_unwrap(new_root).unwrap();
            old_root.merge_into_root(new_root);
            assert_eq!(old_root.fork_id(), new_root_id);
            Ok(Some(new_root_id))
        } else {
            Ok(None)
        }
    }

    /// Initialize the first root
    pub fn init_root(&mut self, delta: BankDelta) {
        assert!(self.deltas.is_empty());
        self.active_fork = delta.fork_id();
        self.root = delta.fork_id();
        //TODO: using u64::MAX as the impossible delta
        //this should be a None instead
        self.deltas
            .store(self.active_fork, Arc::new(delta), std::u64::MAX);
    }

    pub fn is_active_fork(&self, fork: u64) -> bool {
        if let Some(state) = self.deltas.load(fork) {
            !state.0.frozen() && self.active_fork == fork
        } else {
            false
        }
    }
    /// Initialize the `current` fork that is a direct descendant of the `base` fork.
    pub fn init_fork(&mut self, current: u64, last_id: &Hash, base: u64) -> Result<()> {
        if let Some(state) = self.deltas.load(base) {
            if !state.0.frozen() {
                return Err(BankError::DeltaNotFrozen);
            }
            let new = state.0.fork(current, last_id);
            self.deltas.store(current, Arc::new(new), base);
            self.active_fork = current;
            Ok(())
        } else {
            return Err(BankError::UnknownFork);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::hash;

    #[test]
    fn forks_init_root() {
        let mut forks = Forks::default();
        let cp = BankDelta::new(0, &Hash::default());
        forks.init_root(cp);
        assert!(forks.is_active_fork(0));
        assert_eq!(forks.root().deltas.len(), 1);
        assert_eq!(forks.root().head().fork_id(), 0);
        assert_eq!(forks.active_fork().head().fork_id(), 0);
    }

    #[test]
    fn forks_init_fork() {
        let mut forks = Forks::default();
        let last_id = Hash::default();
        let cp = BankDelta::new(0, &last_id);
        cp.register_tick(&last_id);
        forks.init_root(cp);
        let last_id = hash(last_id.as_ref());
        assert_eq!(forks.init_fork(1, &last_id, 1), Err(BankError::UnknownFork));
        assert_eq!(
            forks.init_fork(1, &last_id, 0),
            Err(BankError::DeltaNotFrozen)
        );
        forks.root().head().freeze();
        assert_eq!(forks.init_fork(1, &last_id, 0), Ok(()));

        assert_eq!(forks.root().head().fork_id(), 0);
        assert_eq!(forks.active_fork().head().fork_id(), 1);
        assert_eq!(forks.active_fork().deltas.len(), 2);
    }

    #[test]
    fn forks_merge() {
        let mut forks = Forks::default();
        let last_id = Hash::default();
        let cp = BankDelta::new(0, &last_id);
        cp.register_tick(&last_id);
        forks.init_root(cp);
        let last_id = hash(last_id.as_ref());
        forks.root().head().freeze();
        assert_eq!(forks.init_fork(1, &last_id, 0), Ok(()));
        forks.active_fork().head().register_tick(&last_id);
        forks.active_fork().head().freeze();
        assert_eq!(forks.merge_into_root(2, 1), Ok(None));
        assert_eq!(forks.merge_into_root(1, 1), Ok(Some(1)));

        assert_eq!(forks.active_fork().deltas.len(), 1);
        assert_eq!(forks.root().head().fork_id(), 1);
        assert_eq!(forks.active_fork().head().fork_id(), 1);
    }
    #[test]
    fn forks_merge_prune() {
        let mut forks = Forks::default();
        let last_id = Hash::default();
        let cp = BankDelta::new(0, &last_id);
        cp.register_tick(&last_id);
        forks.init_root(cp);
        let last_id = hash(last_id.as_ref());
        forks.root().head().freeze();
        assert_eq!(forks.init_fork(1, &last_id, 0), Ok(()));
        assert_eq!(forks.fork(1).unwrap().deltas.len(), 2);
        forks.fork(1).unwrap().head().register_tick(&last_id);

        // add a fork 2 to be pruned
        // fork 2 connects to 0
        let last_id = hash(last_id.as_ref());
        assert_eq!(forks.init_fork(2, &last_id, 0), Ok(()));
        assert_eq!(forks.fork(2).unwrap().deltas.len(), 2);
        forks.fork(2).unwrap().head().register_tick(&last_id);

        forks.fork(1).unwrap().head().freeze();
        // fork 1 is the new root, only forks that are descendant from 1 are valid
        assert_eq!(forks.merge_into_root(1, 1), Ok(Some(1)));

        // fork 2 is gone since it does not connect to 1
        assert!(forks.fork(2).is_none());

        assert_eq!(forks.active_fork().deltas.len(), 1);
        assert_eq!(forks.root().head().fork_id(), 1);
        assert_eq!(forks.active_fork().head().fork_id(), 1);
    }
}
