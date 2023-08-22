use {
    itertools::Itertools,
    rand::distributions::{Distribution, WeightedIndex},
    rand_chacha::{rand_core::SeedableRng, ChaChaRng},
    solana_sdk::pubkey::Pubkey,
    std::{collections::HashMap, convert::identity, ops::Index, sync::Arc},
};

// Used for testing
#[derive(Clone, Debug)]
pub struct FixedSchedule {
    pub leader_schedule: Arc<LeaderSchedule>,
}

/// Stake-weighted leader schedule for one epoch.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
    // Inverted index from pubkeys to indices where they are the leader.
    index: HashMap<Pubkey, Arc<Vec<usize>>>,
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
                }
                current_node
            })
            .collect();
        Self::new_from_schedule(slot_leaders)
    }

    pub fn new_from_schedule(slot_leaders: Vec<Pubkey>) -> Self {
        let index = slot_leaders
            .iter()
            .enumerate()
            .map(|(i, pk)| (*pk, i))
            .into_group_map()
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect();
        Self {
            slot_leaders,
            index,
        }
    }

    pub fn get_slot_leaders(&self) -> &[Pubkey] {
        &self.slot_leaders
    }

    pub fn num_slots(&self) -> usize {
        self.slot_leaders.len()
    }

    /// 'offset' is an index into the leader schedule. The function returns an
    /// iterator of indices i >= offset where the given pubkey is the leader.
    pub(crate) fn get_indices(
        &self,
        pubkey: &Pubkey,
        offset: usize, // Starting index.
    ) -> impl Iterator<Item = usize> {
        let index = self.index.get(pubkey).cloned().unwrap_or_default();
        let num_slots = self.slot_leaders.len();
        let size = index.len();
        #[allow(clippy::reversed_empty_ranges)]
        let range = if index.is_empty() {
            1..=0 // Intentionally empty range of type RangeInclusive.
        } else {
            let offset = index
                .binary_search(&(offset % num_slots))
                .unwrap_or_else(identity)
                + offset / num_slots * size;
            offset..=usize::MAX
        };
        // The modular arithmetic here and above replicate Index implementation
        // for LeaderSchedule, where the schedule keeps repeating endlessly.
        // The '%' returns where in a cycle we are and the '/' returns how many
        // times the schedule is repeated.
        range.map(move |k| index[k % size] + k / size * num_slots)
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
    use {super::*, rand::Rng, std::iter::repeat_with};

    #[test]
    fn test_leader_schedule_index() {
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let leader_schedule = LeaderSchedule::new_from_schedule(vec![pubkey0, pubkey1]);
        assert_eq!(leader_schedule[0], pubkey0);
        assert_eq!(leader_schedule[1], pubkey1);
        assert_eq!(leader_schedule[2], pubkey0);
    }

    #[test]
    fn test_leader_schedule_basic() {
        let num_keys = 10;
        let stakes: Vec<_> = (0..num_keys)
            .map(|i| (solana_sdk::pubkey::new_rand(), i))
            .collect();

        let seed = solana_sdk::pubkey::new_rand();
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
        let stakes: Vec<_> = (0..num_keys)
            .map(|i| (solana_sdk::pubkey::new_rand(), i))
            .collect();

        let seed = solana_sdk::pubkey::new_rand();
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
        let alice_pubkey = solana_sdk::pubkey::new_rand();
        let bob_pubkey = solana_sdk::pubkey::new_rand();
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

    #[test]
    fn test_get_indices() {
        const NUM_SLOTS: usize = 97;
        let mut rng = rand::thread_rng();
        let pubkeys: Vec<_> = repeat_with(Pubkey::new_unique).take(4).collect();
        let schedule: Vec<_> = repeat_with(|| pubkeys[rng.gen_range(0..3)])
            .take(19)
            .collect();
        let schedule = LeaderSchedule::new_from_schedule(schedule);
        let leaders = (0..NUM_SLOTS)
            .map(|i| (schedule[i as u64], i))
            .into_group_map();
        for pubkey in &pubkeys {
            let index = leaders.get(pubkey).cloned().unwrap_or_default();
            for offset in 0..NUM_SLOTS {
                let schedule: Vec<_> = schedule
                    .get_indices(pubkey, offset)
                    .take_while(|s| *s < NUM_SLOTS)
                    .collect();
                let index: Vec<_> = index.iter().copied().skip_while(|s| *s < offset).collect();
                assert_eq!(schedule, index);
            }
        }
    }
}
