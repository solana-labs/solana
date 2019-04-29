use crate::blocktree::Blocktree;
use crate::leader_schedule::LeaderSchedule;
use crate::leader_schedule_utils;
use solana_runtime::bank::{Bank, EpochSchedule};
use solana_sdk::pubkey::Pubkey;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

type CachedSchedules = (HashMap<u64, Arc<LeaderSchedule>>, VecDeque<u64>);
const MAX_SCHEDULES: usize = 10;

#[derive(Default)]
pub struct LeaderScheduleCache {
    // Map from an epoch to a leader schedule for that epoch
    pub cached_schedules: RwLock<CachedSchedules>,
    epoch_schedule: EpochSchedule,
}

impl LeaderScheduleCache {
    pub fn new_from_bank(bank: &Bank) -> Self {
        Self::new(*bank.epoch_schedule())
    }

    pub fn new(epoch_schedule: EpochSchedule) -> Self {
        Self {
            cached_schedules: RwLock::new((HashMap::new(), VecDeque::new())),
            epoch_schedule,
        }
    }

    pub fn slot_leader_at(&self, slot: u64) -> Option<Pubkey> {
        let (epoch, slot_index) = self.epoch_schedule.get_epoch_and_slot_index(slot);
        self.cached_schedules
            .read()
            .unwrap()
            .0
            .get(&epoch)
            .map(|schedule| schedule[slot_index])
    }

    pub fn slot_leader_at_else_compute(&self, slot: u64, bank: &Bank) -> Option<Pubkey> {
        let cache_result = self.slot_leader_at(slot);
        if cache_result.is_some() {
            cache_result
        } else {
            let (epoch, slot_index) = bank.get_epoch_and_slot_index(slot);
            if let Some(epoch_schedule) = self.compute_epoch_schedule(epoch, bank) {
                Some(epoch_schedule[slot_index])
            } else {
                None
            }
        }
    }

    /// Return the next slot after the given current_slot that the given node will be leader
    pub fn next_leader_slot(
        &self,
        pubkey: &Pubkey,
        mut current_slot: u64,
        bank: &Bank,
        blocktree: Option<&Blocktree>,
    ) -> Option<u64> {
        let (mut epoch, mut start_index) = bank.get_epoch_and_slot_index(current_slot + 1);
        while let Some(leader_schedule) = self.get_epoch_schedule_else_compute(epoch, bank) {
            // clippy thinks I should do this:
            //  for (i, <item>) in leader_schedule
            //                           .iter()
            //                           .enumerate()
            //                           .take(bank.get_slots_in_epoch(epoch))
            //                           .skip(from_slot_index + 1) {
            //
            //  but leader_schedule doesn't implement Iter...
            #[allow(clippy::needless_range_loop)]
            for i in start_index..bank.get_slots_in_epoch(epoch) {
                current_slot += 1;
                if *pubkey == leader_schedule[i] {
                    if let Some(blocktree) = blocktree {
                        if let Some(meta) = blocktree.meta(current_slot).unwrap() {
                            // We have already sent a blob for this slot, so skip it
                            if meta.received > 0 {
                                continue;
                            }
                        }
                    }

                    return Some(current_slot);
                }
            }

            epoch += 1;
            start_index = 0;
        }
        None
    }

    fn get_epoch_schedule_else_compute(
        &self,
        epoch: u64,
        bank: &Bank,
    ) -> Option<Arc<LeaderSchedule>> {
        let epoch_schedule = self.cached_schedules.read().unwrap().0.get(&epoch).cloned();

        if epoch_schedule.is_some() {
            epoch_schedule
        } else if let Some(epoch_schedule) = self.compute_epoch_schedule(epoch, bank) {
            Some(epoch_schedule)
        } else {
            None
        }
    }

    fn compute_epoch_schedule(&self, epoch: u64, bank: &Bank) -> Option<Arc<LeaderSchedule>> {
        let leader_schedule = leader_schedule_utils::leader_schedule(epoch, bank);
        leader_schedule.map(|leader_schedule| {
            let leader_schedule = Arc::new(leader_schedule);
            let (ref mut cached_schedules, ref mut order) = *self.cached_schedules.write().unwrap();
            // Check to see if schedule exists in case somebody already inserted in the time we were
            // waiting for the lock
            let entry = cached_schedules.entry(epoch);
            if let Entry::Vacant(v) = entry {
                v.insert(leader_schedule.clone());
                order.push_back(epoch);
                Self::retain_latest(cached_schedules, order);
            }
            leader_schedule
        })
    }

    fn retain_latest(schedules: &mut HashMap<u64, Arc<LeaderSchedule>>, order: &mut VecDeque<u64>) {
        if schedules.len() > MAX_SCHEDULES {
            let first = order.pop_front().unwrap();
            schedules.remove(&first);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::tests::make_slot_entries;
    use crate::voting_keypair::tests::new_vote_account;
    use solana_runtime::bank::{Bank, EpochSchedule, MINIMUM_SLOT_LENGTH};
    use solana_sdk::genesis_block::{GenesisBlock, BOOTSTRAP_LEADER_LAMPORTS};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::thread::Builder;

    use crate::blocktree::get_tmp_ledger_path;

    #[test]
    fn test_slot_leader_at_else_compute() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let cache = LeaderScheduleCache::new_from_bank(&bank);

        // Nothing in the cache, should return None
        assert!(cache.slot_leader_at(bank.slot()).is_none());

        // Add something to the cache
        assert!(cache
            .slot_leader_at_else_compute(bank.slot(), &bank)
            .is_some());
        assert!(cache.slot_leader_at(bank.slot()).is_some());
        assert_eq!(cache.cached_schedules.read().unwrap().0.len(), 1);
    }

    #[test]
    fn test_retain_latest() {
        let mut cached_schedules = HashMap::new();
        let mut order = VecDeque::new();
        for i in 0..=MAX_SCHEDULES {
            cached_schedules.insert(i as u64, Arc::new(LeaderSchedule::default()));
            order.push_back(i as u64);
        }
        LeaderScheduleCache::retain_latest(&mut cached_schedules, &mut order);
        assert_eq!(cached_schedules.len(), MAX_SCHEDULES);
        let mut keys: Vec<_> = cached_schedules.keys().cloned().collect();
        keys.sort();
        let expected: Vec<_> = (1..=MAX_SCHEDULES as u64).collect();
        let expected_order: VecDeque<_> = (1..=MAX_SCHEDULES as u64).collect();
        assert_eq!(expected, keys);
        assert_eq!(expected_order, order);
    }

    #[test]
    fn test_thread_race_leader_schedule_cache() {
        let num_runs = 10;
        for _ in 0..num_runs {
            run_thread_race()
        }
    }

    fn run_thread_race() {
        let slots_per_epoch = MINIMUM_SLOT_LENGTH as u64;
        let epoch_schedule = EpochSchedule::new(slots_per_epoch, slots_per_epoch / 2, true);
        let cache = Arc::new(LeaderScheduleCache::new(epoch_schedule));
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));

        let num_threads = 10;
        let (threads, senders): (Vec<_>, Vec<_>) = (0..num_threads)
            .map(|_| {
                let cache = cache.clone();
                let bank = bank.clone();
                let (sender, receiver) = channel();
                (
                    Builder::new()
                        .name("test_thread_race_leader_schedule_cache".to_string())
                        .spawn(move || {
                            let _ = receiver.recv();
                            cache.slot_leader_at_else_compute(bank.slot(), &bank);
                        })
                        .unwrap(),
                    sender,
                )
            })
            .unzip();

        for sender in &senders {
            sender.send(true).unwrap();
        }

        for t in threads.into_iter() {
            t.join().unwrap();
        }

        let (ref cached_schedules, ref order) = *cache.cached_schedules.read().unwrap();
        assert_eq!(cached_schedules.len(), 1);
        assert_eq!(order.len(), 1);
    }

    #[test]
    fn test_next_leader_slot() {
        let pubkey = Pubkey::new_rand();
        let mut genesis_block = GenesisBlock::new_with_leader(
            BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        )
        .0;
        genesis_block.epoch_warmup = false;

        let bank = Bank::new(&genesis_block);
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

        assert_eq!(
            cache
                .slot_leader_at_else_compute(bank.slot(), &bank)
                .unwrap(),
            pubkey
        );
        assert_eq!(cache.next_leader_slot(&pubkey, 0, &bank, None), Some(1));
        assert_eq!(cache.next_leader_slot(&pubkey, 1, &bank, None), Some(2));
        assert_eq!(
            cache.next_leader_slot(
                &pubkey,
                2 * genesis_block.slots_per_epoch - 1, // no schedule generated for epoch 2
                &bank,
                None
            ),
            None
        );

        assert_eq!(
            cache.next_leader_slot(
                &Pubkey::new_rand(), // not in leader_schedule
                0,
                &bank,
                None
            ),
            None
        );
    }

    #[test]
    fn test_next_leader_slot_blocktree() {
        let pubkey = Pubkey::new_rand();
        let mut genesis_block = GenesisBlock::new_with_leader(
            BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        )
        .0;
        genesis_block.epoch_warmup = false;

        let bank = Bank::new(&genesis_block);
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );

            assert_eq!(
                cache
                    .slot_leader_at_else_compute(bank.slot(), &bank)
                    .unwrap(),
                pubkey
            );
            // Check that the next leader slot after 0 is slot 1
            assert_eq!(
                cache.next_leader_slot(&pubkey, 0, &bank, Some(&blocktree)),
                Some(1)
            );

            // Write a blob into slot 2 that chains to slot 1,
            // but slot 1 is empty so should not be skipped
            let (blobs, _) = make_slot_entries(2, 1, 1);
            blocktree.write_blobs(&blobs[..]).unwrap();
            assert_eq!(
                cache.next_leader_slot(&pubkey, 0, &bank, Some(&blocktree)),
                Some(1)
            );

            // Write a blob into slot 1
            let (blobs, _) = make_slot_entries(1, 0, 1);

            // Check that slot 1 and 2 are skipped
            blocktree.write_blobs(&blobs[..]).unwrap();
            assert_eq!(
                cache.next_leader_slot(&pubkey, 0, &bank, Some(&blocktree)),
                Some(3)
            );

            // Integrity checks
            assert_eq!(
                cache.next_leader_slot(
                    &pubkey,
                    2 * genesis_block.slots_per_epoch - 1, // no schedule generated for epoch 2
                    &bank,
                    Some(&blocktree)
                ),
                None
            );

            assert_eq!(
                cache.next_leader_slot(
                    &Pubkey::new_rand(), // not in leader_schedule
                    0,
                    &bank,
                    Some(&blocktree)
                ),
                None
            );
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_next_leader_slot_next_epoch() {
        let pubkey = Pubkey::new_rand();
        let (mut genesis_block, mint_keypair) = GenesisBlock::new_with_leader(
            2 * BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        );
        genesis_block.epoch_warmup = false;

        let bank = Bank::new(&genesis_block);
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let delegate_id = Pubkey::new_rand();

        // Create new vote account
        let new_voting_keypair = Keypair::new();
        new_vote_account(
            &mint_keypair,
            &new_voting_keypair,
            &delegate_id,
            &bank,
            BOOTSTRAP_LEADER_LAMPORTS,
        );

        // Have to wait until the epoch at after the epoch stakes generated at genesis
        // for the new votes to take effect.
        let mut target_slot = 1;
        let epoch = bank.get_stakers_epoch(0);
        while bank.get_stakers_epoch(target_slot) == epoch {
            target_slot += 1;
        }

        let bank = Bank::new_from_parent(&Arc::new(bank), &Pubkey::default(), target_slot);
        let mut expected_slot = 0;
        let epoch = bank.get_stakers_epoch(target_slot);
        for i in 0..epoch {
            expected_slot += bank.get_slots_in_epoch(i);
        }

        let schedule = cache.compute_epoch_schedule(epoch, &bank).unwrap();
        let mut index = 0;
        while schedule[index] != delegate_id {
            index += 1
        }

        expected_slot += index;

        assert_eq!(
            cache.next_leader_slot(&delegate_id, 0, &bank, None),
            Some(expected_slot),
        );
    }
}
