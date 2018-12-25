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
    pub live_fork: u64,

    /// Fork that is trunk
    pub trunk_fork: u64,
}

impl Forks {
    pub fn live_fork(&self) -> BankState {
        self.bank_state(self.live_fork)
    }
    pub fn trunk_fork(&self) -> BankState {
        self.bank_state(self.trunk_fork)
    }

    pub fn bank_state(&self, fork: u64) -> BankState {
        BankState {
            checkpoints: self
                .checkpoints
                .collect(ROLLBACK_DEPTH, fork)
                .into_iter()
                .map(|x| x.1)
                .cloned()
                .collect(),
        }
    }
    /// Collapse the bottom two checkpoints.
    /// The tree is computed from the `leaf` to the `trunk`
    /// The expected last fork must be the trunk
    /// The direct child that leads from the trunk to the leaf becomes the new trunk.
    /// Return the new trunk id.
    pub fn merge_into_trunk(&mut self, trunk: u64, leaf: u64) -> Result<u64> {
        // `old` trunk, should have `trunk` as its fork_id
        // `new` trunk is a direct decendant of old and has new_trunk_id as its fork_id
        // new is merged into old
        // and old is swapped into the checkpoint under new_trunk_id
        let (old, new, new_trunk_id) = {
            let states = self.checkpoints.collect(ROLLBACK_DEPTH + 1, leaf);
            let leaf_id = states.first().map(|x| x.0).ok_or(BankError::UnknownFork)?;
            assert_eq!(leaf_id, leaf);
            let trunk_id = states.last().map(|x| x.0).ok_or(BankError::UnknownFork)?;
            if trunk_id != trunk {
                return Err(BankError::InvalidTrunk);
            }
            let len = states.len();
            let old_trunk = states[len - 1].clone();
            let merged = states[len - 2].clone();
            (old_trunk.1.clone(), merged.1.clone(), merged.0)
        };
        let idag = self.checkpoints.invert();
        let new_checkpoints = self.checkpoints.prune(new_trunk_id, &idag);
        let old_id = old.fork_id();
        self.checkpoints = new_checkpoints;
        self.trunk_fork = new_trunk_id;
        // old should have been pruned
        assert!(self.checkpoints.load(old_id).is_none());
        // new id should be in the new tree
        assert!(!self.checkpoints.load(new_trunk_id).is_none());

        // swap in the old instance under the new id
        // this should be the last external ref to `new`
        self.checkpoints.insert(new_trunk_id, old.clone(), old_id);

        // merge all the new changes into the old instance under the new id
        // this should consume `new`
        // new should have no other references
        let new: BankCheckpoint = Arc::try_unwrap(new).unwrap();
        old.merge_into_trunk(new);
        assert_eq!(old.fork_id(), new_trunk_id);
        Ok(new_trunk_id)
    }

    /// Initialize the first trunk
    pub fn init_trunk_fork(&mut self, checkpoint: BankCheckpoint) {
        assert!(self.checkpoints.is_empty());
        self.live_fork = checkpoint.fork_id();
        self.trunk_fork = checkpoint.fork_id();
        //TODO: using u64::MAX as the impossible checkpoint
        //this should be a None instead
        self.checkpoints
            .store(self.live_fork, Arc::new(checkpoint), std::u64::MAX);
    }

    pub fn is_active_fork(&self, fork: u64) -> bool {
        if let Some(state) = self.checkpoints.load(fork) {
            !state.0.finalized() && self.live_fork == fork
        } else {
            false
        }
    }
    /// Initalize the `current` fork that is a direct decendant of the `base` fork.
    pub fn init_fork(&mut self, current: u64, last_id: &Hash, base: u64) -> Result<()> {
        if let Some(state) = self.checkpoints.load(base) {
            if !state.0.finalized() {
                return Err(BankError::BaseCheckpointNotFinalized);
            }
            let new = state.0.fork(current, last_id);
            self.checkpoints.store(current, Arc::new(new), base);
            self.live_fork = current;
            Ok(())
        } else {
            return Err(BankError::BaseCheckpointMissing);
        }
    }
}
