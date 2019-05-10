use solana_vote_api::vote_state::MAX_LOCKOUT_HISTORY;

pub const MINIMUM_SLOT_LENGTH: usize = MAX_LOCKOUT_HISTORY + 1;

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy, Deserialize, Serialize)]
pub struct EpochSchedule {
    /// The maximum number of slots in each epoch.
    pub slots_per_epoch: u64,

    /// A number of slots before slot_index 0. Used to calculate finalized staked nodes.
    pub stakers_slot_offset: u64,

    /// basically: log2(slots_per_epoch) - log2(MINIMUM_SLOT_LEN)
    pub first_normal_epoch: u64,

    /// basically: 2.pow(first_normal_epoch) - MINIMUM_SLOT_LEN
    pub first_normal_slot: u64,
}

impl EpochSchedule {
    pub fn new(slots_per_epoch: u64, stakers_slot_offset: u64, warmup: bool) -> Self {
        assert!(slots_per_epoch >= MINIMUM_SLOT_LENGTH as u64);
        let (first_normal_epoch, first_normal_slot) = if warmup {
            let next_power_of_two = slots_per_epoch.next_power_of_two();
            let log2_slots_per_epoch = next_power_of_two
                .trailing_zeros()
                .saturating_sub(MINIMUM_SLOT_LENGTH.trailing_zeros());

            (
                u64::from(log2_slots_per_epoch),
                next_power_of_two.saturating_sub(MINIMUM_SLOT_LENGTH as u64),
            )
        } else {
            (0, 0)
        };
        EpochSchedule {
            slots_per_epoch,
            stakers_slot_offset,
            first_normal_epoch,
            first_normal_slot,
        }
    }

    /// get the length of the given epoch (in slots)
    pub fn get_slots_in_epoch(&self, epoch: u64) -> u64 {
        if epoch < self.first_normal_epoch {
            2u64.pow(epoch as u32 + MINIMUM_SLOT_LENGTH.trailing_zeros() as u32)
        } else {
            self.slots_per_epoch
        }
    }

    /// get the epoch for which the given slot should save off
    ///  information about stakers
    pub fn get_stakers_epoch(&self, slot: u64) -> u64 {
        if slot < self.first_normal_slot {
            // until we get to normal slots, behave as if stakers_slot_offset == slots_per_epoch
            self.get_epoch_and_slot_index(slot).0 + 1
        } else {
            self.first_normal_epoch
                + (slot - self.first_normal_slot + self.stakers_slot_offset) / self.slots_per_epoch
        }
    }

    /// get epoch and offset into the epoch for the given slot
    pub fn get_epoch_and_slot_index(&self, slot: u64) -> (u64, u64) {
        if slot < self.first_normal_slot {
            let epoch = (slot + MINIMUM_SLOT_LENGTH as u64 + 1)
                .next_power_of_two()
                .trailing_zeros()
                - MINIMUM_SLOT_LENGTH.trailing_zeros()
                - 1;

            let epoch_len = 2u64.pow(epoch + MINIMUM_SLOT_LENGTH.trailing_zeros());

            (
                u64::from(epoch),
                slot - (epoch_len - MINIMUM_SLOT_LENGTH as u64),
            )
        } else {
            (
                self.first_normal_epoch + ((slot - self.first_normal_slot) / self.slots_per_epoch),
                (slot - self.first_normal_slot) % self.slots_per_epoch,
            )
        }
    }

    pub fn get_first_slot_in_epoch(&self, epoch: u64) -> u64 {
        if epoch <= self.first_normal_epoch {
            (2u64.pow(epoch as u32) - 1) * MINIMUM_SLOT_LENGTH as u64
        } else {
            (epoch - self.first_normal_epoch) * self.slots_per_epoch + self.first_normal_slot
        }
    }

    pub fn get_last_slot_in_epoch(&self, epoch: u64) -> u64 {
        self.get_first_slot_in_epoch(epoch) + self.get_slots_in_epoch(epoch) - 1
    }
}
