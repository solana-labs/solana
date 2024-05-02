use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    hash::Hash,
    vote::state::{TowerSync, Vote, VoteStateUpdate},
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum VoteTransaction {
    Vote(Vote),
    VoteStateUpdate(VoteStateUpdate),
    TowerSync(TowerSync),
}

impl VoteTransaction {
    pub fn slots(&self) -> Vec<Slot> {
        match self {
            VoteTransaction::Vote(vote) => vote.slots.clone(),
            VoteTransaction::VoteStateUpdate(vote_state_update) => vote_state_update
                .lockouts
                .iter()
                .map(|lockout| lockout.slot())
                .collect(),
            VoteTransaction::TowerSync(tower_sync) => tower_sync.slots(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            VoteTransaction::Vote(vote) => vote.slots.is_empty(),
            VoteTransaction::VoteStateUpdate(vote_state_update) => {
                vote_state_update.lockouts.is_empty()
            }
            VoteTransaction::TowerSync(tower_sync) => tower_sync.lockouts.is_empty(),
        }
    }

    pub fn hash(&self) -> Hash {
        match self {
            VoteTransaction::Vote(vote) => vote.hash,
            VoteTransaction::VoteStateUpdate(vote_state_update) => vote_state_update.hash,
            VoteTransaction::TowerSync(tower_sync) => tower_sync.hash,
        }
    }

    pub fn timestamp(&self) -> Option<UnixTimestamp> {
        match self {
            VoteTransaction::Vote(vote) => vote.timestamp,
            VoteTransaction::VoteStateUpdate(vote_state_update) => vote_state_update.timestamp,
            VoteTransaction::TowerSync(tower_sync) => tower_sync.timestamp,
        }
    }

    pub fn last_voted_slot(&self) -> Option<Slot> {
        match self {
            VoteTransaction::Vote(vote) => vote.slots.last().copied(),
            VoteTransaction::VoteStateUpdate(vote_state_update) => {
                Some(vote_state_update.lockouts.back()?.slot())
            }
            VoteTransaction::TowerSync(tower_sync) => tower_sync.last_voted_slot(),
        }
    }

    pub fn last_voted_slot_hash(&self) -> Option<(Slot, Hash)> {
        Some((self.last_voted_slot()?, self.hash()))
    }
}

impl From<Vote> for VoteTransaction {
    fn from(vote: Vote) -> Self {
        VoteTransaction::Vote(vote)
    }
}

impl From<VoteStateUpdate> for VoteTransaction {
    fn from(vote_state_update: VoteStateUpdate) -> Self {
        VoteTransaction::VoteStateUpdate(vote_state_update)
    }
}

impl From<TowerSync> for VoteTransaction {
    fn from(tower_sync: TowerSync) -> Self {
        VoteTransaction::TowerSync(tower_sync)
    }
}
