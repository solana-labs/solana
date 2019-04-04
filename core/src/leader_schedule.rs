use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_sdk::pubkey::Pubkey;
use std::ops::Index;

/// Stake-weighted leader schedule for one epoch.
#[derive(Debug, Default, PartialEq)]
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
}

impl LeaderSchedule {
    // Note: passing in zero stakers will cause a panic.
    pub fn new(ids_and_stakes: &[(Pubkey, u64)], seed: [u8; 32], len: u64, repeat: u64) -> Self {
        let (ids, stakes): (Vec<_>, Vec<_>) = ids_and_stakes.iter().cloned().unzip();
        let rng = &mut ChaChaRng::from_seed(seed);
        let weighted_index = WeightedIndex::new(stakes).unwrap();
        let mut current_node = Pubkey::default();
        let slot_leaders = (0..len)
            .map(|i| {
                if i % repeat == 0 {
                    current_node = ids[weighted_index.sample(rng)];
                    current_node
                } else {
                    current_node
                }
            })
            .collect();
        Self { slot_leaders }
    }
}

impl Index<u64> for LeaderSchedule {
    type Output = Pubkey;
    fn index(&self, index: u64) -> &Pubkey {
        let index = index as usize;
        &self.slot_leaders[index % self.slot_leaders.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_schedule_index() {
        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
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
        let stakes: Vec<_> = (0..num_keys).map(|i| (Pubkey::new_rand(), i)).collect();

        let seed = Pubkey::new_rand();
        let mut seed_bytes = [0u8; 32];
        seed_bytes.copy_from_slice(seed.as_ref());
        let len = num_keys * 10;
        let leader_schedule = LeaderSchedule::new(&stakes, seed_bytes, len, 1);
        let leader_schedule2 = LeaderSchedule::new(&stakes, seed_bytes, len, 1);
        assert_eq!(leader_schedule.slot_leaders.len() as u64, len);
        // Check that the same schedule is reproducibly generated
        assert_eq!(leader_schedule, leader_schedule2);
    }

    #[test]
    fn test_repeated_leader_schedule() {
        let num_keys = 10;
        let stakes: Vec<_> = (0..num_keys).map(|i| (Pubkey::new_rand(), i)).collect();

        let seed = Pubkey::new_rand();
        let mut seed_bytes = [0u8; 32];
        seed_bytes.copy_from_slice(seed.as_ref());
        let len = num_keys * 10;
        let repeat = 8;
        let leader_schedule = LeaderSchedule::new(&stakes, seed_bytes, len, repeat);
        assert_eq!(leader_schedule.slot_leaders.len() as u64, len);
        let mut leader_node = Pubkey::default();
        for (i, node) in leader_schedule.slot_leaders.iter().enumerate() {
            if i % repeat as usize == 0 {
                leader_node = *node;
            } else {
                assert_eq!(leader_node, *node);
            }
        }
    }

    #[test]
    fn test_repeated_leader_schedule_specific() {
        let alice_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let stakes = vec![(alice_pubkey, 2), (bob_pubkey, 1)];

        let seed = Pubkey::default();
        let mut seed_bytes = [0u8; 32];
        seed_bytes.copy_from_slice(seed.as_ref());
        let len = 8;
        // What the schedule looks like without any repeats
        let leaders1 = LeaderSchedule::new(&stakes, seed_bytes, len, 1).slot_leaders;

        // What the schedule looks like with repeats
        let leaders2 = LeaderSchedule::new(&stakes, seed_bytes, len, 2).slot_leaders;
        assert_eq!(leaders1.len(), leaders2.len());

        let leaders1_expected = vec![
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
            bob_pubkey,
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
        ];
        let leaders2_expected = vec![
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
            alice_pubkey,
            bob_pubkey,
            bob_pubkey,
        ];

        assert_eq!(leaders1, leaders1_expected);
        assert_eq!(leaders2, leaders2_expected);
    }
}
