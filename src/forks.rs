/// This module tracks the forks in the bank
use crate::bank_state::{BankCheckpoint, BankState};
use std::sync::Arc;
//TODO: own module error
use crate::bank::{BankError, Result};
use crate::checkpoints::Checkpoints;
use solana_sdk::hash::Hash;
use std;

const ROLLBACK_DEPTH: usize = 32usize;

#[derive(Default)]
pub struct Forks {
    pub checkpoints: Checkpoints<Arc<BankCheckpoint>>,

    /// Last fork to be initialized
    /// This should be the last fork to be replayed or the TPU fork
    pub live_bank_state: u64,

    /// Fork that is root
    pub root_bank_state: u64,
}

impl Forks {
    pub fn live_bank_state(&self) -> BankState {
        self.bank_state(self.live_bank_state).expect("live fork")
    }
    pub fn root_bank_state(&self) -> BankState {
        self.bank_state(self.root_bank_state).expect("root fork")
    }

    pub fn bank_state(&self, fork: u64) -> Option<BankState> {
        let cp: Vec<_> = self
            .checkpoints
            .collect(ROLLBACK_DEPTH, fork)
            .into_iter()
            .map(|x| x.1)
            .cloned()
            .collect();
        if cp.is_empty() {
            None
        } else {
            Some(BankState { checkpoints: cp })
        }
    }
    /// Collapse the bottom two checkpoints.
    /// The tree is computed from the `leaf` to the `root`
    /// The leaf is the last possible fork, it should have no descendants.
    /// The expected oldest fork must be the root
    /// The direct child of the root that leads the leaf becomes the new root.
    /// The forks that are not a decendant of the new root -> leaf path are pruned.
    /// live_bank_state is the leaf.
    /// root_bank_state is the new root.
    /// Return the new root id.
    pub fn merge_into_root(&mut self, root: u64, leaf: u64) -> Result<u64> {
        // `old` root, should have `root` as its fork_id
        // `new` root is a direct decendant of old and has new_root_id as its fork_id
        // new is merged into old
        // and old is swapped into the checkpoint under new_root_id
        let (old_root, new_root, new_root_id) = {
            let states = self.checkpoints.collect(ROLLBACK_DEPTH + 1, leaf);
            let leaf_id = states.first().map(|x| x.0).ok_or(BankError::UnknownFork)?;
            assert_eq!(leaf_id, leaf);
            let root_id = states.last().map(|x| x.0).ok_or(BankError::UnknownFork)?;
            if root_id != root {
                return Err(BankError::InvalidTrunk);
            }
            let len = states.len();
            let old_root = states[len - 1].clone();
            let new_root = states[len - 2].clone();
            if !new_root.1.finalized() {
                println!("new_root id {}", new_root.1.fork_id());
                return Err(BankError::CheckpointNotFinalized);
            }
            if !old_root.1.finalized() {
                println!("old id {}", old_root.1.fork_id());
                return Err(BankError::CheckpointNotFinalized);
            }
            //stupid sanity checks
            assert_eq!(new_root.1.fork_id(), new_root.0);
            assert_eq!(old_root.1.fork_id(), old_root.0);
            (old_root.1.clone(), new_root.1.clone(), new_root.0)
        };
        let idag = self.checkpoints.invert();
        let new_checkpoints = self.checkpoints.prune(new_root_id, &idag);
        let old_root_id = old_root.fork_id();
        self.checkpoints = new_checkpoints;
        self.root_bank_state = new_root_id;
        self.live_bank_state = leaf;
        // old should have been pruned
        assert!(self.checkpoints.load(old_root_id).is_none());
        // new_root id should be in the new tree
        assert!(!self.checkpoints.load(new_root_id).is_none());

        // swap in the old instance under the new_root id
        // this should be the last external ref to `new_root`
        self.checkpoints
            .insert(new_root_id, old_root.clone(), old_root_id);

        // merge all the new changes into the old instance under the new id
        // this should consume `new`
        // new should have no other references
        let new_root: BankCheckpoint = Arc::try_unwrap(new_root).unwrap();
        old_root.merge_into_root(new_root);
        assert_eq!(old_root.fork_id(), new_root_id);
        Ok(new_root_id)
    }

    /// Initialize the first root
    pub fn init_root_bank_state(&mut self, checkpoint: BankCheckpoint) {
        assert!(self.checkpoints.is_empty());
        self.live_bank_state = checkpoint.fork_id();
        self.root_bank_state = checkpoint.fork_id();
        //TODO: using u64::MAX as the impossible checkpoint
        //this should be a None instead
        self.checkpoints
            .store(self.live_bank_state, Arc::new(checkpoint), std::u64::MAX);
    }

    pub fn is_active_fork(&self, fork: u64) -> bool {
        if let Some(state) = self.checkpoints.load(fork) {
            !state.0.finalized() && self.live_bank_state == fork
        } else {
            false
        }
    }
    /// Initalize the `current` fork that is a direct decendant of the `base` fork.
    pub fn init_fork(&mut self, current: u64, last_id: &Hash, base: u64) -> Result<()> {
        if let Some(state) = self.checkpoints.load(base) {
            if !state.0.finalized() {
                return Err(BankError::CheckpointNotFinalized);
            }
            let new = state.0.fork(current, last_id);
            self.checkpoints.store(current, Arc::new(new), base);
            self.live_bank_state = current;
            Ok(())
        } else {
            return Err(BankError::UnknownFork);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank_state::BankCheckpoint;
    use solana_sdk::hash::hash;

    #[test]
    fn forks_init_root() {
        let mut forks = Forks::default();
        let cp = BankCheckpoint::new(0, &Hash::default());
        forks.init_root_bank_state(cp);
        assert!(forks.is_active_fork(0));
        assert_eq!(forks.root_bank_state().checkpoints.len(), 1);
        assert_eq!(forks.root_bank_state().head().fork_id(), 0);
        assert_eq!(forks.live_bank_state().head().fork_id(), 0);
    }

    #[test]
    fn forks_init_fork() {
        let mut forks = Forks::default();
        let last_id = Hash::default();
        let cp = BankCheckpoint::new(0, &last_id);
        cp.register_tick(&last_id);
        forks.init_root_bank_state(cp);
        let last_id = hash(last_id.as_ref());
        assert_eq!(forks.init_fork(1, &last_id, 1), Err(BankError::UnknownFork));
        assert_eq!(
            forks.init_fork(1, &last_id, 0),
            Err(BankError::CheckpointNotFinalized)
        );
        forks.root_bank_state().head().finalize();
        assert_eq!(forks.init_fork(1, &last_id, 0), Ok(()));

        assert_eq!(forks.root_bank_state().head().fork_id(), 0);
        assert_eq!(forks.live_bank_state().head().fork_id(), 1);
        assert_eq!(forks.live_bank_state().checkpoints.len(), 2);
    }

    #[test]
    fn forks_merge() {
        let mut forks = Forks::default();
        let last_id = Hash::default();
        let cp = BankCheckpoint::new(0, &last_id);
        cp.register_tick(&last_id);
        forks.init_root_bank_state(cp);
        let last_id = hash(last_id.as_ref());
        forks.root_bank_state().head().finalize();
        assert_eq!(forks.init_fork(1, &last_id, 0), Ok(()));
        forks.live_bank_state().head().register_tick(&last_id);
        forks.live_bank_state().head().finalize();
        assert_eq!(forks.merge_into_root(0, 1), Ok(1));

        assert_eq!(forks.live_bank_state().checkpoints.len(), 1);
        assert_eq!(forks.root_bank_state().head().fork_id(), 1);
        assert_eq!(forks.live_bank_state().head().fork_id(), 1);
    }
    #[test]
    fn forks_merge_prune() {
        let mut forks = Forks::default();
        let last_id = Hash::default();
        let cp = BankCheckpoint::new(0, &last_id);
        cp.register_tick(&last_id);
        forks.init_root_bank_state(cp);
        let last_id = hash(last_id.as_ref());
        forks.root_bank_state().head().finalize();
        assert_eq!(forks.init_fork(1, &last_id, 0), Ok(()));
        assert_eq!(forks.bank_state(1).unwrap().checkpoints.len(), 2);
        forks.bank_state(1).unwrap().head().register_tick(&last_id);

        // add a fork 2 to be pruned
        // fork 2 connects to 0
        let last_id = hash(last_id.as_ref());
        assert_eq!(forks.init_fork(2, &last_id, 0), Ok(()));
        assert_eq!(forks.bank_state(2).unwrap().checkpoints.len(), 2);
        forks.bank_state(2).unwrap().head().register_tick(&last_id);

        forks.bank_state(1).unwrap().head().finalize();
        // fork 1 is the new root, only forks that are descendant from 1 are valid
        assert_eq!(forks.merge_into_root(0, 1), Ok(1));

        // fork 2 is gone since it does not connect to 1
        assert!(forks.bank_state(2).is_none());

        assert_eq!(forks.live_bank_state().checkpoints.len(), 1);
        assert_eq!(forks.root_bank_state().head().fork_id(), 1);
        assert_eq!(forks.live_bank_state().head().fork_id(), 1);
    }
}
