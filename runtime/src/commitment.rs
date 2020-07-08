use solana_sdk::clock::Slot;
use solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY;
use std::collections::HashMap;

pub const VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64;

pub type BlockCommitmentArray = [u64; MAX_LOCKOUT_HISTORY + 1];

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockCommitment {
    pub commitment: BlockCommitmentArray,
}

impl BlockCommitment {
    pub fn increase_confirmation_stake(&mut self, confirmation_count: usize, stake: u64) {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.commitment[confirmation_count - 1] += stake;
    }

    pub fn get_confirmation_stake(&mut self, confirmation_count: usize) -> u64 {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.commitment[confirmation_count - 1]
    }

    pub fn increase_rooted_stake(&mut self, stake: u64) {
        self.commitment[MAX_LOCKOUT_HISTORY] += stake;
    }

    pub fn get_rooted_stake(&self) -> u64 {
        self.commitment[MAX_LOCKOUT_HISTORY]
    }

    pub fn new(commitment: BlockCommitmentArray) -> Self {
        Self { commitment }
    }
}

#[derive(Default)]
pub struct BlockCommitmentCache {
    block_commitment: HashMap<Slot, BlockCommitment>,
    highest_confirmed_root: Slot,
    total_stake: u64,
    /// The slot of the bank from which all other slots were calculated.
    slot: Slot,
    root: Slot,
    pub highest_confirmed_slot: Slot,
}

impl std::fmt::Debug for BlockCommitmentCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockCommitmentCache")
            .field("block_commitment", &self.block_commitment)
            .field("total_stake", &self.total_stake)
            .field(
                "bank",
                &format_args!("Bank({{current_slot: {:?}}})", self.slot),
            )
            .field("root", &self.root)
            .finish()
    }
}

impl BlockCommitmentCache {
    pub fn new(
        block_commitment: HashMap<Slot, BlockCommitment>,
        highest_confirmed_root: Slot,
        total_stake: u64,
        slot: Slot,
        root: Slot,
        highest_confirmed_slot: Slot,
    ) -> Self {
        Self {
            block_commitment,
            highest_confirmed_root,
            total_stake,
            slot,
            root,
            highest_confirmed_slot,
        }
    }

    pub fn get_block_commitment(&self, slot: Slot) -> Option<&BlockCommitment> {
        self.block_commitment.get(&slot)
    }

    pub fn highest_confirmed_root(&self) -> Slot {
        self.highest_confirmed_root
    }

    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn root(&self) -> Slot {
        self.root
    }

    pub fn highest_confirmed_slot(&self) -> Slot {
        self.highest_confirmed_slot
    }

    fn highest_slot_with_confirmation_count(&self, confirmation_count: usize) -> Slot {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        for slot in (self.root()..self.slot()).rev() {
            if let Some(count) = self.get_confirmation_count(slot) {
                if count >= confirmation_count {
                    return slot;
                }
            }
        }
        self.root
    }

    pub fn calculate_highest_confirmed_slot(&self) -> Slot {
        self.highest_slot_with_confirmation_count(1)
    }

    pub fn get_confirmation_count(&self, slot: Slot) -> Option<usize> {
        self.get_lockout_count(slot, VOTE_THRESHOLD_SIZE)
    }

    // Returns the lowest level at which at least `minimum_stake_percentage` of the total epoch
    // stake is locked out
    fn get_lockout_count(&self, slot: Slot, minimum_stake_percentage: f64) -> Option<usize> {
        self.get_block_commitment(slot).map(|block_commitment| {
            let iterator = block_commitment.commitment.iter().enumerate().rev();
            let mut sum = 0;
            for (i, stake) in iterator {
                sum += stake;
                if (sum as f64 / self.total_stake as f64) > minimum_stake_percentage {
                    return i + 1;
                }
            }
            0
        })
    }

    pub fn new_for_tests() -> Self {
        let mut block_commitment: HashMap<Slot, BlockCommitment> = HashMap::new();
        block_commitment.insert(0, BlockCommitment::default());
        Self {
            block_commitment,
            total_stake: 42,
            ..Self::default()
        }
    }

    pub fn new_for_tests_with_slots(slot: Slot, root: Slot) -> Self {
        let mut block_commitment: HashMap<Slot, BlockCommitment> = HashMap::new();
        block_commitment.insert(0, BlockCommitment::default());
        Self {
            block_commitment,
            total_stake: 42,
            highest_confirmed_root: root,
            slot,
            root,
            highest_confirmed_slot: root,
        }
    }

    pub fn set_highest_confirmed_root(&mut self, root: Slot) {
        self.highest_confirmed_root = root;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_commitment() {
        let mut cache = BlockCommitment::default();
        assert_eq!(cache.get_confirmation_stake(1), 0);
        cache.increase_confirmation_stake(1, 10);
        assert_eq!(cache.get_confirmation_stake(1), 10);
        cache.increase_confirmation_stake(1, 20);
        assert_eq!(cache.get_confirmation_stake(1), 30);
    }

    #[test]
    fn test_get_confirmations() {
        // Build BlockCommitmentCache with votes at depths 0 and 1 for 2 slots
        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(1, 5);
        cache0.increase_confirmation_stake(2, 40);

        let mut cache1 = BlockCommitment::default();
        cache1.increase_confirmation_stake(1, 40);
        cache1.increase_confirmation_stake(2, 5);

        let mut cache2 = BlockCommitment::default();
        cache2.increase_confirmation_stake(1, 20);
        cache2.increase_confirmation_stake(2, 5);

        let mut block_commitment = HashMap::new();
        block_commitment.entry(0).or_insert(cache0);
        block_commitment.entry(1).or_insert(cache1);
        block_commitment.entry(2).or_insert(cache2);
        let block_commitment_cache = BlockCommitmentCache::new(block_commitment, 0, 50, 0, 0, 0);

        assert_eq!(block_commitment_cache.get_confirmation_count(0), Some(2));
        assert_eq!(block_commitment_cache.get_confirmation_count(1), Some(1));
        assert_eq!(block_commitment_cache.get_confirmation_count(2), Some(0),);
        assert_eq!(block_commitment_cache.get_confirmation_count(3), None,);
    }

    #[test]
    fn test_highest_confirmed_slot() {
        let bank_slot_5 = 5;
        let total_stake = 50;

        // Build cache with confirmation_count 2 given total_stake
        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(1, 5);
        cache0.increase_confirmation_stake(2, 40);

        // Build cache with confirmation_count 1 given total_stake
        let mut cache1 = BlockCommitment::default();
        cache1.increase_confirmation_stake(1, 40);
        cache1.increase_confirmation_stake(2, 5);

        // Build cache with confirmation_count 0 given total_stake
        let mut cache2 = BlockCommitment::default();
        cache2.increase_confirmation_stake(1, 20);
        cache2.increase_confirmation_stake(2, 5);

        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache0.clone()); // Slot 1, conf 2
        block_commitment.entry(2).or_insert_with(|| cache1.clone()); // Slot 2, conf 1
        block_commitment.entry(3).or_insert_with(|| cache2.clone()); // Slot 3, conf 0
        let block_commitment_cache =
            BlockCommitmentCache::new(block_commitment, 0, total_stake, bank_slot_5, 0, 0);

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 2);

        // Build map with multiple slots at conf 1
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache1.clone()); // Slot 1, conf 1
        block_commitment.entry(2).or_insert_with(|| cache1.clone()); // Slot 2, conf 1
        block_commitment.entry(3).or_insert_with(|| cache2.clone()); // Slot 3, conf 0
        let block_commitment_cache =
            BlockCommitmentCache::new(block_commitment, 0, total_stake, bank_slot_5, 0, 0);

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 2);

        // Build map with slot gaps
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache1.clone()); // Slot 1, conf 1
        block_commitment.entry(3).or_insert(cache1); // Slot 3, conf 1
        block_commitment.entry(5).or_insert_with(|| cache2.clone()); // Slot 5, conf 0
        let block_commitment_cache =
            BlockCommitmentCache::new(block_commitment, 0, total_stake, bank_slot_5, 0, 0);

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 3);

        // Build map with no conf 1 slots, but one higher
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert(cache0); // Slot 1, conf 2
        block_commitment.entry(2).or_insert_with(|| cache2.clone()); // Slot 2, conf 0
        block_commitment.entry(3).or_insert_with(|| cache2.clone()); // Slot 3, conf 0
        let block_commitment_cache =
            BlockCommitmentCache::new(block_commitment, 0, total_stake, bank_slot_5, 0, 0);

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 1);

        // Build map with no conf 1 or higher slots
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache2.clone()); // Slot 1, conf 0
        block_commitment.entry(2).or_insert_with(|| cache2.clone()); // Slot 2, conf 0
        block_commitment.entry(3).or_insert(cache2); // Slot 3, conf 0
        let block_commitment_cache =
            BlockCommitmentCache::new(block_commitment, 0, total_stake, bank_slot_5, 0, 0);

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 0);
    }
}
