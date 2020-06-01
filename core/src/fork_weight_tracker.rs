use solana_sdk::{
    clock::Slot,
    pubkey::Pubkey,
};
use std::collections::{BTreeMap, HashMap};

pub type ForkId = u64;
pub type ForkWeight = usize;

struct ValidatorLatestVote {
    latest_vote: Slot
    fork_id: ForkId,
}

struct ForkInfo {
    id: ForkId,
    parent: ForkId,
    weight: ForkWeight,
}

struct ForkWeightTracker {
    fork_infos: HashMap<ForkId, ForkInfo>,
    slot_to_fork: BTreeMap<Slot, ForkId>,
    latest_votes: HashMap<Pubkey, ValidatorLatestVote>,
    root: Slot,
}

impl ForkWeightTracker {
    pub fn add_vote(&mut self, pubkey: &Pubkey) {

    }
    
    pub fn update_root(&mut self, root: Slot, descendants: &mut HashMap<Slot, HashSet<Slot>>,) {
        self.root = root;
        let removed = 
    }

    pub fn get_weight()
}

