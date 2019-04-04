use crate::leader_schedule::LeaderSchedule;
use crate::leader_schedule_utils;
use solana_runtime::bank::{Bank, EpochSchedule};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::RwLock;

const MAX_SCHEDULES: usize = 10;

pub struct LeaderScheduleCache {
    // Map from an epoch to a leader schedule for that epoch
    pub cached_schedules: RwLock<HashMap<u64, LeaderSchedule>>,
    epoch_schedule: EpochSchedule,
}

impl LeaderScheduleCache {
    pub fn new(epoch_schedule: EpochSchedule) -> Self {
        Self {
            cached_schedules: RwLock::new(HashMap::new()),
            epoch_schedule,
        }
    }

    pub fn slot_leader_at(&self, slot: u64) -> Option<Pubkey> {
        let (epoch, slot_index) = self.epoch_schedule.get_epoch_and_slot_index(slot);
        self.cached_schedules
            .read()
            .unwrap()
            .get(&epoch)
            .map(|schedule| schedule[slot_index])
    }

    pub fn slot_leader_at_else_compute(&self, slot: u64, bank: &Bank) -> Option<Pubkey> {
        let cache_result = self.slot_leader_at(slot);
        if cache_result.is_some() {
            cache_result
        } else {
            let (epoch, slot_index) = bank.get_epoch_and_slot_index(slot);
            let leader_schedule = leader_schedule_utils::leader_schedule(epoch, bank);
            leader_schedule.map(|leader_schedule| {
                let leader = leader_schedule[slot_index];
                let mut cached_schedules = self.cached_schedules.write().unwrap();
                cached_schedules.insert(epoch, leader_schedule);
                Self::retain_latest(&mut cached_schedules);
                leader
            })
        }
    }

    fn retain_latest(schedules: &mut HashMap<u64, LeaderSchedule>) {
        if schedules.len() > MAX_SCHEDULES {
            let mut keys: Vec<_> = schedules.keys().cloned().collect();
            keys.sort();
            let lowest = keys.first();
            if let Some(lowest) = lowest {
                schedules.remove(lowest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_runtime::bank::{Bank, EpochSchedule};
    use solana_sdk::genesis_block::GenesisBlock;

    #[test]
    fn test_slot_leader_at_else_compute() {
        let slots_per_epoch = 10;
        let epoch_schedule = EpochSchedule::new(slots_per_epoch, slots_per_epoch / 2, true);
        let cache = LeaderScheduleCache::new(epoch_schedule);
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);

        // Nothing in the cache, should return None
        assert!(cache.slot_leader_at(bank.slot()).is_none());

        // Add something to the cache
        assert!(cache
            .slot_leader_at_else_compute(bank.slot(), &bank)
            .is_some());
        assert!(cache.slot_leader_at(bank.slot()).is_some());
        assert_eq!(cache.cached_schedules.read().unwrap().len(), 1);
    }

    #[test]
    fn test_retain_latest() {
        let mut cached_schedules = HashMap::new();
        for i in 0..=MAX_SCHEDULES {
            cached_schedules.insert(i as u64, LeaderSchedule::default());
        }
        LeaderScheduleCache::retain_latest(&mut cached_schedules);
        assert_eq!(cached_schedules.len(), MAX_SCHEDULES);
        let mut keys: Vec<_> = cached_schedules.keys().cloned().collect();
        keys.sort();
        let expected: Vec<_> = (1..=MAX_SCHEDULES as u64).collect();
        assert_eq!(expected, keys);
    }
}
