use crate::bank::Bank;
use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_sdk::pubkey::Pubkey;
use std::ops::Index;

/// Stake-weighted leader schedule for one epoch.
#[derive(Debug, PartialEq)]
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
}

impl LeaderSchedule {
    // Note: passing in zero stakers will cause a panic.
    pub fn new(ids_and_stakes: &[(Pubkey, u64)], seed: [u8; 32], len: u64) -> Self {
        let (ids, stakes): (Vec<_>, Vec<_>) = ids_and_stakes.iter().cloned().unzip();
        let rng = &mut ChaChaRng::from_seed(seed);
        let weighted_index = WeightedIndex::new(stakes).unwrap();
        let slot_leaders = (0..len).map(|_| ids[weighted_index.sample(rng)]).collect();
        Self { slot_leaders }
    }

    pub fn new_with_bank(bank: &Bank) -> Self {
        let id_and_stakes: Vec<_> = bank.staked_nodes().into_iter().collect();
        let mut seed = [0u8; 32];
        seed[0..8].copy_from_slice(&bank.epoch_height().to_le_bytes());
        Self::new(&id_and_stakes, seed, bank.slots_per_epoch())
    }
}

impl Index<usize> for LeaderSchedule {
    type Output = Pubkey;
    fn index(&self, index: usize) -> &Pubkey {
        &self.slot_leaders[index % self.slot_leaders.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::iter;

    #[test]
    fn test_leader_schedule_index() {
        let pubkey0 = Keypair::new().pubkey();
        let pubkey1 = Keypair::new().pubkey();
        let leader_schedule = LeaderSchedule {
            slot_leaders: vec![pubkey0, pubkey1],
        };
        assert_eq!(leader_schedule[0], pubkey0);
        assert_eq!(leader_schedule[1], pubkey1);
        assert_eq!(leader_schedule[2], pubkey0);
    }

    #[test]
    fn test_leader_schedule_basic() {
        let num_keys = 10;
        let stakes: Vec<_> = (0..num_keys)
            .map(|i| (Keypair::new().pubkey(), i))
            .collect();

        let seed = Keypair::new().pubkey();
        let mut seed_bytes = [0u8; 32];
        seed_bytes.copy_from_slice(seed.as_ref());
        let len = num_keys * 10;
        let leader_schedule = LeaderSchedule::new(&stakes, seed_bytes, len);
        let leader_schedule2 = LeaderSchedule::new(&stakes, seed_bytes, len);
        assert_eq!(leader_schedule.slot_leaders.len() as u64, len);
        // Check that the same schedule is reproducibly generated
        assert_eq!(leader_schedule, leader_schedule2);
    }

    #[test]
    fn test_leader_schedule_via_bank() {
        let pubkey = Keypair::new().pubkey();
        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(2, pubkey, 2);
        let bank = Bank::new(&genesis_block);
        let leader_schedule = LeaderSchedule::new_with_bank(&bank);
        let len = bank.slots_per_epoch() as usize;
        let expected: Vec<_> = iter::repeat(pubkey).take(len).collect();
        assert_eq!(leader_schedule.slot_leaders, expected);
    }
}
