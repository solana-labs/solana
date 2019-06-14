use solana_vote_api::vote_state::MAX_LOCKOUT_HISTORY;

pub const MINIMUM_SLOTS_PER_EPOCH: u64 = (MAX_LOCKOUT_HISTORY + 1) as u64;

#[derive(Default, Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
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
        assert!(slots_per_epoch >= MINIMUM_SLOTS_PER_EPOCH as u64);
        let (first_normal_epoch, first_normal_slot) = if warmup {
            let next_power_of_two = slots_per_epoch.next_power_of_two();
            let log2_slots_per_epoch = next_power_of_two
                .trailing_zeros()
                .saturating_sub(MINIMUM_SLOTS_PER_EPOCH.trailing_zeros());

            (
                u64::from(log2_slots_per_epoch),
                next_power_of_two.saturating_sub(MINIMUM_SLOTS_PER_EPOCH),
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
            2u64.pow(epoch as u32 + MINIMUM_SLOTS_PER_EPOCH.trailing_zeros() as u32)
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

    /// get epoch for the given slot
    pub fn get_epoch(&self, slot: u64) -> u64 {
        self.get_epoch_and_slot_index(slot).0
    }

    /// get epoch and offset into the epoch for the given slot
    pub fn get_epoch_and_slot_index(&self, slot: u64) -> (u64, u64) {
        if slot < self.first_normal_slot {
            let epoch = (slot + MINIMUM_SLOTS_PER_EPOCH + 1)
                .next_power_of_two()
                .trailing_zeros()
                - MINIMUM_SLOTS_PER_EPOCH.trailing_zeros()
                - 1;

            let epoch_len = 2u64.pow(epoch + MINIMUM_SLOTS_PER_EPOCH.trailing_zeros());

            (
                u64::from(epoch),
                slot - (epoch_len - MINIMUM_SLOTS_PER_EPOCH),
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
            (2u64.pow(epoch as u32) - 1) * MINIMUM_SLOTS_PER_EPOCH
        } else {
            (epoch - self.first_normal_epoch) * self.slots_per_epoch + self.first_normal_slot
        }
    }

    pub fn get_last_slot_in_epoch(&self, epoch: u64) -> u64 {
        self.get_first_slot_in_epoch(epoch) + self.get_slots_in_epoch(epoch) - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_schedule() {
        // one week of slots at 8 ticks/slot, 10 ticks/sec is
        // (1 * 7 * 24 * 4500u64).next_power_of_two();

        // test values between MINIMUM_SLOT_LEN and MINIMUM_SLOT_LEN * 16, should cover a good mix
        for slots_per_epoch in MINIMUM_SLOTS_PER_EPOCH..=MINIMUM_SLOTS_PER_EPOCH * 16 {
            let epoch_schedule = EpochSchedule::new(slots_per_epoch, slots_per_epoch / 2, true);

            assert_eq!(epoch_schedule.get_first_slot_in_epoch(0), 0);
            assert_eq!(
                epoch_schedule.get_last_slot_in_epoch(0),
                MINIMUM_SLOTS_PER_EPOCH - 1
            );

            let mut last_stakers = 0;
            let mut last_epoch = 0;
            let mut last_slots_in_epoch = MINIMUM_SLOTS_PER_EPOCH;
            for slot in 0..(2 * slots_per_epoch) {
                // verify that stakers_epoch is continuous over the warmup
                // and into the first normal epoch

                let stakers = epoch_schedule.get_stakers_epoch(slot);
                if stakers != last_stakers {
                    assert_eq!(stakers, last_stakers + 1);
                    last_stakers = stakers;
                }

                let (epoch, offset) = epoch_schedule.get_epoch_and_slot_index(slot);

                //  verify that epoch increases continuously
                if epoch != last_epoch {
                    assert_eq!(epoch, last_epoch + 1);
                    last_epoch = epoch;
                    assert_eq!(epoch_schedule.get_first_slot_in_epoch(epoch), slot);
                    assert_eq!(epoch_schedule.get_last_slot_in_epoch(epoch - 1), slot - 1);

                    // verify that slots in an epoch double continuously
                    //   until they reach slots_per_epoch

                    let slots_in_epoch = epoch_schedule.get_slots_in_epoch(epoch);
                    if slots_in_epoch != last_slots_in_epoch {
                        if slots_in_epoch != slots_per_epoch {
                            assert_eq!(slots_in_epoch, last_slots_in_epoch * 2);
                        }
                    }
                    last_slots_in_epoch = slots_in_epoch;
                }
                // verify that the slot offset is less than slots_in_epoch
                assert!(offset < last_slots_in_epoch);
            }

            // assert that these changed  ;)
            assert!(last_stakers != 0); // t
            assert!(last_epoch != 0);
            // assert that we got to "normal" mode
            assert!(last_slots_in_epoch == slots_per_epoch);
        }
    }
}
