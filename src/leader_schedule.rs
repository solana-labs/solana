use crate::leader_scheduler::LeaderScheduler;

use solana_sdk::pubkey::Pubkey;

pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,
    ticks_per_slot: u64,
    ticks_per_epoch: u64,
    current_epoch: u64,
}

impl LeaderSchedule {
    pub fn new(
        slot_leaders: Vec<Pubkey>,
        current_epoch: u64,
        ticks_per_slot: u64,
        ticks_per_epoch: u64,
    ) -> Self {
        Self {
            current_epoch,
            slot_leaders,
            ticks_per_slot,
            ticks_per_epoch,
        }
    }

    pub fn get_leader_for_slot(&self, slot_height: u64) -> Option<Pubkey> {
        let tick_height = slot_height * self.ticks_per_slot;
        let epoch = LeaderScheduler::tick_height_to_epoch(tick_height, self.ticks_per_slot);

        if epoch != self.current_epoch {
            warn!(
                "get_leader_for_slot: leader unknown for epoch {}, which is not equal to {}",
                epoch, self.current_epoch
            );
            None
        } else {
            let first_tick_in_epoch = epoch * self.ticks_per_epoch;

            let slot_index = (tick_height - first_tick_in_epoch) / self.ticks_per_slot;

            // Round robin through each node in the schedule
            Some(self.slot_leaders[slot_index as usize % self.slot_leaders.len()])
        }
    }
}
