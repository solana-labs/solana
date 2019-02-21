use crate::active_stakers::ActiveStakers;
use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_sdk::pubkey::Pubkey;
use std::ops::Index;

/// Round-robin leader schedule.
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
}

impl LeaderSchedule {
    pub fn from_schedule(slot_leaders: Vec<Pubkey>) -> Self {
        Self { slot_leaders }
    }

    pub fn new(active_stakers: &ActiveStakers, seed: &[u8; 32], slots_per_epoch: u64) -> Self {
        let (pubkeys, stakes): (Vec<Pubkey>, Vec<u64>) =
            active_stakers.sorted_stakes().into_iter().unzip();
        let mut rng = ChaChaRng::from_seed(*seed);
        // Should have no zero weighted stakes
        let weighted_index = WeightedIndex::new(stakes).unwrap();
        let slot_leaders = (0..slots_per_epoch)
            .map(|_| {
                let i = weighted_index.sample(&mut rng);
                pubkeys[i]
            })
            .collect();

        LeaderSchedule { slot_leaders }
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
        let leader_schedule = LeaderSchedule::from_schedule(vec![pubkey0, pubkey1]);
        assert_eq!(leader_schedule[0], pubkey0);
        assert_eq!(leader_schedule[1], pubkey1);
        assert_eq!(leader_schedule[2], pubkey0);
    }

    #[test]
    fn test_new_leader_schedule() {
        let num_keys = 10;
        let stakes = (0..num_keys)
            .map(|i| (Keypair::new().pubkey(), i))
            .collect();
        let active_stakers = ActiveStakers::from_stakes(stakes);

        let seed = Keypair::new().pubkey();
        let mut seed_bytes = [0u8; 32];
        seed_bytes.copy_from_slice(seed.as_ref());
        let slots_per_epoch = num_keys * 10;
        let leader_schedule = LeaderSchedule::new(&active_stakers, &seed_bytes, slots_per_epoch);
        let leader_schedule2 = LeaderSchedule::new(&active_stakers, &seed_bytes, slots_per_epoch);
        assert_eq!(leader_schedule.slot_leaders.len() as u64, slots_per_epoch);
        // Check that the same schedule is reproducibly generated
        assert_eq!(leader_schedule.slot_leaders, leader_schedule2.slot_leaders);
    }
}
