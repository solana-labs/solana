/// A helper for calculating a stake-weighted timestamp estimate from a set of timestamps and epoch
/// stake.
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    pubkey::Pubkey,
};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, HashMap},
    time::Duration,
};

// Obsolete limits
const _MAX_ALLOWABLE_DRIFT_PERCENTAGE: u32 = 50;
const _MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW: u32 = 80;

pub(crate) const MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST: u32 = 25;
pub(crate) const MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2: u32 = 150;

#[derive(Copy, Clone)]
pub(crate) struct MaxAllowableDrift {
    pub fast: u32, // Max allowable drift percentage faster than poh estimate
    pub slow: u32, // Max allowable drift percentage slower than poh estimate
}

pub(crate) fn calculate_stake_weighted_timestamp<I, K, V, T>(
    unique_timestamps: I,
    stakes: &HashMap<Pubkey, (u64, T /*Account|VoteAccount*/)>,
    slot: Slot,
    slot_duration: Duration,
    epoch_start_timestamp: Option<(Slot, UnixTimestamp)>,
    max_allowable_drift: MaxAllowableDrift,
    fix_estimate_into_u64: bool,
) -> Option<UnixTimestamp>
where
    I: IntoIterator<Item = (K, V)>,
    K: Borrow<Pubkey>,
    V: Borrow<(Slot, UnixTimestamp)>,
{
    let mut stake_per_timestamp: BTreeMap<UnixTimestamp, u128> = BTreeMap::new();
    let mut total_stake: u128 = 0;
    for (vote_pubkey, slot_timestamp) in unique_timestamps {
        let (timestamp_slot, timestamp) = slot_timestamp.borrow();
        let offset = slot_duration.saturating_mul(slot.saturating_sub(*timestamp_slot) as u32);
        let estimate = timestamp.saturating_add(offset.as_secs() as i64);
        let stake = stakes
            .get(vote_pubkey.borrow())
            .map(|(stake, _account)| stake)
            .unwrap_or(&0);
        stake_per_timestamp
            .entry(estimate)
            .and_modify(|stake_sum| *stake_sum = stake_sum.saturating_add(*stake as u128))
            .or_insert(*stake as u128);
        total_stake = total_stake.saturating_add(*stake as u128);
    }
    if total_stake == 0 {
        return None;
    }
    let mut stake_accumulator: u128 = 0;
    let mut estimate = 0;
    // Populate `estimate` with stake-weighted median timestamp
    for (timestamp, stake) in stake_per_timestamp.into_iter() {
        stake_accumulator = stake_accumulator.saturating_add(stake);
        if stake_accumulator > total_stake / 2 {
            estimate = timestamp;
            break;
        }
    }
    // Bound estimate by `max_allowable_drift` since the start of the epoch
    if let Some((epoch_start_slot, epoch_start_timestamp)) = epoch_start_timestamp {
        let poh_estimate_offset =
            slot_duration.saturating_mul(slot.saturating_sub(epoch_start_slot) as u32);
        let estimate_offset = Duration::from_secs(if fix_estimate_into_u64 {
            (estimate as u64).saturating_sub(epoch_start_timestamp as u64)
        } else {
            estimate.saturating_sub(epoch_start_timestamp) as u64
        });
        let max_allowable_drift_fast =
            poh_estimate_offset.saturating_mul(max_allowable_drift.fast) / 100;
        let max_allowable_drift_slow =
            poh_estimate_offset.saturating_mul(max_allowable_drift.slow) / 100;
        if estimate_offset > poh_estimate_offset
            && estimate_offset.saturating_sub(poh_estimate_offset) > max_allowable_drift_slow
        {
            // estimate offset since the start of the epoch is higher than
            // `max_allowable_drift_slow`
            estimate = epoch_start_timestamp
                .saturating_add(poh_estimate_offset.as_secs() as i64)
                .saturating_add(max_allowable_drift_slow.as_secs() as i64);
        } else if estimate_offset < poh_estimate_offset
            && poh_estimate_offset.saturating_sub(estimate_offset) > max_allowable_drift_fast
        {
            // estimate offset since the start of the epoch is lower than
            // `max_allowable_drift_fast`
            estimate = epoch_start_timestamp
                .saturating_add(poh_estimate_offset.as_secs() as i64)
                .saturating_sub(max_allowable_drift_fast.as_secs() as i64);
        }
    }
    Some(estimate)
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_sdk::{account::Account, native_token::sol_to_lamports},
    };

    #[test]
    fn test_calculate_stake_weighted_timestamp_uses_median() {
        let recent_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 5;
        let slot_duration = Duration::from_millis(400);
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let pubkey3 = solana_sdk::pubkey::new_rand();
        let pubkey4 = solana_sdk::pubkey::new_rand();
        let max_allowable_drift = MaxAllowableDrift { fast: 25, slow: 25 };

        // Test low-staked outlier(s)
        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (sol_to_lamports(1.0), Account::new(1, 0, &Pubkey::default())),
            ),
            (
                pubkey1,
                (sol_to_lamports(1.0), Account::new(1, 0, &Pubkey::default())),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey3,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey4,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, 0)),
            (pubkey1, (5, recent_timestamp)),
            (pubkey2, (5, recent_timestamp)),
            (pubkey3, (5, recent_timestamp)),
            (pubkey4, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
            max_allowable_drift,
            true,
        )
        .unwrap();
        // With no bounding, timestamp w/ 0.00003% of the stake can shift the timestamp backward 8min
        assert_eq!(bounded, recent_timestamp); // low-staked outlier cannot affect bounded timestamp

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, recent_timestamp)),
            (pubkey1, (5, i64::MAX)),
            (pubkey2, (5, recent_timestamp)),
            (pubkey3, (5, recent_timestamp)),
            (pubkey4, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
            max_allowable_drift,
            true,
        )
        .unwrap();
        // With no bounding, timestamp w/ 0.00003% of the stake can shift the timestamp forward 97k years!
        assert_eq!(bounded, recent_timestamp); // low-staked outlier cannot affect bounded timestamp

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, 0)),
            (pubkey1, (5, i64::MAX)),
            (pubkey2, (5, recent_timestamp)),
            (pubkey3, (5, recent_timestamp)),
            (pubkey4, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, recent_timestamp); // multiple low-staked outliers cannot affect bounded timestamp if they don't shift the median

        // Test higher-staked outlier(s)
        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0), // 1/3 stake
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, 0)),
            (pubkey1, (5, i64::MAX)),
            (pubkey2, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, recent_timestamp); // outlier(s) cannot affect bounded timestamp if they don't shift the median

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_001.0), // 1/3 stake
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> =
            [(pubkey0, (5, 0)), (pubkey1, (5, recent_timestamp))]
                .iter()
                .cloned()
                .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(recent_timestamp - bounded, 1578909061); // outliers > 1/2 of available stake can affect timestamp
    }

    #[test]
    fn test_calculate_stake_weighted_timestamp_poh() {
        let epoch_start_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 20;
        let slot_duration = Duration::from_millis(400);
        let poh_offset = (slot * slot_duration).as_secs();
        let max_allowable_drift_percentage = 25;
        let max_allowable_drift = MaxAllowableDrift {
            fast: max_allowable_drift_percentage,
            slow: max_allowable_drift_percentage,
        };
        let acceptable_delta = (max_allowable_drift_percentage * poh_offset as u32 / 100) as i64;
        let poh_estimate = epoch_start_timestamp + poh_offset as i64;
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        // Test when stake-weighted median is too high
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate + acceptable_delta + 1)),
            (pubkey1, (slot as u64, poh_estimate + acceptable_delta + 1)),
            (pubkey2, (slot as u64, poh_estimate + acceptable_delta + 1)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta);

        // Test when stake-weighted median is too low
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate - acceptable_delta - 1)),
            (pubkey1, (slot as u64, poh_estimate - acceptable_delta - 1)),
            (pubkey2, (slot as u64, poh_estimate - acceptable_delta - 1)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate - acceptable_delta);

        // Test stake-weighted median within bounds
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate + acceptable_delta)),
            (pubkey1, (slot as u64, poh_estimate + acceptable_delta)),
            (pubkey2, (slot as u64, poh_estimate + acceptable_delta)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta);

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate - acceptable_delta)),
            (pubkey1, (slot as u64, poh_estimate - acceptable_delta)),
            (pubkey2, (slot as u64, poh_estimate - acceptable_delta)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate - acceptable_delta);
    }

    #[test]
    fn test_calculate_stake_weighted_timestamp_levels() {
        let epoch_start_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 20;
        let slot_duration = Duration::from_millis(400);
        let poh_offset = (slot * slot_duration).as_secs();
        let max_allowable_drift_percentage_25 = 25;
        let allowable_drift_25 = MaxAllowableDrift {
            fast: max_allowable_drift_percentage_25,
            slow: max_allowable_drift_percentage_25,
        };
        let max_allowable_drift_percentage_50 = 50;
        let allowable_drift_50 = MaxAllowableDrift {
            fast: max_allowable_drift_percentage_50,
            slow: max_allowable_drift_percentage_50,
        };
        let acceptable_delta_25 =
            (max_allowable_drift_percentage_25 * poh_offset as u32 / 100) as i64;
        let acceptable_delta_50 =
            (max_allowable_drift_percentage_50 * poh_offset as u32 / 100) as i64;
        assert!(acceptable_delta_50 > acceptable_delta_25 + 1);
        let poh_estimate = epoch_start_timestamp + poh_offset as i64;
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        // Test when stake-weighted median is above 25% deviance but below 50% deviance
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (
                pubkey0,
                (slot as u64, poh_estimate + acceptable_delta_25 + 1),
            ),
            (
                pubkey1,
                (slot as u64, poh_estimate + acceptable_delta_25 + 1),
            ),
            (
                pubkey2,
                (slot as u64, poh_estimate + acceptable_delta_25 + 1),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            allowable_drift_25,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta_25);

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            allowable_drift_50,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta_25 + 1);

        // Test when stake-weighted median is above 50% deviance
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (
                pubkey0,
                (slot as u64, poh_estimate + acceptable_delta_50 + 1),
            ),
            (
                pubkey1,
                (slot as u64, poh_estimate + acceptable_delta_50 + 1),
            ),
            (
                pubkey2,
                (slot as u64, poh_estimate + acceptable_delta_50 + 1),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            allowable_drift_25,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta_25);

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            allowable_drift_50,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta_50);
    }

    #[test]
    fn test_calculate_stake_weighted_timestamp_fast_slow() {
        let epoch_start_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 20;
        let slot_duration = Duration::from_millis(400);
        let poh_offset = (slot * slot_duration).as_secs();
        let max_allowable_drift_percentage_25 = 25;
        let max_allowable_drift_percentage_50 = 50;
        let max_allowable_drift = MaxAllowableDrift {
            fast: max_allowable_drift_percentage_25,
            slow: max_allowable_drift_percentage_50,
        };
        let acceptable_delta_fast =
            (max_allowable_drift_percentage_25 * poh_offset as u32 / 100) as i64;
        let acceptable_delta_slow =
            (max_allowable_drift_percentage_50 * poh_offset as u32 / 100) as i64;
        assert!(acceptable_delta_slow > acceptable_delta_fast + 1);
        let poh_estimate = epoch_start_timestamp + poh_offset as i64;
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        // Test when stake-weighted median is more than 25% fast
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (
                pubkey0,
                (slot as u64, poh_estimate - acceptable_delta_fast - 1),
            ),
            (
                pubkey1,
                (slot as u64, poh_estimate - acceptable_delta_fast - 1),
            ),
            (
                pubkey2,
                (slot as u64, poh_estimate - acceptable_delta_fast - 1),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate - acceptable_delta_fast);

        // Test when stake-weighted median is more than 25% but less than 50% slow
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (
                pubkey0,
                (slot as u64, poh_estimate + acceptable_delta_fast + 1),
            ),
            (
                pubkey1,
                (slot as u64, poh_estimate + acceptable_delta_fast + 1),
            ),
            (
                pubkey2,
                (slot as u64, poh_estimate + acceptable_delta_fast + 1),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta_fast + 1);

        // Test when stake-weighted median is more than 50% slow
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (
                pubkey0,
                (slot as u64, poh_estimate + acceptable_delta_slow + 1),
            ),
            (
                pubkey1,
                (slot as u64, poh_estimate + acceptable_delta_slow + 1),
            ),
            (
                pubkey2,
                (slot as u64, poh_estimate + acceptable_delta_slow + 1),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta_slow);
    }

    #[test]
    fn test_calculate_stake_weighted_timestamp_early() {
        let epoch_start_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 20;
        let slot_duration = Duration::from_millis(400);
        let poh_offset = (slot * slot_duration).as_secs();
        let max_allowable_drift_percentage = 50;
        let max_allowable_drift = MaxAllowableDrift {
            fast: max_allowable_drift_percentage,
            slow: max_allowable_drift_percentage,
        };
        let acceptable_delta = (max_allowable_drift_percentage * poh_offset as u32 / 100) as i64;
        let poh_estimate = epoch_start_timestamp + poh_offset as i64;
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        // Test when stake-weighted median is before epoch_start_timestamp
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate - acceptable_delta - 20)),
            (pubkey1, (slot as u64, poh_estimate - acceptable_delta - 20)),
            (pubkey2, (slot as u64, poh_estimate - acceptable_delta - 20)),
        ]
        .iter()
        .cloned()
        .collect();

        // Without fix, median timestamps before epoch_start_timestamp actually increase the time
        // estimate due to incorrect casting.
        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            false,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta);

        let bounded = calculate_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
            max_allowable_drift,
            true,
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate - acceptable_delta);
    }
}
