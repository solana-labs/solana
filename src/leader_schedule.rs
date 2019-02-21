use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_sdk::pubkey::Pubkey;
use std::ops::Index;

/// Round-robin leader schedule.
#[derive(Debug, PartialEq)]
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
}

impl LeaderSchedule {
    pub fn new(ids_and_stakes: &[(Pubkey, u64)], seed: &[u8; 32], slots_per_epoch: u64) -> Self {
        let (pubkeys, stakes): (Vec<Pubkey>, Vec<u64>) = ids_and_stakes
            .iter()
            .map(|&(ref id, ref stake)| (id, stake))
            .unzip();

        // Should have no zero weighted stakes
        let mut rng = ChaChaRng::from_seed(*seed);
        let weighted_index = WeightedIndex::new(stakes).unwrap();
        let slot_leaders = (0..slots_per_epoch)
            .map(|_| pubkeys[weighted_index.sample(&mut rng)])
            .collect();

        Self { slot_leaders }
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
    use solana_sdk::signature::{Keypair, KeypairUtil};

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
    fn test_new_leader_schedule() {
        let num_keys = 10;
        let stakes: Vec<_> = (0..num_keys)
            .map(|i| (Keypair::new().pubkey(), i))
            .collect();

        let seed = Keypair::new().pubkey();
        let mut seed_bytes = [0u8; 32];
        seed_bytes.copy_from_slice(seed.as_ref());
        let slots_per_epoch = num_keys * 10;
        let leader_schedule = LeaderSchedule::new(&stakes, &seed_bytes, slots_per_epoch);
        let leader_schedule2 = LeaderSchedule::new(&stakes, &seed_bytes, slots_per_epoch);
        assert_eq!(leader_schedule.slot_leaders.len() as u64, slots_per_epoch);
        // Check that the same schedule is reproducibly generated
        assert_eq!(leader_schedule, leader_schedule2);
    }
}
