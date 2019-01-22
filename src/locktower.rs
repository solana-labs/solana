use std::collections::VecDeque;
pub struct VoteLock {
    /// Fork id, same as slot id. There can only be 1 fork per slot, since the empty PoH ticks
    /// cannot be voted on.  A validator cannot vote on a fork unless that fork is connected to
    /// a trunk. The starting trunk is the initial `finalized_fork`.
    fork: u64,
    /// Lockout is in terms of slots, so the slot height or fork id to unroll this
    /// vote must be greater than fork + lockout.
    lockout: u64,
}

pub enum LockTowerError {
    VoteLocked,
    VoteNotConverging,
}

pub struct LockTower {
    vote_locks: VecDeque<VoteLock>,
    /// The maximum size of vote_locks before reward is generated.  When the queue reached max_size,
    /// a reward is generated and the oldest lock is removed.  That fork is considered finalized.
    max_size: usize,
    /// Depth is equal to lockout, since lockout doubles as votes get stacked
    /// If the requirement is at depth 8 to have converge_amount of 0.667,
    /// than the lockout will not increase past 256 (2^8) on the fork at depth 8
    /// without 66.7% total network commitment to that fork.
    ///
    /// It is not important for the rest of the network to also have the same lockout on the fork.
    /// The worst case is that the lockout for this node will double to 512 due to
    /// rollback for the rest of the network.  So any lockout of network commitment is acceptable,
    /// but it must cover the required threshold percentage of network stake.
    converge_depth: usize,
    converge_amount: f64,
    /// The last finalized fork.  This value is initialized at the start with some pre-defined
    /// network value, such as fork 0.
    finalized_fork: u64,
}

/// This should be implemented by the Bank
pub trait ForkTree {
    /// Return true if the leaf is a descendant of the trunk.
    fn is_trunk_of(&self, trunk: u64, leaf: u64) -> bool;
    /// Return what percentage of the network has agreed to the fork or a descendant fork.
    fn convergance(&self, fork: u64) -> f64;
}

impl LockTower {
    pub fn new(
        max_size: usize,
        converge_depth: usize,
        converge_amount: f64,
        finalized_fork: u64,
    ) -> Self {
        LockTower {
            vote_locks: VecDeque::new(),
            max_size,
            converge_depth,
            converge_amount,
            finalized_fork,
        }
    }

    /// * vote - fork id to vote on
    /// Returns true if this vote is unlocked.
    pub fn is_unlocked<T: ForkTree>(&self, vote: u64, tree: &T) -> bool {
        let ix = self.num_cleared_votes(vote);
        self.vote_locks
            .get(ix)
            .map(|lockout| tree.is_trunk_of(lockout.fork, vote))
            .unwrap_or(tree.is_trunk_of(self.finalized_fork, vote))
    }

    /// * vote - fork id to vote on
    /// Returns true if this vote is converging at the configured converge_depth
    /// and converge_amount.
    pub fn is_converging<T: ForkTree>(&self, vote: u64, tree: &T) -> bool {
        let ix = self.num_cleared_votes(vote);
        let depth = ix + self.converge_depth;
        self.vote_locks
            .get(depth)
            .map(|lockout| self.converge_amount < tree.convergance(lockout.fork))
            .unwrap_or(true)
    }

    /// * vote - fork id to vote on
    /// Total lockout of this vote.  This function simulates the generated lockout if this vote was
    /// applied.
    /// The fork selection algorithm should compute the vote_weight for everyone in the network for
    /// this vote and pick the highest stake weighted option.
    pub fn vote_weight<T: ForkTree>(&mut self, vote: u64, tree: &T) -> u64 {
        if !self.is_converging(vote, tree) {
            return 0;
        }
        if !self.is_unlocked(vote, tree) {
            return 0;
        }
        // skip any expired votes
        let ix = self.num_cleared_votes(vote);
        let mut prev = 2;
        let mut total = 2;
        for ix in ix..self.vote_locks.len() {
            if self.vote_locks[ix].lockout == prev {
                // simulate doubling of vote_locks
                total += self.vote_locks[ix].lockout * 2;
                prev = self.vote_locks[ix].lockout * 2;
            } else {
                total += self.vote_locks[ix].lockout;
                prev = self.vote_locks[ix].lockout;
            }
        }
        total
    }

    /// Enter a new vote, and return the reward for it if any
    pub fn enter_vote<T: ForkTree>(
        &mut self,
        vote: u64,
        tree: &T,
    ) -> Result<Option<VoteLock>, LockTowerError> {
        if !self.is_converging(vote, tree) {
            return Err(LockTowerError::VoteNotConverging);
        }
        if !self.is_unlocked(vote, tree) {
            return Err(LockTowerError::VoteLocked);
        }
        // remove expired votes
        let ix = self.num_cleared_votes(vote);
        let _ = self.vote_locks.drain(..ix);

        //sanity check, rewards should be pop such that this never happens
        assert!(self.vote_locks.len() < self.max_size);

        // push the new vote
        self.vote_locks.push_front(VoteLock {
            fork: vote,
            lockout: 2,
        });
        for ix in 1..self.vote_locks.len() {
            // This will double any vote_locks if the stack is full
            // and will stop doubling until the tower is rebuilt
            if self.vote_locks[ix - 1].lockout == self.vote_locks[ix].lockout {
                self.vote_locks[ix].lockout *= 2;
            }
        }
        // make sure we pop the reward
        Ok(self.pop_reward())
    }

    /// Generate a reward if the vote_locks queue is full.
    /// The finalized fork is updated
    fn pop_reward(&mut self) -> Option<VoteLock> {
        if self.max_size == self.vote_locks.len() {
            let reward = self.vote_locks.pop_back();
            reward.iter().for_each(|r| {
                self.finalized_fork = r.fork;
            });
            reward
        } else {
            None
        }
    }

    /// find the number of votes that are cleared by this vote
    fn num_cleared_votes(&self, vote: u64) -> usize {
        let mut ret = 0;
        for (i, v) in self.vote_locks.iter().enumerate() {
            if v.fork + v.lockout < vote {
                ret = i + 1;
            }
        }
        ret
    }
}
