use crate::active_stakers::ActiveStakers;
use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use std::ops::Index;

/// Round-robin leader schedule.
#[derive(Debug, PartialEq)]
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
}

impl LeaderSchedule {
    pub fn new(ids_and_stakes: &[(Pubkey, u64)], seed: &[u8; 32], len: u64) -> Self {
        let (pubkeys, stakes): (Vec<Pubkey>, Vec<u64>) = ids_and_stakes
            .iter()
            .map(|&(ref id, ref stake)| (id, stake))
            .unzip();

        // Should have no zero weighted stakes
        let mut rng = ChaChaRng::from_seed(*seed);
        let weighted_index = WeightedIndex::new(stakes).unwrap();
        let slot_leaders = (0..len)
            .map(|_| pubkeys[weighted_index.sample(&mut rng)])
            .collect();

        Self { slot_leaders }
    }

    pub fn new_with_bank(bank: &Bank) -> Self {
        let active_stakers = ActiveStakers::new(&bank);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(bank.last_id().as_ref());
        Self::new(
            &active_stakers.sorted_stakes(),
            &seed,
            bank.slots_per_epoch(),
        )
    }
}

impl Index<usize> for LeaderSchedule {
    type Output = Pubkey;
    fn index(&self, index: usize) -> &Pubkey {
        &self.slot_leaders[index % self.slot_leaders.len()]
    }
}

trait LeaderScheduleUtil {
    /// Return the leader schedule for the current epoch.
    fn leader_schedule(&self) -> LeaderSchedule;
}

impl LeaderScheduleUtil for Bank {
    fn leader_schedule(&self) -> LeaderSchedule {
        match self.leader_schedule_bank() {
            None => LeaderSchedule::new_with_bank(self),
            Some(bank) => LeaderSchedule::new_with_bank(&bank),
        }
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
        let leader_schedule = LeaderSchedule::new(&stakes, &seed_bytes, len);
        let leader_schedule2 = LeaderSchedule::new(&stakes, &seed_bytes, len);
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
        assert_eq!(bank.leader_schedule().slot_leaders, expected); // Same thing, but with the trait
    }
}
