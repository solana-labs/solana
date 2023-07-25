//! Utility functions and types for Epoch Accounts Hash

use {
    crate::bank::Bank,
    solana_sdk::{
        clock::{Epoch, Slot},
        vote::state::MAX_LOCKOUT_HISTORY,
    },
};

/// Is the EAH enabled this Epoch?
#[must_use]
pub fn is_enabled_this_epoch(bank: &Bank) -> bool {
    // The EAH calculation "start" is based on when a bank is *rooted*, and "stop" is based on when a
    // bank is *frozen*.  Banks are rooted after exceeding the maximum lockout, so there is a delay
    // of at least `maximum lockout` number of slots the EAH calculation must take into
    // consideration.  To ensure an EAH calculation has started by the time that calculation is
    // needed, the calculation interval must be at least `maximum lockout` plus some buffer to
    // handle when banks are not rooted every single slot.
    const MINIMUM_CALCULATION_INTERVAL: u64 =
        (MAX_LOCKOUT_HISTORY as u64).saturating_add(CALCULATION_INTERVAL_BUFFER);
    // The calculation buffer is a best-attempt at median worst-case for how many bank ancestors can
    // accumulate before the bank is rooted.
    // [brooks] On Wed Oct 26 12:15:21 2022, over the previous 6 hour period against mainnet-beta,
    // I saw multiple validators reporting metrics in the 120s for `total_parent_banks`.  The mean
    // is 2 to 3, but a number of nodes also reported values in the low 20s.  A value of 150 should
    // capture the majority of validators, and will not be an issue for clusters running with
    // normal slots-per-epoch; this really will only affect tests and epoch schedule warmup.
    const CALCULATION_INTERVAL_BUFFER: u64 = 150;

    let calculation_interval = calculation_interval(bank);
    calculation_interval >= MINIMUM_CALCULATION_INTERVAL
}

/// Calculation of the EAH occurs once per epoch.  All nodes in the cluster must agree on which
/// slot the EAH is based on.  This slot will be at an offset into the epoch, and referred to as
/// the "start" slot for the EAH calculation.
#[must_use]
#[inline]
pub fn calculation_offset_start(bank: &Bank) -> Slot {
    calculation_info(bank).calculation_offset_start
}

/// Calculation of the EAH occurs once per epoch.  All nodes in the cluster must agree on which
/// bank will hash the EAH into its `Bank::hash`.  This slot will be at an offset into the epoch,
/// and referred to as the "stop" slot for the EAH calculation.  All nodes must complete the EAH
/// calculation before this slot!
#[must_use]
#[inline]
pub fn calculation_offset_stop(bank: &Bank) -> Slot {
    calculation_info(bank).calculation_offset_stop
}

/// For the epoch that `bank` is in, get the slot that the EAH calculation starts
#[must_use]
#[inline]
pub fn calculation_start(bank: &Bank) -> Slot {
    calculation_info(bank).calculation_start
}

/// For the epoch that `bank` is in, get the slot that the EAH calculation stops
#[must_use]
#[inline]
pub fn calculation_stop(bank: &Bank) -> Slot {
    calculation_info(bank).calculation_stop
}

/// Get the number of slots from EAH calculation start to stop; known as the calculation interval
#[must_use]
#[inline]
pub fn calculation_interval(bank: &Bank) -> u64 {
    calculation_info(bank).calculation_interval
}

/// Is this bank in the calculation window?
#[must_use]
pub fn is_in_calculation_window(bank: &Bank) -> bool {
    let info = calculation_info(bank);
    let range = info.calculation_start..info.calculation_stop;
    range.contains(&bank.slot())
}

/// For the epoch that `bank` is in, get all the EAH calculation information
pub fn calculation_info(bank: &Bank) -> CalculationInfo {
    let epoch = bank.epoch();
    let epoch_schedule = bank.epoch_schedule();

    let slots_per_epoch = epoch_schedule.get_slots_in_epoch(epoch);
    let calculation_offset_start = slots_per_epoch / 4;
    let calculation_offset_stop = slots_per_epoch / 4 * 3;

    let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);
    let last_slot_in_epoch = epoch_schedule.get_last_slot_in_epoch(epoch);
    let calculation_start = first_slot_in_epoch.saturating_add(calculation_offset_start);
    let calculation_stop = first_slot_in_epoch.saturating_add(calculation_offset_stop);
    let calculation_interval = calculation_offset_stop.saturating_sub(calculation_offset_start);

    CalculationInfo {
        epoch,
        slots_per_epoch,
        first_slot_in_epoch,
        last_slot_in_epoch,
        calculation_offset_start,
        calculation_offset_stop,
        calculation_start,
        calculation_stop,
        calculation_interval,
    }
}

/// All the EAH calculation information for a specific epoch
///
/// Computing the EAH calculation information looks up a bunch of values.  Instead of throwing
/// those values away, they are kept in here as well.  This may aid in future debugging, and the
/// additional fields are trivial in size.
#[derive(Debug, Default, Copy, Clone)]
pub struct CalculationInfo {
    /*
     * The values that were looked up, which were needed to get the calculation info
     */
    /// The epoch this information applies to
    pub epoch: Epoch,
    /// Number of slots in this epoch
    pub slots_per_epoch: u64,
    /// First slot in this epoch
    pub first_slot_in_epoch: Slot,
    /// Last slot in this epoch
    pub last_slot_in_epoch: Slot,

    /*
     * The computed values for the calculation info
     */
    /// Offset into the epoch when the EAH calculation starts
    pub calculation_offset_start: Slot,
    /// Offset into the epoch when the EAH calculation stops
    pub calculation_offset_stop: Slot,
    /// Absolute slot where the EAH calculation starts
    pub calculation_start: Slot,
    /// Absolute slot where the EAH calculation stops
    pub calculation_stop: Slot,
    /// Number of slots from EAH calculation start to stop
    pub calculation_interval: u64,
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{epoch_schedule::EpochSchedule, genesis_config::GenesisConfig},
        test_case::test_case,
    };

    #[test_case(     32 => false)] // minimum slots per epoch
    #[test_case(    361 => false)] // below minimum slots per epoch *for EAH*
    #[test_case(    362 => false)] // minimum slots per epoch *for EAH*
    #[test_case(  8_192 => true)] // default dev slots per epoch
    #[test_case(432_000 => true)] // default slots per epoch
    fn test_is_enabled_this_epoch(slots_per_epoch: u64) -> bool {
        let genesis_config = GenesisConfig {
            epoch_schedule: EpochSchedule::custom(slots_per_epoch, slots_per_epoch, false),
            ..GenesisConfig::default()
        };
        let bank = Bank::new_for_tests(&genesis_config);
        is_enabled_this_epoch(&bank)
    }

    #[test]
    fn test_calculation_offset_bounds() {
        let bank = Bank::default_for_tests();
        let offset_start = calculation_offset_start(&bank);
        let offset_stop = calculation_offset_stop(&bank);
        assert!(offset_start < offset_stop);
    }

    #[test]
    fn test_calculation_bounds() {
        let bank = Bank::default_for_tests();
        let start = calculation_start(&bank);
        let stop = calculation_stop(&bank);
        assert!(start < stop);
    }

    #[test]
    fn test_calculation_info() {
        for slots_per_epoch in [32, 361, 362, 8_192, 65_536, 432_000, 123_456_789] {
            for warmup in [false, true] {
                let genesis_config = GenesisConfig {
                    epoch_schedule: EpochSchedule::custom(slots_per_epoch, slots_per_epoch, warmup),
                    ..GenesisConfig::default()
                };
                let info = calculation_info(&Bank::new_for_tests(&genesis_config));
                assert!(info.calculation_offset_start < info.calculation_offset_stop);
                assert!(info.calculation_offset_start < info.slots_per_epoch);
                assert!(info.calculation_offset_stop < info.slots_per_epoch);
                assert!(info.calculation_start < info.calculation_stop);
                assert!(info.calculation_start > info.first_slot_in_epoch);
                assert!(info.calculation_stop < info.last_slot_in_epoch);
                assert!(info.calculation_interval > 0);
            }
        }
    }
}
